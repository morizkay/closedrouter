//! Request and response translation between OpenAI, Anthropic, DeepSeek, and GLM.

use crate::error::{AppError, AppResult};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// Wire protocol a client or upstream speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    OpenAi,
    Anthropic,
    DeepSeek,
    Glm,
}

impl Protocol {
    /// Map a stored provider `kind` to a protocol.
    pub fn from_kind(kind: &str) -> AppResult<Self> {
        match kind {
            "openai" => Ok(Protocol::OpenAi),
            "anthropic" => Ok(Protocol::Anthropic),
            "deepseek" => Ok(Protocol::DeepSeek),
            "glm" | "zhipu" | "chatglm" => Ok(Protocol::Glm),
            other => Err(AppError::BadRequest(format!(
                "unsupported provider kind '{other}'"
            ))),
        }
    }

    /// OpenAI, DeepSeek, and GLM share the Chat Completions family.
    pub fn is_openai_family(self) -> bool {
        !matches!(self, Protocol::Anthropic)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::OpenAi => "openai",
            Protocol::Anthropic => "anthropic",
            Protocol::DeepSeek => "deepseek",
            Protocol::Glm => "glm",
        }
    }

    /// Relative chat path appended to the provider `base_url`.
    pub fn chat_path(self) -> &'static str {
        match self {
            Protocol::Anthropic => "messages",
            Protocol::Glm => "chat/completions",
            Protocol::OpenAi | Protocol::DeepSeek => "chat/completions",
        }
    }
}

pub fn extract_model(body: &Value) -> AppResult<&str> {
    body.get("model")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::BadRequest("model is required".into()))
}

pub fn extract_stream(body: &Value) -> bool {
    body.get("stream").and_then(Value::as_bool).unwrap_or(false)
}

pub fn include_usage(body: &Value) -> bool {
    body.pointer("/stream_options/include_usage")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Rewrite `model` while preserving extra fields (tools, reasoning_content, GLM thinking, …).
pub fn rewrite_model(mut body: Value, upstream_model: &str) -> Value {
    if let Some(obj) = body.as_object_mut() {
        obj.insert("model".into(), Value::String(upstream_model.to_string()));
    }
    body
}

/// OpenAI-family (OpenAI / DeepSeek / GLM) → Anthropic Messages.
pub fn openai_to_anthropic(body: &Value, upstream_model: &str) -> AppResult<Value> {
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::BadRequest("messages is required".into()))?;

    let mut system_parts: Vec<String> = Vec::new();
    let mut converted: Vec<Value> = Vec::new();

    for msg in messages {
        let role = msg.get("role").and_then(Value::as_str).unwrap_or("user");
        let content = flatten_content(msg.get("content"));
        let reasoning = flatten_optional(msg.get("reasoning_content"))
            .or_else(|| flatten_optional(msg.get("reasoning")));
        match role {
            "system" => {
                if !content.is_empty() {
                    system_parts.push(content);
                }
            }
            "user" | "assistant" => {
                let mut content_blocks: Vec<Value> = Vec::new();
                if let Some(reason) = reasoning {
                    if !reason.is_empty() {
                        content_blocks.push(json!({
                            "type": "text",
                            "text": format!("[reasoning]\n{reason}")
                        }));
                    }
                }
                if let Some(tool_calls) = msg.get("tool_calls").and_then(Value::as_array) {
                    for call in tool_calls {
                        let id = call
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("toolu_closedrouter");
                        let name = call
                            .pointer("/function/name")
                            .and_then(Value::as_str)
                            .unwrap_or("tool");
                        let input_raw = call
                            .pointer("/function/arguments")
                            .and_then(Value::as_str)
                            .unwrap_or("{}");
                        let input = serde_json::from_str::<Value>(input_raw)
                            .unwrap_or_else(|_| json!({ "raw": input_raw }));
                        content_blocks.push(json!({
                            "type": "tool_use",
                            "id": id,
                            "name": name,
                            "input": input
                        }));
                    }
                }
                if !content.is_empty() {
                    content_blocks.push(json!({"type": "text", "text": content}));
                }
                let payload = if content_blocks.len() == 1
                    && content_blocks[0].get("type").and_then(Value::as_str) == Some("text")
                {
                    json!({
                        "role": role,
                        "content": content_blocks[0].get("text").cloned().unwrap_or(Value::String(content)),
                    })
                } else if content_blocks.is_empty() {
                    json!({"role": role, "content": content})
                } else {
                    json!({"role": role, "content": content_blocks})
                };
                converted.push(payload);
            }
            "tool" => {
                let tool_id = msg
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("toolu_closedrouter");
                converted.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": tool_id,
                        "content": content
                    }]
                }));
            }
            other => {
                converted.push(json!({
                    "role": "user",
                    "content": format!("[{other}] {content}"),
                }));
            }
        }
    }

    if converted.is_empty() {
        return Err(AppError::BadRequest(
            "at least one user or assistant message is required".into(),
        ));
    }

    let max_tokens = body
        .get("max_tokens")
        .and_then(Value::as_u64)
        .or_else(|| body.get("max_completion_tokens").and_then(Value::as_u64))
        .unwrap_or(4096);

    let mut out = json!({
        "model": upstream_model,
        "messages": converted,
        "max_tokens": max_tokens,
    });

    if let Some(obj) = out.as_object_mut() {
        if !system_parts.is_empty() {
            obj.insert("system".into(), Value::String(system_parts.join("\n\n")));
        }
        copy_if_present(body, obj, "stream");
        copy_if_present(body, obj, "temperature");
        copy_if_present(body, obj, "top_p");
        // Anthropic rejects unknown fields such as OpenAI's `stop`.
        if let Some(stop) = body.get("stop") {
            if stop.is_string() {
                obj.insert("stop_sequences".into(), json!([stop]));
            } else if stop.is_array() {
                obj.insert("stop_sequences".into(), stop.clone());
            }
        }
        if let Some(tools) = openai_tools_to_anthropic(body.get("tools")) {
            obj.insert("tools".into(), tools);
        }
        if body
            .get("thinking")
            .and_then(|t| t.get("type"))
            .and_then(Value::as_str)
            == Some("enabled")
        {
            obj.insert(
                "thinking".into(),
                json!({"type": "enabled", "budget_tokens": 8000}),
            );
        }
    }

    Ok(out)
}

/// Anthropic Messages → OpenAI-family Chat Completions (DeepSeek/GLM extras included).
pub fn anthropic_to_openai(body: &Value, upstream_model: &str) -> AppResult<Value> {
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::BadRequest("messages is required".into()))?;

    let mut converted: Vec<Value> = Vec::new();
    if let Some(system) = flatten_optional(body.get("system")) {
        converted.push(json!({"role": "system", "content": system}));
    }

    for msg in messages {
        let role = msg.get("role").and_then(Value::as_str).unwrap_or("user");
        let (text, reasoning, tool_calls, tool_results) = split_anthropic_content(msg.get("content"));
        match role {
            "assistant" => {
                let mut out = json!({"role": "assistant", "content": text});
                if let Some(obj) = out.as_object_mut() {
                    if let Some(r) = reasoning {
                        obj.insert("reasoning_content".into(), Value::String(r));
                    }
                    if !tool_calls.is_empty() {
                        obj.insert("tool_calls".into(), Value::Array(tool_calls));
                    }
                }
                converted.push(out);
            }
            _ => {
                if tool_results.is_empty() {
                    converted.push(json!({"role": "user", "content": text}));
                } else {
                    for (tool_call_id, content) in tool_results {
                        converted.push(json!({
                            "role": "tool",
                            "tool_call_id": tool_call_id,
                            "content": content
                        }));
                    }
                    if !text.is_empty() {
                        converted.push(json!({"role": "user", "content": text}));
                    }
                }
            }
        }
    }

    if converted.is_empty() {
        return Err(AppError::BadRequest("messages is required".into()));
    }

    let mut out = json!({
        "model": upstream_model,
        "messages": converted,
    });

    if let Some(obj) = out.as_object_mut() {
        copy_if_present(body, obj, "stream");
        copy_if_present(body, obj, "temperature");
        copy_if_present(body, obj, "top_p");
        copy_if_present(body, obj, "stream_options");
        if let Some(max_tokens) = body.get("max_tokens") {
            obj.insert("max_tokens".into(), max_tokens.clone());
        }
        if let Some(stops) = body.get("stop_sequences") {
            obj.insert("stop".into(), stops.clone());
        }
        if let Some(tools) = anthropic_tools_to_openai(body.get("tools")) {
            obj.insert("tools".into(), tools);
        }
        // GLM-4.5 thinking + sampling knobs if a GLM client used Anthropic-shaped extras.
        copy_if_present(body, obj, "do_sample");
        copy_if_present(body, obj, "request_id");
        copy_if_present(body, obj, "thinking");
    }

    Ok(out)
}

pub fn openai_response_to_anthropic(body: &Value) -> Value {
    let text = body
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let reasoning = body
        .pointer("/choices/0/message/reasoning_content")
        .and_then(Value::as_str)
        .or_else(|| {
            body.pointer("/choices/0/message/reasoning")
                .and_then(Value::as_str)
        });
    let finish = body
        .pointer("/choices/0/finish_reason")
        .and_then(Value::as_str)
        .unwrap_or("stop");
    let stop_reason = match finish {
        "length" => "max_tokens",
        "tool_calls" | "function_call" => "tool_use",
        _ => "end_turn",
    };
    let id = body
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("msg_closedrouter");
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("closedrouter");
    let usage = body.get("usage").cloned().unwrap_or(json!({}));

    let mut content = Vec::new();
    if let Some(reason) = reasoning {
        if !reason.is_empty() {
            content.push(json!({"type": "thinking", "thinking": reason}));
        }
    }
    if let Some(calls) = body.pointer("/choices/0/message/tool_calls") {
        if let Some(arr) = calls.as_array() {
            for call in arr {
                let id = call
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("toolu_closedrouter");
                let name = call
                    .pointer("/function/name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                let input_raw = call
                    .pointer("/function/arguments")
                    .and_then(Value::as_str)
                    .unwrap_or("{}");
                let input = serde_json::from_str::<Value>(input_raw)
                    .unwrap_or_else(|_| json!({ "raw": input_raw }));
                content.push(json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": input
                }));
            }
        }
    }
    content.push(json!({"type": "text", "text": text}));

    json!({
        "id": id,
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": content,
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": {
            "input_tokens": usage.get("prompt_tokens").cloned().unwrap_or(json!(0)),
            "output_tokens": usage.get("completion_tokens").cloned().unwrap_or(json!(0)),
        }
    })
}

pub fn anthropic_response_to_openai(body: &Value) -> Value {
    let (text, reasoning, tool_calls, _) = split_anthropic_content(body.get("content"));
    let id = body
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("chatcmpl-closedrouter");
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("closedrouter");
    let stop = body
        .get("stop_reason")
        .and_then(Value::as_str)
        .unwrap_or("end_turn");
    let finish = match stop {
        "max_tokens" => "length",
        "tool_use" => "tool_calls",
        _ => "stop",
    };
    let usage = body.get("usage").cloned().unwrap_or(json!({}));
    let prompt = usage.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
    let completion = usage
        .get("output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    let mut message = json!({
        "role": "assistant",
        "content": text
    });
    if let Some(obj) = message.as_object_mut() {
        if let Some(r) = reasoning {
            obj.insert("reasoning_content".into(), Value::String(r));
        }
        if !tool_calls.is_empty() {
            obj.insert("tool_calls".into(), Value::Array(tool_calls));
        }
    }

    json!({
        "id": id,
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": finish
        }],
        "usage": {
            "prompt_tokens": prompt,
            "completion_tokens": completion,
            "total_tokens": prompt + completion
        }
    })
}

pub fn flatten_content(content: Option<&Value>) -> String {
    match content {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| {
                if let Some(s) = part.as_str() {
                    return Some(s.to_string());
                }
                match part.get("type").and_then(Value::as_str) {
                    Some("text") => part.get("text").and_then(Value::as_str).map(str::to_string),
                    Some("thinking") => None,
                    _ => part.get("text").and_then(Value::as_str).map(str::to_string),
                }
            })
            .collect::<Vec<_>>()
            .join(""),
        Some(other) => other.to_string(),
    }
}

fn flatten_optional(value: Option<&Value>) -> Option<String> {
    let text = flatten_content(value);
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn copy_if_present(src: &Value, dest: &mut Map<String, Value>, key: &str) {
    if let Some(v) = src.get(key) {
        dest.insert(key.to_string(), v.clone());
    }
}

fn openai_tools_to_anthropic(tools: Option<&Value>) -> Option<Value> {
    let arr = tools.and_then(Value::as_array)?;
    let converted: Vec<Value> = arr
        .iter()
        .filter_map(|tool| {
            let name = tool
                .pointer("/function/name")
                .or_else(|| tool.get("name"))
                .and_then(Value::as_str)?;
            let description = tool
                .pointer("/function/description")
                .or_else(|| tool.get("description"))
                .cloned()
                .unwrap_or(Value::String(String::new()));
            let schema = tool
                .pointer("/function/parameters")
                .or_else(|| tool.get("input_schema"))
                .cloned()
                .unwrap_or(json!({"type": "object", "properties": {}}));
            Some(json!({
                "name": name,
                "description": description,
                "input_schema": schema
            }))
        })
        .collect();
    if converted.is_empty() {
        None
    } else {
        Some(Value::Array(converted))
    }
}

fn anthropic_tools_to_openai(tools: Option<&Value>) -> Option<Value> {
    let arr = tools.and_then(Value::as_array)?;
    let converted: Vec<Value> = arr
        .iter()
        .filter_map(|tool| {
            let name = tool.get("name").and_then(Value::as_str)?;
            Some(json!({
                "type": "function",
                "function": {
                    "name": name,
                    "description": tool.get("description").cloned().unwrap_or(json!("")),
                    "parameters": tool.get("input_schema").cloned().unwrap_or(json!({"type":"object"}))
                }
            }))
        })
        .collect();
    if converted.is_empty() {
        None
    } else {
        Some(Value::Array(converted))
    }
}

type AnthropicSplit = (String, Option<String>, Vec<Value>, Vec<(String, String)>);

fn split_anthropic_content(content: Option<&Value>) -> AnthropicSplit {
    let mut text = String::new();
    let mut reasoning: Option<String> = None;
    let mut tool_calls = Vec::new();
    let mut tool_results = Vec::new();
    match content {
        Some(Value::String(s)) => text = s.clone(),
        Some(Value::Array(parts)) => {
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(t) = part.get("text").and_then(Value::as_str) {
                            text.push_str(t);
                        }
                    }
                    Some("thinking") => {
                        let t = part
                            .get("thinking")
                            .and_then(Value::as_str)
                            .or_else(|| part.get("text").and_then(Value::as_str))
                            .unwrap_or("");
                        reasoning = Some(match reasoning.take() {
                            Some(existing) => format!("{existing}{t}"),
                            None => t.to_string(),
                        });
                    }
                    Some("tool_use") => {
                        let id = part
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("toolu_closedrouter");
                        let name = part.get("name").and_then(Value::as_str).unwrap_or("tool");
                        let input = part.get("input").cloned().unwrap_or(json!({}));
                        let args = serde_json::to_string(&input).unwrap_or_else(|_| "{}".into());
                        tool_calls.push(json!({
                            "id": id,
                            "type": "function",
                            "function": {"name": name, "arguments": args}
                        }));
                    }
                    Some("tool_result") => {
                        let id = part
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .unwrap_or("toolu_closedrouter")
                            .to_string();
                        let body = flatten_content(part.get("content"));
                        tool_results.push((id, body));
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    (text, reasoning, tool_calls, tool_results)
}

/// Convert a parsed Anthropic SSE event into OpenAI chat.completion.chunk SSE lines.
pub fn anthropic_event_to_openai_sse(
    event: &str,
    data: &Value,
    id: &str,
    model: &str,
) -> Vec<String> {
    let mut lines = Vec::new();
    match event {
        "message_start" => {
            let chunk = json!({
                "id": id,
                "object": "chat.completion.chunk",
                "created": chrono::Utc::now().timestamp(),
                "model": model,
                "choices": [{"index": 0, "delta": {"role": "assistant"}, "finish_reason": null}]
            });
            lines.push(format!("data: {chunk}"));
        }
        "content_block_start" => {
            let block = data.get("content_block");
            if block.and_then(|b| b.get("type")).and_then(Value::as_str) == Some("tool_use") {
                let index = data.get("index").and_then(Value::as_u64).unwrap_or(0);
                let tool_id = block
                    .and_then(|b| b.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or("toolu_closedrouter");
                let name = block
                    .and_then(|b| b.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                let chunk = json!({
                    "id": id,
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": model,
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "tool_calls": [{
                                "index": index,
                                "id": tool_id,
                                "type": "function",
                                "function": {"name": name, "arguments": ""}
                            }]
                        },
                        "finish_reason": null
                    }]
                });
                lines.push(format!("data: {chunk}"));
            }
        }
        "content_block_delta" => {
            if let Some(text) = data.pointer("/delta/text").and_then(Value::as_str) {
                let chunk = json!({
                    "id": id,
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": model,
                    "choices": [{"index": 0, "delta": {"content": text}, "finish_reason": null}]
                });
                lines.push(format!("data: {chunk}"));
            }
            if let Some(thinking) = data
                .pointer("/delta/thinking")
                .and_then(Value::as_str)
                .or_else(|| data.pointer("/delta/text").and_then(Value::as_str).filter(|_| {
                    data.pointer("/delta/type").and_then(Value::as_str) == Some("thinking_delta")
                }))
            {
                if data.pointer("/delta/type").and_then(Value::as_str) == Some("thinking_delta")
                    || data.get("delta").and_then(|d| d.get("thinking")).is_some()
                {
                    let chunk = json!({
                        "id": id,
                        "object": "chat.completion.chunk",
                        "created": chrono::Utc::now().timestamp(),
                        "model": model,
                        "choices": [{"index": 0, "delta": {"reasoning_content": thinking}, "finish_reason": null}]
                    });
                    lines.push(format!("data: {chunk}"));
                }
            }
            if let Some(args) = data
                .pointer("/delta/partial_json")
                .and_then(Value::as_str)
            {
                let index = data.get("index").and_then(Value::as_u64).unwrap_or(0);
                let chunk = json!({
                    "id": id,
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": model,
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "tool_calls": [{
                                "index": index,
                                "function": {"arguments": args}
                            }]
                        },
                        "finish_reason": null
                    }]
                });
                lines.push(format!("data: {chunk}"));
            }
        }
        "message_delta" => {
            if let Some(stop) = data.pointer("/delta/stop_reason").and_then(Value::as_str) {
                let finish = match stop {
                    "max_tokens" => "length",
                    "tool_use" => "tool_calls",
                    _ => "stop",
                };
                let mut chunk = json!({
                    "id": id,
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": model,
                    "choices": [{"index": 0, "delta": {}, "finish_reason": finish}]
                });
                if let Some(usage) = data.get("usage") {
                    if let Some(obj) = chunk.as_object_mut() {
                        obj.insert(
                            "usage".into(),
                            json!({
                                "prompt_tokens": usage.get("input_tokens").cloned().unwrap_or(json!(0)),
                                "completion_tokens": usage.get("output_tokens").cloned().unwrap_or(json!(0)),
                                "total_tokens": json!(
                                    usage.get("input_tokens").and_then(Value::as_u64).unwrap_or(0)
                                    + usage.get("output_tokens").and_then(Value::as_u64).unwrap_or(0)
                                )
                            }),
                        );
                    }
                }
                lines.push(format!("data: {chunk}"));
            }
        }
        "message_stop" => {
            lines.push("data: [DONE]".into());
        }
        _ => {}
    }
    lines
}

/// Streaming translator state for OpenAI-family SSE → Anthropic SSE.
#[derive(Debug, Default)]
pub struct OpenaiToAnthropicSse {
    started: bool,
    next_block: u64,
    text_block: Option<u64>,
    /// OpenAI `tool_calls[].index` → Anthropic content-block index.
    tools: HashMap<u64, u64>,
    open_blocks: Vec<u64>,
}

impl OpenaiToAnthropicSse {
    fn alloc_block(&mut self) -> u64 {
        let index = self.next_block;
        self.next_block += 1;
        self.open_blocks.push(index);
        index
    }
}

/// Map an OpenAI-family `finish_reason` to Anthropic `stop_reason`.
/// Unknown values (including `content_filter`) default to `end_turn`.
fn openai_finish_reason_to_anthropic(reason: &str) -> &'static str {
    match reason {
        "stop" => "end_turn",
        "length" => "max_tokens",
        "tool_calls" => "tool_use",
        _ => "end_turn",
    }
}

/// Convert a parsed OpenAI SSE data payload into Anthropic SSE events.
pub fn openai_data_to_anthropic_sse(data: &str, state: &mut OpenaiToAnthropicSse) -> Vec<String> {
    if data.trim() == "[DONE]" {
        return vec![
            "event: message_stop".into(),
            "data: {\"type\":\"message_stop\"}".into(),
        ];
    }
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if !state.started {
        state.started = true;
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("msg_closedrouter");
        let model = value
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("closedrouter");
        let start = json!({
            "type": "message_start",
            "message": {
                "id": id,
                "type": "message",
                "role": "assistant",
                "model": model,
                "content": [],
                "stop_reason": null,
                "stop_sequence": null,
                "usage": {"input_tokens": 0, "output_tokens": 0}
            }
        });
        out.push("event: message_start".into());
        out.push(format!("data: {start}"));
    }
    if let Some(text) = value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
    {
        if !text.is_empty() {
            let index = match state.text_block {
                Some(index) => index,
                None => {
                    let index = state.alloc_block();
                    state.text_block = Some(index);
                    let start = json!({
                        "type": "content_block_start",
                        "index": index,
                        "content_block": {"type": "text", "text": ""}
                    });
                    out.push("event: content_block_start".into());
                    out.push(format!("data: {start}"));
                    index
                }
            };
            let delta = json!({
                "type": "content_block_delta",
                "index": index,
                "delta": {"type": "text_delta", "text": text}
            });
            out.push("event: content_block_delta".into());
            out.push(format!("data: {delta}"));
        }
    }
    if let Some(reason) = value
        .pointer("/choices/0/delta/reasoning_content")
        .and_then(Value::as_str)
    {
        if !reason.is_empty() {
            let delta = json!({
                "type": "content_block_delta",
                "index": state.text_block.unwrap_or(0),
                "delta": {"type": "thinking_delta", "thinking": reason}
            });
            out.push("event: content_block_delta".into());
            out.push(format!("data: {delta}"));
        }
    }
    if let Some(calls) = value
        .pointer("/choices/0/delta/tool_calls")
        .and_then(Value::as_array)
    {
        for call in calls {
            let openai_index = call.get("index").and_then(Value::as_u64).unwrap_or(0);
            let anthropic_index = match state.tools.get(&openai_index).copied() {
                Some(index) => index,
                None => {
                    let index = state.alloc_block();
                    state.tools.insert(openai_index, index);
                    let id = call
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("toolu_closedrouter");
                    let name = call
                        .pointer("/function/name")
                        .and_then(Value::as_str)
                        .unwrap_or("tool");
                    let start = json!({
                        "type": "content_block_start",
                        "index": index,
                        "content_block": {
                            "type": "tool_use",
                            "id": id,
                            "name": name,
                            "input": {}
                        }
                    });
                    out.push("event: content_block_start".into());
                    out.push(format!("data: {start}"));
                    index
                }
            };
            if let Some(args) = call.pointer("/function/arguments").and_then(Value::as_str) {
                if !args.is_empty() {
                    let delta = json!({
                        "type": "content_block_delta",
                        "index": anthropic_index,
                        "delta": {"type": "input_json_delta", "partial_json": args}
                    });
                    out.push("event: content_block_delta".into());
                    out.push(format!("data: {delta}"));
                }
            }
        }
    }
    if let Some(reason) = value
        .pointer("/choices/0/finish_reason")
        .and_then(Value::as_str)
    {
        let stop_reason = openai_finish_reason_to_anthropic(reason);
        for index in std::mem::take(&mut state.open_blocks) {
            let stop = json!({"type": "content_block_stop", "index": index});
            out.push("event: content_block_stop".into());
            out.push(format!("data: {stop}"));
        }
        let delta = json!({
            "type": "message_delta",
            "delta": {"stop_reason": stop_reason, "stop_sequence": null}
        });
        out.push("event: message_delta".into());
        out.push(format!("data: {delta}"));
    }
    out
}

pub fn openai_chunk_id(model: &str) -> String {
    format!("chatcmpl-{}", model.replace(['/', '.'], "-"))
}

/// Pull token usage out of an OpenAI- or Anthropic-shaped JSON body.
pub fn usage_from_body(body: &Value) -> (Option<i32>, Option<i32>) {
    if let Some(usage) = body.get("usage") {
        let prompt = usage
            .get("prompt_tokens")
            .or_else(|| usage.get("input_tokens"))
            .and_then(Value::as_i64)
            .map(|n| n as i32);
        let completion = usage
            .get("completion_tokens")
            .or_else(|| usage.get("output_tokens"))
            .and_then(Value::as_i64)
            .map(|n| n as i32);
        return (prompt, completion);
    }
    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_system_extracted() {
        let body = json!({
            "model": "llama3",
            "messages": [
                {"role": "system", "content": "be brief"},
                {"role": "user", "content": "hi"}
            ],
            "max_tokens": 32
        });
        let out = openai_to_anthropic(&body, "claude-3").unwrap();
        assert_eq!(out["model"], "claude-3");
        assert_eq!(out["system"], "be brief");
        assert_eq!(out["messages"][0]["role"], "user");
        assert_eq!(out["max_tokens"], 32);
    }

    #[test]
    fn openai_content_blocks_flattened() {
        let body = json!({
            "model": "x",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "hello "},
                    {"type": "text", "text": "world"}
                ]
            }]
        });
        let out = openai_to_anthropic(&body, "claude").unwrap();
        assert_eq!(out["messages"][0]["content"], "hello world");
    }

    #[test]
    fn anthropic_system_becomes_openai_message() {
        let body = json!({
            "model": "claude",
            "system": "rules",
            "messages": [{"role": "user", "content": "go"}],
            "max_tokens": 10
        });
        let out = anthropic_to_openai(&body, "gpt-4").unwrap();
        assert_eq!(out["model"], "gpt-4");
        assert_eq!(out["messages"][0]["role"], "system");
        assert_eq!(out["messages"][1]["content"], "go");
        assert_eq!(out["max_tokens"], 10);
    }

    #[test]
    fn response_round_trip_text() {
        let openai = json!({
            "id": "chatcmpl-1",
            "model": "gpt",
            "choices": [{"message": {"role": "assistant", "content": "pong"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 2, "completion_tokens": 1}
        });
        let anthropic = openai_response_to_anthropic(&openai);
        assert_eq!(anthropic["content"].as_array().unwrap().last().unwrap()["text"], "pong");
        let back = anthropic_response_to_openai(&anthropic);
        assert_eq!(back["choices"][0]["message"]["content"], "pong");
    }

    #[test]
    fn deepseek_reasoning_to_anthropic_and_back() {
        let openai = json!({
            "id": "chatcmpl-ds",
            "model": "deepseek-reasoner",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "42",
                    "reasoning_content": "think hard"
                },
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 4, "completion_tokens": 2}
        });
        let anthropic = openai_response_to_anthropic(&openai);
        assert_eq!(anthropic["content"][0]["type"], "thinking");
        assert_eq!(anthropic["content"][0]["thinking"], "think hard");
        let back = anthropic_response_to_openai(&anthropic);
        assert_eq!(back["choices"][0]["message"]["reasoning_content"], "think hard");
        assert_eq!(back["choices"][0]["message"]["content"], "42");
    }

    #[test]
    fn glm_thinking_and_request_id_preserved_on_rewrite() {
        let body = json!({
            "model": "glm-4.5",
            "messages": [{"role": "user", "content": "hi"}],
            "thinking": {"type": "enabled"},
            "do_sample": true,
            "request_id": "req-1"
        });
        let out = rewrite_model(body, "glm-4-plus");
        assert_eq!(out["model"], "glm-4-plus");
        assert_eq!(out["thinking"]["type"], "enabled");
        assert_eq!(out["do_sample"], true);
        assert_eq!(out["request_id"], "req-1");
    }

    #[test]
    fn openai_tools_round_trip_anthropic() {
        let body = json!({
            "model": "x",
            "messages": [{"role": "user", "content": "weather?"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "weather",
                    "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}
                }
            }]
        });
        let anthropic = openai_to_anthropic(&body, "claude").unwrap();
        assert_eq!(anthropic["tools"][0]["name"], "get_weather");
        let back = anthropic_to_openai(&anthropic, "gpt").unwrap();
        assert_eq!(back["tools"][0]["function"]["name"], "get_weather");
    }

    #[test]
    fn anthropic_sse_text_delta() {
        let data = json!({
            "type": "content_block_delta",
            "delta": {"type": "text_delta", "text": "Hi"}
        });
        let lines = anthropic_event_to_openai_sse("content_block_delta", &data, "id", "m");
        assert!(lines[0].contains("Hi"));
        assert!(lines[0].starts_with("data: "));
    }

    #[test]
    fn openai_sse_done() {
        let lines = openai_data_to_anthropic_sse("[DONE]", &mut OpenaiToAnthropicSse::default());
        assert_eq!(lines[0], "event: message_stop");
    }

    #[test]
    fn openai_tool_call_chunks_become_anthropic_tool_use() {
        let mut state = OpenaiToAnthropicSse::default();
        let start = json!({
            "id": "chatcmpl-1",
            "model": "gpt",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "get_weather", "arguments": ""}
                    }]
                }
            }]
        })
        .to_string();
        let lines = openai_data_to_anthropic_sse(&start, &mut state);
        let joined = lines.join("\n");
        assert!(joined.contains("\"type\":\"tool_use\""));
        assert!(joined.contains("call_1"));
        assert!(joined.contains("get_weather"));
        assert!(
            !joined.contains("\"type\":\"text\""),
            "tool-only streams must not open a text block"
        );

        let args = json!({
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": {"arguments": "{\"city\":\"sf\"}"}
                    }]
                }
            }]
        })
        .to_string();
        let lines = openai_data_to_anthropic_sse(&args, &mut state);
        let joined = lines.join("\n");
        assert!(joined.contains("input_json_delta"));
        assert!(joined.contains("city"));
    }

    #[test]
    fn openai_finish_reason_maps_to_anthropic_stop_reason() {
        for (openai, anthropic) in [
            ("stop", "end_turn"),
            ("length", "max_tokens"),
            ("tool_calls", "tool_use"),
            ("content_filter", "end_turn"),
        ] {
            let mut state = OpenaiToAnthropicSse::default();
            let start = json!({
                "id": "c",
                "model": "m",
                "choices": [{"delta": {"content": "x"}}]
            })
            .to_string();
            openai_data_to_anthropic_sse(&start, &mut state);
            let finish = json!({
                "choices": [{"delta": {}, "finish_reason": openai}]
            })
            .to_string();
            let lines = openai_data_to_anthropic_sse(&finish, &mut state);
            let stop = lines.iter().find_map(|line| {
                let data = line.strip_prefix("data: ")?;
                let value: Value = serde_json::from_str(data).ok()?;
                if value.get("type").and_then(Value::as_str) == Some("message_delta") {
                    value
                        .pointer("/delta/stop_reason")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                } else {
                    None
                }
            });
            assert_eq!(stop.as_deref(), Some(anthropic), "openai finish {openai}");
        }
    }

    #[test]
    fn openai_stop_maps_to_stop_sequences_only() {
        let body = json!({
            "model": "x",
            "messages": [{"role": "user", "content": "hi"}],
            "stop": ["END", "STOP"]
        });
        let out = openai_to_anthropic(&body, "claude").unwrap();
        assert_eq!(out["stop_sequences"], json!(["END", "STOP"]));
        assert!(out.get("stop").is_none());

        let body = json!({
            "model": "x",
            "messages": [{"role": "user", "content": "hi"}],
            "stop": "END"
        });
        let out = openai_to_anthropic(&body, "claude").unwrap();
        assert_eq!(out["stop_sequences"], json!(["END"]));
        assert!(out.get("stop").is_none());
    }

    #[test]
    fn anthropic_tool_use_start_emits_openai_tool_metadata() {
        let data = json!({
            "type": "content_block_start",
            "index": 1,
            "content_block": {
                "type": "tool_use",
                "id": "toolu_abc",
                "name": "get_weather",
                "input": {}
            }
        });
        let lines = anthropic_event_to_openai_sse("content_block_start", &data, "chatcmpl-1", "m");
        assert_eq!(lines.len(), 1);
        let payload: Value =
            serde_json::from_str(lines[0].strip_prefix("data: ").expect("data prefix")).unwrap();
        let call = &payload["choices"][0]["delta"]["tool_calls"][0];
        assert_eq!(call["id"], "toolu_abc");
        assert_eq!(call["type"], "function");
        assert_eq!(call["index"], 1);
        assert_eq!(call["function"]["name"], "get_weather");
        assert_eq!(call["function"]["arguments"], "");
    }

    #[test]
    fn anthropic_text_block_start_is_silent() {
        let data = json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "text", "text": ""}
        });
        let lines = anthropic_event_to_openai_sse("content_block_start", &data, "id", "m");
        assert!(lines.is_empty());
    }

    #[test]
    fn anthropic_tool_arg_delta_keeps_block_index() {
        let data = json!({
            "type": "content_block_delta",
            "index": 1,
            "delta": {"type": "input_json_delta", "partial_json": "{\"city\":"}
        });
        let lines = anthropic_event_to_openai_sse("content_block_delta", &data, "id", "m");
        let payload: Value =
            serde_json::from_str(lines[0].strip_prefix("data: ").expect("data prefix")).unwrap();
        let call = &payload["choices"][0]["delta"]["tool_calls"][0];
        assert_eq!(call["index"], 1);
        assert_eq!(call["function"]["arguments"], "{\"city\":");
    }

    #[test]
    fn rewrite_model_keeps_extra_fields() {
        let body = json!({"model": "friendly", "temperature": 0.2, "foo": 1});
        let out = rewrite_model(body, "upstream-model");
        assert_eq!(out["model"], "upstream-model");
        assert_eq!(out["foo"], 1);
    }

    #[test]
    fn protocol_family() {
        assert!(Protocol::DeepSeek.is_openai_family());
        assert!(Protocol::Glm.is_openai_family());
        assert!(!Protocol::Anthropic.is_openai_family());
        assert_eq!(Protocol::from_kind("zhipu").unwrap(), Protocol::Glm);
    }

    #[test]
    fn usage_parser() {
        let (p, c) = usage_from_body(&json!({"usage": {"prompt_tokens": 3, "completion_tokens": 9}}));
        assert_eq!(p, Some(3));
        assert_eq!(c, Some(9));
        let (p, c) = usage_from_body(&json!({"usage": {"input_tokens": 1, "output_tokens": 2}}));
        assert_eq!(p, Some(1));
        assert_eq!(c, Some(2));
    }
}
