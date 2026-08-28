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

use crate::config::Config;
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
        db::set_setting(db, "admin_token", &token).await.ok();
        return Ok(token);
    }
    if let Ok(Some(existing)) = db::get_setting(db, "admin_token").await {
        if !existing.is_empty() {
            return Ok(existing);
        }
    }
    let token = db::generate_admin_token();
    db::set_setting(db, "admin_token", &token).await?;
    tracing::warn!(token, "generated admin token (also stored in the database)");
    Ok(token)
}

async fn seed_from_config(db: &PgPool, config: &Config) -> anyhow::Result<()> {
    if config.providers.is_empty() {
        return Ok(());
    }
    if db::model_count(db).await.unwrap_or(0) > 0 {
        return Ok(());
    }
    for provider in &config.providers {
        let record = db::create_provider(
            db,
            &provider.name,
            &provider.kind,
            &provider.base_url,
            provider.api_key.as_deref(),
        )
        .await?;
        for model in &provider.models {
            db::create_model(
                db,
                &model.id,
                &record.id,
                &model.upstream_model,
                model.display_name.as_deref(),
                model.capability.as_deref(),
            )
            .await?;
        }
        tracing::info!(
            provider = %provider.name,
            models = provider.models.len(),
            "seeded provider from config"
        );
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
            config: Arc::new(Config::default()),
            metrics,
        })
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
