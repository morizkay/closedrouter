use crate::db::ProviderSecret;
use crate::error::{AppError, AppResult};
use crate::translate::{
    anthropic_event_to_openai_sse, anthropic_response_to_openai, anthropic_to_openai,
    extract_model, extract_stream, openai_chunk_id, openai_data_to_anthropic_sse,
    openai_response_to_anthropic, openai_to_anthropic, rewrite_model, Protocol,
};
use axum::body::Body;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

pub fn http_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .expect("http client")
}

fn join_url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn upstream_url(provider: &ProviderSecret, protocol: Protocol) -> String {
    match protocol {
        Protocol::OpenAi => join_url(&provider.base_url, "chat/completions"),
        Protocol::Anthropic => join_url(&provider.base_url, "messages"),
    }
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
        _ => {
            if let Some(key) = provider.api_key.as_deref().filter(|k| !k.is_empty()) {
                builder.bearer_auth(key)
            } else {
                builder.bearer_auth("closedrouter")
            }
        }
    }
}

pub async fn proxy(
    client: &Client,
    incoming: Protocol,
    provider: &ProviderSecret,
    upstream_model: &str,
    body: Value,
) -> AppResult<Response> {
    let upstream_kind = Protocol::from_kind(&provider.kind)?;
    let stream = extract_stream(&body);
    let _incoming_model = extract_model(&body)?;

    let (url, payload) = if incoming == upstream_kind {
        (
            upstream_url(provider, upstream_kind),
            rewrite_model(body, upstream_model),
        )
    } else {
        match (incoming, upstream_kind) {
            (Protocol::OpenAi, Protocol::Anthropic) => (
                upstream_url(provider, Protocol::Anthropic),
                openai_to_anthropic(&body, upstream_model)?,
            ),
            (Protocol::Anthropic, Protocol::OpenAi) => (
                upstream_url(provider, Protocol::OpenAi),
                anthropic_to_openai(&body, upstream_model)?,
            ),
            (a, b) => {
                return Err(AppError::Internal(format!(
                    "unhandled protocol pair {a:?} -> {b:?}"
                )));
            }
        }
    };

    tracing::info!(
        incoming = ?incoming,
        upstream = ?upstream_kind,
        url = %url,
        stream,
        model = %upstream_model,
        "proxying completion"
    );

    let request = apply_upstream_auth(client.post(&url).json(&payload), provider);
    let response = request.send().await?;
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);

    if stream {
        return pipe_stream(incoming, upstream_kind, upstream_model, response, status).await;
    }

    let bytes = response.bytes().await?;
    if !status.is_success() {
        return Ok(json_passthrough(status, bytes));
    }

    if incoming == upstream_kind {
        return Ok(json_passthrough(status, bytes));
    }

    let parsed: Value = serde_json::from_slice(&bytes).unwrap_or_else(
        |_| serde_json::json!({"error": {"message": String::from_utf8_lossy(&bytes)}}),
    );
    let translated = match (incoming, upstream_kind) {
        (Protocol::OpenAi, Protocol::Anthropic) => anthropic_response_to_openai(&parsed),
        (Protocol::Anthropic, Protocol::OpenAi) => openai_response_to_anthropic(&parsed),
        _ => parsed,
    };
    Ok(json_value(status, translated))
}

fn json_passthrough(status: StatusCode, bytes: Bytes) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response_fallback())
}

fn json_value(status: StatusCode, value: Value) -> Response {
    let bytes = serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec());
    json_passthrough(status, Bytes::from(bytes))
}

trait FallbackResponse {
    fn into_response_fallback(self) -> Response;
}

impl FallbackResponse for StatusCode {
    fn into_response_fallback(self) -> Response {
        let mut res = Response::new(Body::empty());
        *res.status_mut() = self;
        res
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

    if incoming == upstream {
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
    let byte_stream = response.bytes_stream();
    let converted = async_stream::stream! {
        let mut rest = String::new();
        let mut openai_started = false;
        let mut current_event = String::new();
        futures_util::pin_mut!(byte_stream);
        while let Some(chunk) = byte_stream.next().await {
            match chunk {
                Ok(bytes) => {
                    rest.push_str(&String::from_utf8_lossy(&bytes));
                    loop {
                        let Some(idx) = rest.find('\n') else { break };
                        let mut line = rest[..idx].to_string();
                        rest = rest[idx + 1..].to_string();
                        if line.ends_with('\r') {
                            line.pop();
                        }
                        if line.is_empty() {
                            if incoming == Protocol::OpenAi && upstream == Protocol::Anthropic {
                                // Anthropic uses event/data pairs flushed on blank lines.
                                continue;
                            }
                            continue;
                        }
                        let frames = match (incoming, upstream) {
                            (Protocol::OpenAi, Protocol::Anthropic) => {
                                convert_anthropic_line(&line, &mut current_event, &id, &model_owned)
                            }
                            (Protocol::Anthropic, Protocol::OpenAi) => {
                                convert_openai_line(&line, &mut openai_started)
                            }
                            _ => Vec::new(),
                        };
                        for frame in frames {
                            yield Ok::<Bytes, std::io::Error>(Bytes::from(format!("{frame}\n\n")));
                        }
                    }
                }
                Err(err) => {
                    yield Err(std::io::Error::other(err.to_string()));
                    break;
                }
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

fn convert_openai_line(line: &str, started: &mut bool) -> Vec<String> {
    let Some(data) = line.strip_prefix("data:") else {
        return Vec::new();
    };
    openai_data_to_anthropic_sse(data.trim(), started)
}
