use crate::error::{AppError, AppResult};
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    OpenAi,
    Anthropic,
}

impl Protocol {
    pub fn from_kind(kind: &str) -> AppResult<Self> {
        match kind {
            "openai" => Ok(Protocol::OpenAi),
            "anthropic" => Ok(Protocol::Anthropic),
            _ => Err(AppError::BadRequest(format!(
                "unsupported provider kind '{kind}'"
            ))),
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

pub fn rewrite_model(mut body: Value, upstream_model: &str) -> Value {
    if let Some(obj) = body.as_object_mut() {
        obj.insert("model".into(), Value::String(upstream_model.to_string()));
    }
    body
}

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
        match role {
            "system" => {
                if !content.is_empty() {
                    system_parts.push(content);
                }
            }
            "user" | "assistant" => {
                converted.push(json!({
                    "role": role,
                    "content": content,
                }));
            }
            "tool" => {
                converted.push(json!({
                    "role": "user",
                    "content": format!("[tool] {content}"),
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
        copy_if_present(body, obj, "stop");
        if let Some(stop) = body.get("stop") {
            if stop.is_string() {
                obj.insert("stop_sequences".into(), json!([stop]));
            } else if stop.is_array() {
                obj.insert("stop_sequences".into(), stop.clone());
            }
        }
    }

    Ok(out)
}

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
        let content = flatten_content(msg.get("content"));
        let mapped = match role {
            "assistant" => "assistant",
            _ => "user",
        };
        converted.push(json!({"role": mapped, "content": content}));
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
        if let Some(max_tokens) = body.get("max_tokens") {
            obj.insert("max_tokens".into(), max_tokens.clone());
        }
        if let Some(stops) = body.get("stop_sequences") {
            obj.insert("stop".into(), stops.clone());
        }
    }

    Ok(out)
}

pub fn openai_response_to_anthropic(body: &Value) -> Value {
    let text = body
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
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

    json!({
        "id": id,
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": [{"type": "text", "text": text}],
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": {
            "input_tokens": usage.get("prompt_tokens").cloned().unwrap_or(json!(0)),
            "output_tokens": usage.get("completion_tokens").cloned().unwrap_or(json!(0)),
        }
    })
}

pub fn anthropic_response_to_openai(body: &Value) -> Value {
    let text = anthropic_text(body);
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
    json!({
        "id": id,
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": text},
            "finish_reason": finish
        }],
        "usage": {
            "prompt_tokens": usage.get("input_tokens").cloned().unwrap_or(json!(0)),
            "completion_tokens": usage.get("output_tokens").cloned().unwrap_or(json!(0)),
            "total_tokens": json!(
                usage.get("input_tokens").and_then(Value::as_u64).unwrap_or(0)
                + usage.get("output_tokens").and_then(Value::as_u64).unwrap_or(0)
            )
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
                if part.get("type").and_then(Value::as_str) == Some("text") {
                    return part.get("text").and_then(Value::as_str).map(str::to_string);
                }
                part.get("text").and_then(Value::as_str).map(str::to_string)
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

fn anthropic_text(body: &Value) -> String {
    body.get("content")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
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
        }
        "message_delta" => {
            if let Some(stop) = data.pointer("/delta/stop_reason").and_then(Value::as_str) {
                let finish = match stop {
                    "max_tokens" => "length",
                    "tool_use" => "tool_calls",
                    _ => "stop",
                };
                let chunk = json!({
                    "id": id,
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": model,
                    "choices": [{"index": 0, "delta": {}, "finish_reason": finish}]
                });
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

/// Convert a parsed OpenAI SSE data payload into Anthropic SSE events.
pub fn openai_data_to_anthropic_sse(data: &str, started: &mut bool) -> Vec<String> {
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
    if !*started {
        *started = true;
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
        out.push("event: content_block_start".into());
        out.push("data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}".into());
    }
    if let Some(text) = value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
    {
        if !text.is_empty() {
            let delta = json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": text}
            });
            out.push("event: content_block_delta".into());
            out.push(format!("data: {delta}"));
        }
    }
    if value
        .pointer("/choices/0/finish_reason")
        .and_then(Value::as_str)
        .is_some()
    {
        out.push("event: content_block_stop".into());
        out.push("data: {\"type\":\"content_block_stop\",\"index\":0}".into());
        out.push("event: message_delta".into());
        out.push("data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null}}".into());
    }
    out
}

pub fn openai_chunk_id(model: &str) -> String {
    format!("chatcmpl-{}", model.replace(['/', '.'], "-"))
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
        assert_eq!(anthropic["content"][0]["text"], "pong");
        let back = anthropic_response_to_openai(&anthropic);
        assert_eq!(back["choices"][0]["message"]["content"], "pong");
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
        let lines = openai_data_to_anthropic_sse("[DONE]", &mut true);
        assert_eq!(lines[0], "event: message_stop");
    }

    #[test]
    fn rewrite_model_keeps_extra_fields() {
        let body = json!({"model": "friendly", "temperature": 0.2, "foo": 1});
        let out = rewrite_model(body, "upstream-model");
        assert_eq!(out["model"], "upstream-model");
        assert_eq!(out["foo"], 1);
    }
}
