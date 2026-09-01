//! ClosedRouter gateway library: Axum router, Postgres catalog, and protocol translation.
//!
//! Incoming clients may speak **OpenAI**, **Anthropic**, **DeepSeek**, or **GLM** (Zhipu).
//! Upstreams of a different dialect are translated, including SSE streams.

pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod hermes;
pub mod observability;
pub mod routes;
pub mod translate;
pub mod upstream;
pub mod url_policy;

use crate::config::Config;
use crate::url_policy::UpstreamUrlPolicy;
use anyhow::Context;
use axum::http::{header, HeaderValue, Method};
use axum::routing::{delete, get, patch, post};
use axum::Router;
use clap::Parser;
use metrics_exporter_prometheus::PrometheusHandle;
use sqlx::PgPool;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::cors::{AllowHeaders, AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub http: reqwest::Client,
    pub admin_token: String,
    pub metrics_token: Option<String>,
    pub upstream_url_policy: UpstreamUrlPolicy,
    pub metrics: PrometheusHandle,
}

#[derive(Parser, Debug)]
#[command(
    name = "closedrouter",
    about = "Self-hosted OpenAI/Anthropic/DeepSeek/GLM LLM gateway"
)]
struct Cli {
    /// YAML config file (overrides CLOSEDROUTER_CONFIG)
    #[arg(short, long, env = "CLOSEDROUTER_CONFIG")]
    config: Option<PathBuf>,
}

/// Process entry used by the `closedrouter` binary.
pub async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_tracing();

    let config = Config::load(cli.config.as_deref())?;
    let db_url = config.database_url()?;
    let db = db::connect(db_url)
        .await
        .with_context(|| format!("connecting to postgres at {db_url}"))?;

    let admin_token = resolve_admin_token(&db, config.admin_token.clone()).await?;
    seed_from_config(&db, &config).await?;

    let metrics = observability::install_metrics()?;
    let bind = config.bind_addr()?;
    let state = AppState {
        db,
        http: upstream::http_client()?,
        admin_token: admin_token.clone(),
        metrics_token: config.metrics_token.clone(),
        upstream_url_policy: config.upstream_url_policy(),
        config: Arc::new(config.clone()),
        metrics,
    };

    let app = router(state, &config.cors_origins);

    tracing::info!(%bind, "ClosedRouter gateway listening");
    if config.admin_token.is_none() {
        tracing::warn!(
            "ADMIN_TOKEN was not set; using persisted token ending …{}",
            suffix(&admin_token, 6)
        );
        tracing::warn!("Set ADMIN_TOKEN in the environment for a stable admin secret");
    }

    let listener = TcpListener::bind(bind)
        .await
        .with_context(|| format!("bind {bind}"))?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Build the public + admin router.
pub fn router(state: AppState, cors_origins: &[String]) -> Router {
    Router::new()
        .route("/health", get(routes::health))
        .route("/metrics", get(routes::metrics))
        .route("/v1/models", get(routes::list_openai_models))
        .route("/v1/chat/completions", post(routes::chat_completions))
        .route("/chat/completions", post(routes::deepseek_chat_completions))
        .route("/v4/chat/completions", post(routes::glm_chat_completions))
        .route("/v1/messages", post(routes::anthropic_messages))
        .route("/v1/embeddings", post(routes::embeddings))
        .route(
            "/v1/hermes/sessions",
            get(routes::hermes_list_sessions).post(routes::hermes_create_session),
        )
        .route(
            "/v1/hermes/memories",
            get(routes::hermes_list_memories).post(routes::hermes_store_memory),
        )
        .route(
            "/v1/hermes/memories/search",
            post(routes::hermes_search_memories),
        )
        .route("/admin/v1/status", get(routes::admin_status))
        .route(
            "/admin/v1/keys",
            get(routes::admin_list_keys).post(routes::admin_create_key),
        )
        .route("/admin/v1/keys/{id}", delete(routes::admin_revoke_key))
        .route(
            "/admin/v1/providers",
            get(routes::admin_list_providers).post(routes::admin_create_provider),
        )
        .route(
            "/admin/v1/providers/{id}",
            patch(routes::admin_patch_provider).delete(routes::admin_delete_provider),
        )
        .route(
            "/admin/v1/models",
            get(routes::admin_list_models).post(routes::admin_create_model),
        )
        .route(
            "/admin/v1/models/{id}",
            patch(routes::admin_patch_model).delete(routes::admin_delete_model),
        )
        .layer(RequestBodyLimitLayer::new(16 * 1024 * 1024))
        .layer(cors_layer(cors_origins))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

fn cors_layer(origins: &[String]) -> CorsLayer {
    // Cursor, browsers, and SDKs send a wide set of headers (http-referer, x-title, OpenAI-Beta, …).
    let layer = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(AllowHeaders::any())
        .expose_headers([
            header::CONTENT_TYPE,
            header::CACHE_CONTROL,
            header::HeaderName::from_static("x-request-id"),
        ]);

    if origins.iter().any(|o| o == "*") {
        layer.allow_origin(AllowOrigin::any())
    } else {
        let parsed: Vec<HeaderValue> = origins
            .iter()
            .filter_map(|o| o.parse::<HeaderValue>().ok())
            .collect();
        layer.allow_origin(AllowOrigin::list(parsed))
    }
}

async fn resolve_admin_token(db: &PgPool, from_env: Option<String>) -> anyhow::Result<String> {
    if let Some(token) = from_env {
        ensure_secure_admin_token(&token)?;
        db::set_setting(db, "admin_token", token.trim()).await.ok();
        return Ok(token.trim().to_string());
    }
    if let Ok(Some(existing)) = db::get_setting(db, "admin_token").await {
        if !is_insecure_admin_token(&existing) {
            return Ok(existing);
        }
        tracing::warn!("ignoring insecure persisted admin token");
    }
    let token = db::generate_admin_token();
    db::set_setting(db, "admin_token", &token).await?;
    tracing::warn!(token, "generated admin token (also stored in the database)");
    Ok(token)
}

/// Public example / empty secrets that must never protect admin routes.
fn is_insecure_admin_token(token: &str) -> bool {
    let token = token.trim();
    token.is_empty() || token.eq_ignore_ascii_case("change-me-now")
}

fn ensure_secure_admin_token(token: &str) -> anyhow::Result<()> {
    if is_insecure_admin_token(token) {
        anyhow::bail!(
            "ADMIN_TOKEN is empty or the public example value 'change-me-now'; set a unique secret in .env"
        );
    }
    Ok(())
}

async fn seed_from_config(db: &PgPool, config: &Config) -> anyhow::Result<()> {
    let policy = config.upstream_url_policy();
    let inserted = db::seed_catalog(db, &policy, &config.providers).await?;
    if inserted > 0 {
        tracing::info!(models = inserted, "seeded catalog from config");
    }
    Ok(())
}

fn suffix(value: &str, n: usize) -> String {
    value
        .chars()
        .rev()
        .take(n)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let json = std::env::var("LOG_FORMAT").ok().as_deref() == Some("json");
    if json {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .compact()
            .init();
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %err, "ctrl-c handler");
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(err) => {
                tracing::error!(error = %err, "SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutting down");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body as HttpBody;
    use axum::http::{Request, StatusCode};
    use sqlx::Row;
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_ok_without_catalog() {
        let Some(state) = test_state().await else {
            assert!(
                !crate::test_support::database_url_configured(),
                "DATABASE_URL is set but Postgres is unreachable"
            );
            eprintln!("skipping: DATABASE_URL unset and testcontainers unavailable");
            return;
        };
        let app = router(state, &["*".into()]);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(HttpBody::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn completions_reject_missing_key() {
        let Some(state) = test_state().await else {
            assert!(
                !crate::test_support::database_url_configured(),
                "DATABASE_URL is set but Postgres is unreachable"
            );
            eprintln!("skipping: DATABASE_URL unset and testcontainers unavailable");
            return;
        };
        let app = router(state, &["*".into()]);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .body(HttpBody::from(r#"{"model":"x","messages":[]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    async fn test_state() -> Option<AppState> {
        let db = crate::test_support::test_pool().await?;
        let metrics = observability::install_metrics().ok()?;
        Some(AppState {
            db,
            http: upstream::http_client().ok()?,
            admin_token: "test-admin".into(),
            metrics_token: Some("test-metrics".into()),
            upstream_url_policy: crate::url_policy::UpstreamUrlPolicy::allow_loopback_for_tests(),
            config: Arc::new(Config::default()),
            metrics,
        })
    }

    #[test]
    fn insecure_admin_token_rejected() {
        assert!(is_insecure_admin_token(""));
        assert!(is_insecure_admin_token("   "));
        assert!(is_insecure_admin_token("change-me-now"));
        assert!(is_insecure_admin_token("Change-Me-Now"));
        assert!(ensure_secure_admin_token("change-me-now").is_err());
        assert!(ensure_secure_admin_token("a-unique-secret").is_ok());
        assert!(!is_insecure_admin_token("a-unique-secret"));
    }

    #[tokio::test]
    async fn metrics_rejects_unauthenticated_requests() {
        let Some(state) = test_state().await else {
            eprintln!("skipping metrics auth test");
            return;
        };
        let app = router(state, &["*".into()]);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(HttpBody::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn metrics_accepts_admin_token() {
        let Some(state) = test_state().await else {
            eprintln!("skipping metrics auth test");
            return;
        };
        let app = router(state.clone(), &["*".into()]);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .header("x-admin-token", "test-admin")
                    .body(HttpBody::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn metrics_accepts_metrics_token() {
        let Some(state) = test_state().await else {
            eprintln!("skipping metrics auth test");
            return;
        };
        let app = router(state, &["*".into()]);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .header("x-metrics-token", "test-metrics")
                    .body(HttpBody::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn admin_create_provider_rejects_ssrf_url() {
        let Some(state) = test_state().await else {
            eprintln!("skipping provider ssrf test");
            return;
        };
        let app = router(state, &["*".into()]);
        let body = serde_json::json!({
            "name": "evil",
            "kind": "openai",
            "base_url": "http://169.254.169.254/latest/meta-data"
        });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/v1/providers")
                    .header("content-type", "application/json")
                    .header("x-admin-token", "test-admin")
                    .body(HttpBody::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn openai_stream_http_records_usage() {
        use http_body_util::BodyExt;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let Some(state) = test_state().await else {
            eprintln!("skipping openai stream http test");
            return;
        };

        let mock = MockServer::start().await;
        let sse = concat!(
            "data: {\"id\":\"c1\",\"choices\":[{\"delta\":{\"content\":\"hi\"},\"index\":0,\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"c1\",\"choices\":[{\"delta\":{},\"index\":0,\"finish_reason\":\"stop\"}],",
            "\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\n\n",
            "data: [DONE]\n\n"
        );
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse),
            )
            .mount(&mock)
            .await;

        let provider = db::create_provider(
            &state.db,
            &state.upstream_url_policy,
            &format!("stream-openai-{}", uuid::Uuid::new_v4()),
            "openai",
            &format!("{}/v1", mock.uri()),
            Some("sk-test"),
        )
        .await
        .expect("provider");
        let model_id = format!("stream-openai-{}", uuid::Uuid::new_v4());
        db::create_model(
            &state.db,
            &model_id,
            &provider.id,
            "upstream-model",
            None,
            None,
        )
        .await
        .expect("model");
        let (_key, plaintext) = db::create_api_key(&state.db, "stream-http")
            .await
            .expect("key");

        let app = router(state.clone(), &["*".into()]);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {plaintext}"))
                    .body(HttpBody::from(
                        serde_json::json!({
                            "model": model_id,
                            "messages": [{"role":"user","content":"ping"}],
                            "stream": true
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.expect("body").to_bytes();
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("data:"));
        assert!(text.contains("hi"));

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let logs = sqlx::query("SELECT prompt_tokens, completion_tokens FROM request_logs ORDER BY created_at DESC LIMIT 1")
            .fetch_one(&state.db)
            .await
            .expect("log row");
        let prompt: Option<i32> = logs.try_get("prompt_tokens").expect("prompt");
        let completion: Option<i32> = logs.try_get("completion_tokens").expect("completion");
        assert_eq!(prompt, Some(3));
        assert_eq!(completion, Some(2));
    }

    #[tokio::test]
    async fn anthropic_translated_stream_http_maps_finish_reason() {
        use http_body_util::BodyExt;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let Some(state) = test_state().await else {
            eprintln!("skipping anthropic stream http test");
            return;
        };

        let mock = MockServer::start().await;
        let sse = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude\",\"usage\":{\"input_tokens\":4}}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"pong\"}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        Mock::given(method("POST"))
            .and(path("/messages"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse),
            )
            .mount(&mock)
            .await;

        let provider = db::create_provider(
            &state.db,
            &state.upstream_url_policy,
            &format!("stream-anthropic-{}", uuid::Uuid::new_v4()),
            "anthropic",
            &mock.uri(),
            Some("sk-ant"),
        )
        .await
        .expect("provider");
        let model_id = format!("stream-anthropic-{}", uuid::Uuid::new_v4());
        db::create_model(
            &state.db,
            &model_id,
            &provider.id,
            "claude-3",
            None,
            None,
        )
        .await
        .expect("model");
        let (_key, plaintext) = db::create_api_key(&state.db, "stream-anthropic-http")
            .await
            .expect("key");

        let app = router(state, &["*".into()]);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {plaintext}"))
                    .body(HttpBody::from(
                        serde_json::json!({
                            "model": model_id,
                            "messages": [{"role":"user","content":"ping"}],
                            "stream": true
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.expect("body").to_bytes();
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("pong"));
        assert!(text.contains("\"finish_reason\":\"stop\""));
    }
}

/// Test helpers (Postgres URL via env or testcontainers).
#[cfg(test)]
pub mod test_support {
    use sqlx::PgPool;
    use std::sync::OnceLock;
    use tokio::sync::Mutex;

    static URL: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    static POOL: OnceLock<Mutex<Option<PgPool>>> = OnceLock::new();

    pub fn database_url_configured() -> bool {
        std::env::var("DATABASE_URL")
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    }

    pub async fn postgres_url() -> Option<String> {
        let slot = URL.get_or_init(|| Mutex::new(None));
        let mut guard = slot.lock().await;
        if let Some(url) = guard.as_ref() {
            return Some(url.clone());
        }
        if let Ok(url) = std::env::var("DATABASE_URL") {
            if !url.is_empty() {
                *guard = Some(url.clone());
                return Some(url);
            }
        }
        match start_pgvector().await {
            Ok(url) => {
                *guard = Some(url.clone());
                Some(url)
            }
            Err(err) => {
                eprintln!("testcontainers postgres skipped: {err}");
                None
            }
        }
    }

    /// Shared pool so parallel tests apply schema once against one database.
    pub async fn test_pool() -> Option<PgPool> {
        let slot = POOL.get_or_init(|| Mutex::new(None));
        let mut guard = slot.lock().await;
        if let Some(pool) = guard.as_ref() {
            return Some(pool.clone());
        }
        let url = postgres_url().await?;
        match crate::db::connect(&url).await {
            Ok(pool) => {
                *guard = Some(pool.clone());
                Some(pool)
            }
            Err(err) => {
                eprintln!("postgres connect failed: {err:#}");
                None
            }
        }
    }

    async fn start_pgvector() -> anyhow::Result<String> {
        use testcontainers::core::{IntoContainerPort, WaitFor};
        use testcontainers::runners::AsyncRunner;
        use testcontainers::{GenericImage, ImageExt};

        let container = GenericImage::new("pgvector/pgvector", "pg16")
            .with_exposed_port(5432.tcp())
            .with_wait_for(WaitFor::message_on_stderr(
                "database system is ready to accept connections",
            ))
            .with_env_var("POSTGRES_USER", "closedrouter")
            .with_env_var("POSTGRES_PASSWORD", "closedrouter")
            .with_env_var("POSTGRES_DB", "closedrouter")
            .start()
            .await?;
        let port = container.get_host_port_ipv4(5432).await?;
        // Keep the container alive for the process by leaking it.
        std::mem::forget(container);
        Ok(format!(
            "postgres://closedrouter:closedrouter@127.0.0.1:{port}/closedrouter"
        ))
    }
}
