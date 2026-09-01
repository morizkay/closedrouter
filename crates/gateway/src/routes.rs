//! HTTP handlers: OpenAI / Anthropic / DeepSeek / GLM / Cursor / Hermes / admin.

use crate::auth::{metrics_authorized, AdminAuth, ApiAuth};
use crate::db;
use crate::error::{AppError, AppResult};
use crate::hermes;
use crate::observability;
use crate::translate::{extract_model, Protocol};
use crate::upstream;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub async fn health(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "closedrouter",
        "version": env!("CARGO_PKG_VERSION"),
        "bind": format!("{}:{}", state.config.host, state.config.port),
        "database": "postgres",
    }))
}

pub async fn metrics(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<impl IntoResponse> {
    if !metrics_authorized(&headers, &state) {
        return Err(AppError::Unauthorized(
            "metrics require METRICS_TOKEN or admin credentials".into(),
        ));
    }
    Ok((
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        state.metrics.render(),
    ))
}

fn cursor_model_object(id: &str, created: i64) -> Value {
    json!({
        "id": id,
        "object": "model",
        "created": created,
        "owned_by": "closedrouter",
        "permission": [{
            "id": format!("modelperm-{id}"),
            "object": "model_permission",
            "created": created,
            "allow_create_engine": false,
            "allow_sampling": true,
            "allow_logprobs": true,
            "allow_search_indices": false,
            "allow_view": true,
            "allow_fine_tuning": false,
            "organization": "*",
            "group": null,
            "is_blocking": false
        }],
        "root": id,
        "parent": null
    })
}

pub async fn list_openai_models(
    State(state): State<AppState>,
    _auth: ApiAuth,
) -> AppResult<Json<Value>> {
    let models = db::list_models(&state.db).await?;
    let data: Vec<Value> = models
        .into_iter()
        .map(|m| cursor_model_object(&m.id, m.created_at))
        .collect();
    Ok(Json(json!({"object": "list", "data": data})))
}

async fn proxy_chat(
    state: &AppState,
    auth: &ApiAuth,
    incoming: Protocol,
    body: Value,
) -> AppResult<Response> {
    if !body.is_object() {
        return Err(AppError::BadRequest("JSON object required".into()));
    }
    let model = extract_model(&body)?.to_string();
    let route = db::resolve_model(&state.db, &model).await?;
    tracing::info!(
        key = %auth.key_name,
        model = %model,
        protocol = incoming.as_str(),
        "chat request"
    );
    let (response, meta, stream_usage) = upstream::proxy(
        &state.http,
        &state.upstream_url_policy,
        incoming,
        &route.provider,
        &route.upstream_model,
        body,
    )
    .await?;

    let pool = state.db.clone();
    let protocol = incoming.as_str().to_string();
    let key_id = auth.key_id.clone();
    let upstream_model = route.upstream_model.clone();
    let model_for_log = model.clone();
    let limit = state.config.request_log_limit;

    if let Some(usage_rx) = stream_usage {
        tokio::spawn(async move {
            let usage = usage_rx.await.unwrap_or_default();
            let prompt_tokens = usage.prompt_tokens.or(meta.prompt_tokens);
            let completion_tokens = usage.completion_tokens.or(meta.completion_tokens);
            if let Err(err) = db::insert_request_log(
                &pool,
                db::RequestLogWrite {
                    key_id: Some(&key_id),
                    protocol: &protocol,
                    model: Some(&model_for_log),
                    upstream_model: Some(&upstream_model),
                    status: meta.status as i32,
                    latency_ms: meta.latency_ms.min(i32::MAX as u64) as i32,
                    prompt_tokens,
                    completion_tokens,
                    error: meta.error.as_deref(),
                },
                limit,
            )
            .await
            {
                tracing::debug!(error = %err, "stream request log insert failed");
            }
        });
    } else {
        tokio::spawn(async move {
            if let Err(err) = db::insert_request_log(
                &pool,
                db::RequestLogWrite {
                    key_id: Some(&key_id),
                    protocol: &protocol,
                    model: Some(&model_for_log),
                    upstream_model: Some(&upstream_model),
                    status: meta.status as i32,
                    latency_ms: meta.latency_ms.min(i32::MAX as u64) as i32,
                    prompt_tokens: meta.prompt_tokens,
                    completion_tokens: meta.completion_tokens,
                    error: meta.error.as_deref(),
                },
                limit,
            )
            .await
            {
                tracing::debug!(error = %err, "request log insert failed");
            }
        });
    }

    if state.config.langfuse_enabled() {
        observability::spawn_trace(
            state.http.clone(),
            state.config.langfuse.clone(),
            observability::TraceEvent {
                protocol: incoming.as_str(),
                model: &model,
                key_id: &auth.key_id,
                latency_ms: meta.latency_ms,
                prompt_tokens: meta.prompt_tokens,
                completion_tokens: meta.completion_tokens,
                status: meta.status,
            },
        );
    }

    Ok(response)
}

pub async fn chat_completions(
    State(state): State<AppState>,
    auth: ApiAuth,
    Json(body): Json<Value>,
) -> AppResult<Response> {
    proxy_chat(&state, &auth, Protocol::OpenAi, body).await
}

pub async fn deepseek_chat_completions(
    State(state): State<AppState>,
    auth: ApiAuth,
    Json(body): Json<Value>,
) -> AppResult<Response> {
    proxy_chat(&state, &auth, Protocol::DeepSeek, body).await
}

pub async fn glm_chat_completions(
    State(state): State<AppState>,
    auth: ApiAuth,
    Json(body): Json<Value>,
) -> AppResult<Response> {
    proxy_chat(&state, &auth, Protocol::Glm, body).await
}

pub async fn anthropic_messages(
    State(state): State<AppState>,
    auth: ApiAuth,
    Json(body): Json<Value>,
) -> AppResult<Response> {
    proxy_chat(&state, &auth, Protocol::Anthropic, body).await
}

pub async fn embeddings(
    State(state): State<AppState>,
    auth: ApiAuth,
    Json(body): Json<Value>,
) -> AppResult<Response> {
    if !body.is_object() {
        return Err(AppError::BadRequest("JSON object required".into()));
    }
    let model = extract_model(&body)?.to_string();
    let route = db::resolve_model(&state.db, &model).await?;
    tracing::info!(key = %auth.key_name, model = %model, "embeddings");
    upstream::embeddings(
        &state.http,
        &state.upstream_url_policy,
        &route.provider,
        &route.upstream_model,
        body,
    )
    .await
}

#[derive(Deserialize)]
pub struct CreateKeyBody {
    pub name: String,
}

#[derive(Serialize)]
pub struct CreatedKey {
    pub id: String,
    pub name: String,
    pub key: String,
    pub key_prefix: String,
    pub created_at: i64,
}

pub async fn admin_status(
    State(state): State<AppState>,
    _admin: AdminAuth,
) -> AppResult<Json<Value>> {
    let (keys, providers, models) = db::counts(&state.db).await?;
    Ok(Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "keys": keys,
        "providers": providers,
        "models": models,
        "host": state.config.host,
        "port": state.config.port,
        "embedding_dim": db::embedding_dim(),
        "langfuse": state.config.langfuse_enabled(),
    })))
}

pub async fn admin_list_keys(
    State(state): State<AppState>,
    _admin: AdminAuth,
) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "data": db::list_api_keys(&state.db).await? })))
}

pub async fn admin_create_key(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Json(body): Json<CreateKeyBody>,
) -> AppResult<(StatusCode, Json<CreatedKey>)> {
    let (record, plaintext) = db::create_api_key(&state.db, &body.name).await?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedKey {
            id: record.id,
            name: record.name,
            key: plaintext,
            key_prefix: record.key_prefix,
            created_at: record.created_at,
        }),
    ))
}

pub async fn admin_revoke_key(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    db::revoke_api_key(&state.db, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ProviderBody {
    pub name: String,
    pub kind: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct ProviderPatch {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub clear_api_key: bool,
}

pub async fn admin_list_providers(
    State(state): State<AppState>,
    _admin: AdminAuth,
) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "data": db::list_providers(&state.db).await? })))
}

pub async fn admin_create_provider(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Json(body): Json<ProviderBody>,
) -> AppResult<(StatusCode, Json<db::ProviderRecord>)> {
    let record = db::create_provider(
        &state.db,
        &state.upstream_url_policy,
        &body.name,
        &body.kind,
        &body.base_url,
        body.api_key.as_deref(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(record)))
}

pub async fn admin_patch_provider(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(id): Path<String>,
    Json(body): Json<ProviderPatch>,
) -> AppResult<Json<db::ProviderRecord>> {
    Ok(Json(
        db::update_provider(
            &state.db,
            &state.upstream_url_policy,
            &id,
            body.name.as_deref(),
            body.kind.as_deref(),
            body.base_url.as_deref(),
            body.api_key.as_deref(),
            body.clear_api_key,
        )
        .await?,
    ))
}

pub async fn admin_delete_provider(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    db::delete_provider(&state.db, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ModelBody {
    pub id: String,
    pub provider_id: String,
    pub upstream_model: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub capability: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct ModelPatch {
    pub provider_id: Option<String>,
    pub upstream_model: Option<String>,
    pub display_name: Option<String>,
    pub capability: Option<String>,
}

pub async fn admin_list_models(
    State(state): State<AppState>,
    _admin: AdminAuth,
) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "data": db::list_models(&state.db).await? })))
}

pub async fn admin_create_model(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Json(body): Json<ModelBody>,
) -> AppResult<(StatusCode, Json<db::ModelRecord>)> {
    let record = db::create_model(
        &state.db,
        &body.id,
        &body.provider_id,
        &body.upstream_model,
        body.display_name.as_deref(),
        body.capability.as_deref(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(record)))
}

pub async fn admin_patch_model(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(id): Path<String>,
    Json(body): Json<ModelPatch>,
) -> AppResult<Json<db::ModelRecord>> {
    Ok(Json(
        db::update_model(
            &state.db,
            &id,
            body.provider_id.as_deref(),
            body.upstream_model.as_deref(),
            body.display_name.as_deref(),
            body.capability.as_deref(),
        )
        .await?,
    ))
}

pub async fn admin_delete_model(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    db::delete_model(&state.db, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub use hermes::{
    create_session as hermes_create_session, list_memories as hermes_list_memories,
    list_sessions as hermes_list_sessions, search_memories as hermes_search_memories,
    store_memory as hermes_store_memory,
};
