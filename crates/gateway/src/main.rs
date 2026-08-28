mod auth;
mod config;
mod db;
mod error;
mod routes;
mod translate;
mod upstream;

use crate::config::Config;
use crate::db::Db;
use anyhow::Context;
use axum::http::{header, HeaderValue, Method};
use axum::routing::{delete, get, patch, post};
use axum::Router;
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub config: Arc<Config>,
    pub http: reqwest::Client,
    pub admin_token: String,
}

#[derive(Parser, Debug)]
#[command(
    name = "closedrouter",
    about = "Self-hosted OpenAI/Anthropic LLM gateway"
)]
struct Cli {
    /// YAML config file (overrides CLOSEDROUTER_CONFIG)
    #[arg(short, long, env = "CLOSEDROUTER_CONFIG")]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_tracing();

    let config = Config::load(cli.config.as_deref())?;
    let db = db::open(&config.database_path)
        .with_context(|| format!("opening database {}", config.database_path.display()))?;

    let admin_token = resolve_admin_token(&db, config.admin_token.clone())?;
    seed_from_config(&db, &config)?;

    let bind = config.bind_addr()?;
    let state = AppState {
        db,
        http: upstream::http_client(),
        admin_token: admin_token.clone(),
        config: Arc::new(config.clone()),
    };

    let app = router(state, &config.cors_origins);

    tracing::info!(%bind, db = %config.database_path.display(), "ClosedRouter gateway listening");
    if config.admin_token.is_none() {
        tracing::warn!(
            "ADMIN_TOKEN was not set; using persisted token ending …{}",
            admin_token
                .chars()
                .rev()
                .take(6)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
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

fn router(state: AppState, cors_origins: &[String]) -> Router {
    Router::new()
        .route("/health", get(routes::health))
        .route("/v1/models", get(routes::list_openai_models))
        .route("/v1/chat/completions", post(routes::chat_completions))
        .route("/v1/messages", post(routes::anthropic_messages))
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
    let layer = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::ACCEPT,
            header::HeaderName::from_static("x-api-key"),
            header::HeaderName::from_static("x-admin-token"),
            header::HeaderName::from_static("anthropic-version"),
            header::HeaderName::from_static("anthropic-beta"),
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

fn resolve_admin_token(db: &Db, from_env: Option<String>) -> anyhow::Result<String> {
    if let Some(token) = from_env {
        db::set_setting(db, "admin_token", &token).ok();
        return Ok(token);
    }
    if let Ok(Some(existing)) = db::get_setting(db, "admin_token") {
        if !existing.is_empty() {
            return Ok(existing);
        }
    }
    let token = db::generate_admin_token();
    db::set_setting(db, "admin_token", &token)?;
    tracing::warn!(token, "generated admin token (also stored in the database)");
    Ok(token)
}

fn seed_from_config(db: &Db, config: &Config) -> anyhow::Result<()> {
    if config.providers.is_empty() {
        return Ok(());
    }
    if db::model_count(db).unwrap_or(0) > 0 {
        return Ok(());
    }
    for provider in &config.providers {
        let record = db::create_provider(
            db,
            &provider.name,
            &provider.kind,
            &provider.base_url,
            provider.api_key.as_deref(),
        )?;
        for model in &provider.models {
            db::create_model(
                db,
                &model.id,
                &record.id,
                &model.upstream_model,
                model.display_name.as_deref(),
            )?;
        }
        tracing::info!(provider = %provider.name, models = provider.models.len(), "seeded provider from config");
    }
    Ok(())
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
        tokio::signal::ctrl_c().await.ok();
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
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

    fn test_state() -> AppState {
        let db = db::open(std::path::Path::new(":memory:")).unwrap();
        AppState {
            db,
            http: upstream::http_client(),
            admin_token: "test-admin".into(),
            config: Arc::new(Config::default()),
        }
    }

    #[tokio::test]
    async fn health_ok() {
        let app = router(test_state(), &["*".into()]);
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
        let app = router(test_state(), &["*".into()]);
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
}
