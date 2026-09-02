//! Prometheus metrics and optional Langfuse generation export.

use crate::config::LangfuseConfig;
use anyhow::Context;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use metrics::{counter, histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Duration;
use uuid::Uuid;

static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();
static LANGFUSE_HTTP: OnceLock<Client> = OnceLock::new();

fn langfuse_http_client() -> &'static Client {
    LANGFUSE_HTTP.get_or_init(|| {
        Client::builder()
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build langfuse HTTP client")
    })
}

/// Install the Prometheus recorder once per process.
pub fn install_metrics() -> anyhow::Result<PrometheusHandle> {
    if let Some(existing) = HANDLE.get() {
        return Ok(existing.clone());
    }
    let handle = PrometheusBuilder::new()
        .install_recorder()
        .context("install prometheus recorder")?;
    let _ = HANDLE.set(handle.clone());
    Ok(handle)
}

/// Record a proxied LLM request.
pub fn record_request(
    protocol: &str,
    status: u16,
    latency_ms: u64,
    prompt_tokens: Option<i32>,
    completion_tokens: Option<i32>,
    upstream_error: bool,
) {
    let status_label = status.to_string();
    counter!("closedrouter_requests_total", "protocol" => protocol.to_string(), "status" => status_label)
        .increment(1);
    histogram!("closedrouter_request_duration_seconds").record(latency_ms as f64 / 1000.0);
    if let Some(n) = prompt_tokens {
        if n > 0 {
            counter!("closedrouter_tokens_total", "type" => "prompt").increment(n as u64);
        }
    }
    if let Some(n) = completion_tokens {
        if n > 0 {
            counter!("closedrouter_tokens_total", "type" => "completion").increment(n as u64);
        }
    }
    if upstream_error {
        counter!("closedrouter_upstream_errors_total", "protocol" => protocol.to_string())
            .increment(1);
    }
}

/// Payload for an optional Langfuse generation export.
pub struct TraceEvent<'a> {
    pub protocol: &'a str,
    pub model: &'a str,
    pub key_id: &'a str,
    pub latency_ms: u64,
    pub prompt_tokens: Option<i32>,
    pub completion_tokens: Option<i32>,
    pub status: u16,
}

/// Fire-and-forget Langfuse ingestion. Never fails the user request.
pub fn spawn_trace(cfg: LangfuseConfig, event: TraceEvent<'_>) {
    if !cfg.enabled {
        return;
    }
    let Some(host) = cfg.host.filter(|s| !s.is_empty()) else {
        return;
    };
    let Some(public) = cfg.public_key.filter(|s| !s.is_empty()) else {
        return;
    };
    let Some(secret) = cfg.secret_key.filter(|s| !s.is_empty()) else {
        return;
    };

    let now = chrono::Utc::now();
    let start = now - chrono::Duration::milliseconds(event.latency_ms as i64);
    let generation_id = Uuid::new_v4().to_string();
    let trace_id = Uuid::new_v4().to_string();
    let event_id = Uuid::new_v4().to_string();
    let protocol = event.protocol.to_string();
    let model = event.model.to_string();
    let key_id = event.key_id.to_string();
    let prompt_tokens = event.prompt_tokens;
    let completion_tokens = event.completion_tokens;
    let status = event.status;
    let body = json!({
        "batch": [{
            "id": event_id,
            "type": "generation-create",
            "timestamp": now.to_rfc3339(),
            "body": {
                "id": generation_id,
                "traceId": trace_id,
                "name": "closedrouter.chat",
                "model": model,
                "startTime": start.to_rfc3339(),
                "endTime": now.to_rfc3339(),
                "usage": {
                    "input": prompt_tokens.unwrap_or(0),
                    "output": completion_tokens.unwrap_or(0),
                    "total": prompt_tokens.unwrap_or(0) + completion_tokens.unwrap_or(0),
                    "unit": "TOKENS"
                },
                "metadata": {
                    "protocol": protocol,
                    "key_id": key_id,
                    "status": status
                }
            }
        }]
    });

    tokio::spawn(async move {
        let http = langfuse_http_client();
        if let Err(err) = post_langfuse(http, &host, &public, &secret, body).await {
            tracing::debug!(error = %err, "langfuse export skipped");
        }
    });
}

async fn post_langfuse(
    http: &reqwest::Client,
    host: &str,
    public: &str,
    secret: &str,
    body: Value,
) -> anyhow::Result<()> {
    let url = format!(
        "{}/api/public/ingestion",
        host.trim_end_matches('/')
    );
    let token = B64.encode(format!("{public}:{secret}"));
    let response = http
        .post(url)
        .header("Authorization", format!("Basic {token}"))
        .header("Content-Type", "application/json")
        .timeout(Duration::from_secs(5))
        .json(&body)
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("langfuse {status}: {text}");
    }
    Ok(())
}
