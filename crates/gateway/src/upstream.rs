//! Upstream HTTP proxy, including protocol translation and embeddings.

use crate::db::ProviderSecret;
use crate::error::{AppError, AppResult};
use crate::observability;
use crate::translate::{
    anthropic_event_to_openai_sse, anthropic_response_to_openai, anthropic_to_openai,
    extract_model, extract_stream, openai_chunk_id, openai_data_to_anthropic_sse,
    openai_response_to_anthropic, openai_to_anthropic, rewrite_model, usage_from_body,
    OpenaiToAnthropicSse, Protocol,
};
use anyhow::Context;
use axum::body::Body;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// Outcome of a proxied call, used for logs / Langfuse / Prometheus.
#[derive(Debug, Clone, Default)]
pub struct ProxyMeta {
    pub status: u16,
    pub latency_ms: u64,
    pub prompt_tokens: Option<i32>,
    pub completion_tokens: Option<i32>,
    pub error: Option<String>,
}

pub fn http_client() -> anyhow::Result<Client> {
    Client::builder()
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .context("build HTTP client")
}

fn join_url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn chat_url(provider: &ProviderSecret, protocol: Protocol) -> String {
    join_url(&provider.base_url, protocol.chat_path())
}

fn embeddings_url(provider: &ProviderSecret) -> String {
    join_url(&provider.base_url, "embeddings")
}

fn apply_upstream_auth(
    builder: reqwest::RequestBuilder,
    provider: &ProviderSecret,
) -> reqwest::RequestBuilder {
    match provider.kind.as_str() {
        "anthropic" => {
            let mut b = builder.header("anthropic-version", "2023-06-01");
            if let Some(key) = provider.api_key.as_deref().filter(|k| !k.is_empty()) {
                b = b.header("x-api-key", key);
            }
            b
        }
        "glm" => {
            if let Some(key) = provider.api_key.as_deref().filter(|k| !k.is_empty()) {
                builder.bearer_auth(key)
            } else {
                builder
            }
        }
        _ => {
            if let Some(key) = provider.api_key.as_deref().filter(|k| !k.is_empty()) {
                builder.bearer_auth(key)
            } else {
                builder.bearer_auth("closedrouter")
            }
        }
    }
}

/// Proxy a chat completion / messages request, translating as needed.
pub async fn proxy(
    client: &Client,
    incoming: Protocol,
    provider: &ProviderSecret,
    upstream_model: &str,
    body: Value,
) -> AppResult<(Response, ProxyMeta)> {
    let started = Instant::now();
    let upstream_kind = Protocol::from_kind(&provider.kind)?;
    let stream = extract_stream(&body);
    let _incoming_model = extract_model(&body)?;

    let payload = if incoming.is_openai_family() && upstream_kind.is_openai_family()
        || incoming == Protocol::Anthropic && upstream_kind == Protocol::Anthropic
    {
        rewrite_model(body, upstream_model)
    } else if incoming.is_openai_family() && upstream_kind == Protocol::Anthropic {
        openai_to_anthropic(&body, upstream_model)?
    } else if incoming == Protocol::Anthropic && upstream_kind.is_openai_family() {
        anthropic_to_openai(&body, upstream_model)?
    } else {
        return Err(AppError::Internal(format!(
            "unhandled protocol pair {incoming:?} -> {upstream_kind:?}"
        )));
    };

    let url = chat_url(provider, upstream_kind);

    tracing::info!(
        incoming = incoming.as_str(),
        upstream = upstream_kind.as_str(),
        url = %url,
        stream,
        model = %upstream_model,
        "proxying completion"
    );

    let request = apply_upstream_auth(client.post(&url).json(&payload), provider);
    let response = request.send().await?;
    let status_code = response.status().as_u16();
    let status = StatusCode::from_u16(status_code).unwrap_or(StatusCode::BAD_GATEWAY);

    if stream {
        let res = pipe_stream(incoming, upstream_kind, upstream_model, response, status).await?;
        let latency_ms = started.elapsed().as_millis() as u64;
        observability::record_request(
            incoming.as_str(),
            status_code,
            latency_ms,
            None,
            None,
            !status.is_success(),
        );
        let error = if status.is_success() {
            None
        } else {
            Some("upstream stream error".into())
        };
        return Ok((
            res,
            ProxyMeta {
                status: status_code,
                latency_ms,
                prompt_tokens: None,
                completion_tokens: None,
                error,
            },
        ));
    }

    let bytes = response.bytes().await?;
    let latency_ms = started.elapsed().as_millis() as u64;
    if !status.is_success() {
        observability::record_request(
            incoming.as_str(),
            status_code,
            latency_ms,
            None,
            None,
            true,
        );
        let message = String::from_utf8_lossy(&bytes).chars().take(500).collect();
        return Ok((
            json_passthrough(status, bytes),
            ProxyMeta {
                status: status_code,
                latency_ms,
                prompt_tokens: None,
                completion_tokens: None,
                error: Some(message),
            },
        ));
    }

    let same_family = incoming.is_openai_family() == upstream_kind.is_openai_family()
        && (incoming.is_openai_family() || incoming == upstream_kind);

    let (out_bytes, parsed) = if same_family {
        let parsed = serde_json::from_slice::<Value>(&bytes).ok();
        (bytes, parsed)
    } else {
        let parsed: Value = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            json!({"error": {"message": String::from_utf8_lossy(&bytes)}})
        });
        let translated = if incoming.is_openai_family() && upstream_kind == Protocol::Anthropic {
            anthropic_response_to_openai(&parsed)
        } else {
            openai_response_to_anthropic(&parsed)
        };
        let encoded = serde_json::to_vec(&translated).unwrap_or_else(|_| b"{}".to_vec());
        (Bytes::from(encoded), Some(translated))
    };

    let (prompt, completion) = parsed.as_ref().map(usage_from_body).unwrap_or((None, None));
    observability::record_request(
        incoming.as_str(),
        status_code,
        latency_ms,
        prompt,
        completion,
        false,
    );

    Ok((
        json_passthrough(status, out_bytes),
        ProxyMeta {
            status: status_code,
            latency_ms,
            prompt_tokens: prompt,
            completion_tokens: completion,
            error: None,
        },
    ))
}

/// OpenAI-compatible embeddings against an OpenAI-family provider.
pub async fn embeddings(
    client: &Client,
    provider: &ProviderSecret,
    upstream_model: &str,
    mut body: Value,
) -> AppResult<Response> {
    if Protocol::from_kind(&provider.kind)?.is_openai_family() {
        // ok
    } else {
        return Err(AppError::BadRequest(
            "embeddings require an OpenAI-compatible (openai, deepseek, or glm) provider".into(),
        ));
    }
    body = rewrite_model(body, upstream_model);
    let url = embeddings_url(provider);
    let response = apply_upstream_auth(client.post(&url).json(&body), provider)
        .send()
        .await?;
    let status_code = response.status().as_u16();
    let status = StatusCode::from_u16(status_code).unwrap_or(StatusCode::BAD_GATEWAY);
    let bytes = response.bytes().await?;
    Ok(json_passthrough(status, bytes))
}

/// Fetch a single embedding vector (1536-d expected by Hermes).
pub async fn embed_text(
    client: &Client,
    provider: &ProviderSecret,
    upstream_model: &str,
    input: &str,
) -> AppResult<Vec<f32>> {
    let body = json!({
        "model": upstream_model,
        "input": input,
    });
    let url = embeddings_url(provider);
    let response = apply_upstream_auth(client.post(&url).json(&body), provider)
        .send()
        .await?;
    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(AppError::Upstream(format!(
            "embedding upstream failed: {text}"
        )));
    }
    let value: Value = response.json().await?;
    let arr = value
        .pointer("/data/0/embedding")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::Upstream("embedding response missing data[0].embedding".into()))?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let n = item.as_f64().ok_or_else(|| {
            AppError::Upstream("embedding vector contained a non-number".into())
        })?;
        out.push(n as f32);
    }
    Ok(out)
}

fn json_passthrough(status: StatusCode, bytes: Bytes) -> Response {
    match Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
    {
        Ok(res) => res,
        Err(err) => {
            tracing::error!(error = %err, "failed to build JSON response");
            let mut res = Response::new(Body::from(
                "{\"error\":{\"message\":\"internal error\",\"type\":\"internal_error\"}}",
            ));
            *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            res
        }
    }
}

async fn pipe_stream(
    incoming: Protocol,
    upstream: Protocol,
    model: &str,
    response: reqwest::Response,
    status: StatusCode,
) -> AppResult<Response> {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));
    headers.insert(
        header::HeaderName::from_static("x-accel-buffering"),
        HeaderValue::from_static("no"),
    );

    let same_family = incoming.is_openai_family() && upstream.is_openai_family()
        || incoming == Protocol::Anthropic && upstream == Protocol::Anthropic;

    if same_family {
        let byte_stream = response
            .bytes_stream()
            .map(|chunk| chunk.map_err(|err| std::io::Error::other(err.to_string())));
        let mut res = Response::new(Body::from_stream(byte_stream));
        *res.status_mut() = status;
        *res.headers_mut() = headers;
        return Ok(res);
    }

    let id = openai_chunk_id(model);
    let model_owned = model.to_string();
    let to_openai = incoming.is_openai_family() && upstream == Protocol::Anthropic;
    let byte_stream = response.bytes_stream();
    let converted = async_stream::stream! {
        let mut rest = Vec::new();
        let mut openai_sse = OpenaiToAnthropicSse::default();
        let mut current_event = String::new();
        futures_util::pin_mut!(byte_stream);
        while let Some(chunk) = byte_stream.next().await {
            match chunk {
                Ok(bytes) => {
                    let lines = push_sse_bytes(&mut rest, &bytes);
                    for line in lines {
                        let frames = translate_sse_line(
                            &line,
                            to_openai,
                            &mut current_event,
                            &mut openai_sse,
                            &id,
                            &model_owned,
                        );
                        for message in serialize_sse_frames(frames) {
                            yield Ok::<Bytes, std::io::Error>(Bytes::from(message));
                        }
                    }
                }
                Err(err) => {
                    yield Err(std::io::Error::other(err.to_string()));
                    break;
                }
            }
        }
        for line in flush_sse_bytes(&mut rest) {
            let frames = translate_sse_line(
                &line,
                to_openai,
                &mut current_event,
                &mut openai_sse,
                &id,
                &model_owned,
            );
            for message in serialize_sse_frames(frames) {
                yield Ok::<Bytes, std::io::Error>(Bytes::from(message));
            }
        }
    };

    let mut res = Response::new(Body::from_stream(converted));
    *res.status_mut() = status;
    *res.headers_mut() = headers;
    Ok(res)
}

fn convert_anthropic_line(
    line: &str,
    current_event: &mut String,
    id: &str,
    model: &str,
) -> Vec<String> {
    if let Some(event) = line.strip_prefix("event:") {
        *current_event = event.trim().to_string();
        return Vec::new();
    }
    if let Some(data) = line.strip_prefix("data:") {
        let data = data.trim();
        if data.is_empty() {
            return Vec::new();
        }
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            return Vec::new();
        };
        let event = if current_event.is_empty() {
            value.get("type").and_then(Value::as_str).unwrap_or("")
        } else {
            current_event.as_str()
        };
        return anthropic_event_to_openai_sse(event, &value, id, model);
    }
    Vec::new()
}

fn convert_openai_line(line: &str, state: &mut OpenaiToAnthropicSse) -> Vec<String> {
    let Some(data) = line.strip_prefix("data:") else {
        return Vec::new();
    };
    openai_data_to_anthropic_sse(data.trim(), state)
}

fn translate_sse_line(
    line: &str,
    to_openai: bool,
    current_event: &mut String,
    openai_sse: &mut OpenaiToAnthropicSse,
    id: &str,
    model: &str,
) -> Vec<String> {
    if line.is_empty() {
        return Vec::new();
    }
    if to_openai {
        convert_anthropic_line(line, current_event, id, model)
    } else {
        convert_openai_line(line, openai_sse)
    }
}

/// Append raw HTTP bytes and return SSE lines that are complete (newline-terminated)
/// and valid UTF-8. Incomplete trailing bytes stay in `buf` so a multibyte character
/// split across chunks is decoded only once the rest arrives.
fn push_sse_bytes(buf: &mut Vec<u8>, chunk: &[u8]) -> Vec<String> {
    buf.extend_from_slice(chunk);
    drain_complete_sse_lines(buf)
}

fn drain_complete_sse_lines(buf: &mut Vec<u8>) -> Vec<String> {
    let mut lines = Vec::new();
    loop {
        let Some(idx) = buf.iter().position(|&b| b == b'\n') else {
            break;
        };
        let mut remainder = buf.split_off(idx + 1);
        std::mem::swap(buf, &mut remainder);
        let mut line = remainder;
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        if line.is_empty() {
            continue;
        }
        if let Ok(s) = String::from_utf8(line) {
            lines.push(s);
        }
    }
    lines
}

/// Decode a trailing unterminated line at end-of-stream when it is valid UTF-8.
fn flush_sse_bytes(buf: &mut Vec<u8>) -> Vec<String> {
    let mut lines = drain_complete_sse_lines(buf);
    if buf.is_empty() {
        return lines;
    }
    if let Ok(s) = String::from_utf8(std::mem::take(buf)) {
        let s = s.trim_end_matches('\r');
        if !s.is_empty() {
            lines.push(s.to_string());
        }
    }
    lines
}

/// Group converter output into SSE messages.
///
/// Anthropic streams are paired `event:` + `data:` lines that must share a
/// single terminating blank line. OpenAI-family frames are already one `data:`
/// line each.
fn serialize_sse_frames(frames: Vec<String>) -> Vec<String> {
    let mut messages = Vec::new();
    let mut index = 0;
    while index < frames.len() {
        let frame = &frames[index];
        if frame.starts_with("event:")
            && index + 1 < frames.len()
            && frames[index + 1].starts_with("data:")
        {
            messages.push(format!("{frame}\n{}\n\n", frames[index + 1]));
            index += 2;
        } else {
            messages.push(format!("{frame}\n\n"));
            index += 1;
        }
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn provider(kind: &str, base: &str) -> ProviderSecret {
        ProviderSecret {
            id: "p".into(),
            name: "test".into(),
            kind: kind.into(),
            base_url: base.into(),
            api_key: Some("sk-test".into()),
        }
    }

    #[tokio::test]
    async fn openai_to_openai_rewrites_model() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "chatcmpl-1",
                "model": "llama3.2",
                "choices": [{"message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            })))
            .mount(&mock)
            .await;

        let client = http_client().expect("client");
        let (res, meta) = proxy(
            &client,
            Protocol::OpenAi,
            &provider("openai", &mock.uri()),
            "llama3.2",
            json!({"model": "llama3", "messages": [{"role":"user","content":"hi"}]}),
        )
        .await
        .expect("proxy");
        assert!(res.status().is_success());
        assert_eq!(meta.prompt_tokens, Some(1));
    }

    #[tokio::test]
    async fn deepseek_to_openai_passthrough() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "chatcmpl-ds",
                "choices": [{"message": {
                    "role": "assistant",
                    "content": "ok",
                    "reasoning_content": "n"
                }, "finish_reason": "stop"}]
            })))
            .mount(&mock)
            .await;
        let client = http_client().expect("client");
        let (res, _) = proxy(
            &client,
            Protocol::DeepSeek,
            &provider("openai", &mock.uri()),
            "deepseek-chat",
            json!({
                "model": "friendly",
                "messages": [{"role":"user","content":"hi"}],
                "reasoning_content": "keep-me"
            }),
        )
        .await
        .expect("proxy");
        assert!(res.status().is_success());
    }

    #[tokio::test]
    async fn glm_path_openai_compat() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"role": "assistant", "content": "glm"}, "finish_reason": "stop"}]
            })))
            .mount(&mock)
            .await;
        let client = http_client().expect("client");
        let (res, _) = proxy(
            &client,
            Protocol::Glm,
            &provider("glm", &mock.uri()),
            "glm-4",
            json!({
                "model": "glm-4",
                "messages": [{"role":"user","content":"hi"}],
                "thinking": {"type": "enabled"},
                "do_sample": false
            }),
        )
        .await
        .expect("proxy");
        assert!(res.status().is_success());
    }

    #[tokio::test]
    async fn openai_to_anthropic_translates() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg_1",
                "type": "message",
                "role": "assistant",
                "model": "claude",
                "content": [{"type": "text", "text": "pong"}],
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 2, "output_tokens": 1}
            })))
            .mount(&mock)
            .await;
        let client = http_client().expect("client");
        let (res, meta) = proxy(
            &client,
            Protocol::OpenAi,
            &provider("anthropic", &mock.uri()),
            "claude-3",
            json!({
                "model": "friendly",
                "messages": [{"role":"user","content":"ping"}]
            }),
        )
        .await
        .expect("proxy");
        assert!(res.status().is_success());
        assert_eq!(meta.completion_tokens, Some(1));
    }

    #[test]
    fn anthropic_event_data_pairs_share_one_blank_line() {
        let frames = vec![
            "event: message_stop".into(),
            "data: {\"type\":\"message_stop\"}".into(),
            "event: ping".into(),
            "data: {}".into(),
        ];
        let messages = serialize_sse_frames(frames);
        assert_eq!(messages.len(), 2);
        assert_eq!(
            messages[0],
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
        );
        assert_eq!(messages[1], "event: ping\ndata: {}\n\n");
    }

    #[test]
    fn openai_data_frames_stay_separate_messages() {
        let frames = vec!["data: {\"x\":1}".into(), "data: [DONE]".into()];
        let messages = serialize_sse_frames(frames);
        assert_eq!(
            messages,
            vec!["data: {\"x\":1}\n\n".to_string(), "data: [DONE]\n\n".to_string()]
        );
    }

    #[test]
    fn openai_to_anthropic_converter_pairs_are_one_sse_message() {
        let frames = openai_data_to_anthropic_sse("[DONE]", &mut OpenaiToAnthropicSse::default());
        let messages = serialize_sse_frames(frames);
        assert_eq!(messages.len(), 1);
        assert!(messages[0].starts_with("event: message_stop\n"));
        assert!(messages[0].contains("data: {\"type\":\"message_stop\"}"));
        assert!(messages[0].ends_with("\n\n"));
        assert_eq!(messages[0].matches("\n\n").count(), 1);
    }

    #[test]
    fn utf8_character_split_across_chunks_survives() {
        let line = "data: {\"text\":\"café\"}\n";
        let bytes = line.as_bytes();
        let split = bytes
            .windows(2)
            .position(|window| window == [0xC3, 0xA9])
            .expect("é is U+00E9 utf-8 c3 a9")
            + 1;
        let mut buf = Vec::new();
        let first = push_sse_bytes(&mut buf, &bytes[..split]);
        assert!(first.is_empty(), "incomplete line must stay buffered");
        assert!(!buf.is_empty());
        let second = push_sse_bytes(&mut buf, &bytes[split..]);
        assert_eq!(second, vec!["data: {\"text\":\"café\"}".to_string()]);
        assert!(buf.is_empty());
        assert!(!second[0].contains('\u{FFFD}'));
    }
}
