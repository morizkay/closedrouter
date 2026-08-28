use crate::auth::{AdminAuth, ApiAuth};
use crate::db;
use crate::error::{AppError, AppResult};
use crate::translate::{extract_model, Protocol};
use crate::upstream;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub async fn health(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "closedrouter",
        "version": env!("CARGO_PKG_VERSION"),
        "bind": format!("{}:{}", state.config.host, state.config.port),
    }))
}

pub async fn list_openai_models(
    State(state): State<AppState>,
    _auth: ApiAuth,
) -> AppResult<Json<Value>> {
    let models = db::list_models(&state.db)?;
    let data: Vec<Value> = models
        .into_iter()
        .map(|m| {
            json!({
                "id": m.id,
                "object": "model",
                "created": m.created_at,
                "owned_by": "closedrouter",
            })
        })
        .collect();
    Ok(Json(json!({"object": "list", "data": data})))
}

pub async fn chat_completions(
    State(state): State<AppState>,
    auth: ApiAuth,
    Json(body): Json<Value>,
) -> AppResult<Response> {
    if !body.is_object() {
        return Err(AppError::BadRequest("JSON object required".into()));
    }
    let model = extract_model(&body)?.to_string();
    let route = db::resolve_model(&state.db, &model)?;
    tracing::info!(key = %auth.key_name, model = %model, "openai chat completions");
    upstream::proxy(
        &state.http,
        Protocol::OpenAi,
        &route.provider,
        &route.upstream_model,
        body,
    )
    .await
}

pub async fn anthropic_messages(
    State(state): State<AppState>,
    auth: ApiAuth,
    Json(body): Json<Value>,
) -> AppResult<Response> {
    if !body.is_object() {
        return Err(AppError::BadRequest("JSON object required".into()));
    }
    let model = extract_model(&body)?.to_string();
    let route = db::resolve_model(&state.db, &model)?;
    tracing::info!(key = %auth.key_name, model = %model, "anthropic messages");
    upstream::proxy(
        &state.http,
        Protocol::Anthropic,
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
    let (keys, providers, models) = db::counts(&state.db)?;
    Ok(Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "keys": keys,
        "providers": providers,
        "models": models,
        "host": state.config.host,
        "port": state.config.port,
    })))
}

pub async fn admin_list_keys(
    State(state): State<AppState>,
    _admin: AdminAuth,
) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "data": db::list_api_keys(&state.db)? })))
}

pub async fn admin_create_key(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Json(body): Json<CreateKeyBody>,
) -> AppResult<(StatusCode, Json<CreatedKey>)> {
    let (record, plaintext) = db::create_api_key(&state.db, &body.name)?;
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
    db::revoke_api_key(&state.db, &id)?;
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
    Ok(Json(json!({ "data": db::list_providers(&state.db)? })))
}

pub async fn admin_create_provider(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Json(body): Json<ProviderBody>,
) -> AppResult<(StatusCode, Json<db::ProviderRecord>)> {
    let record = db::create_provider(
        &state.db,
        &body.name,
        &body.kind,
        &body.base_url,
        body.api_key.as_deref(),
    )?;
    Ok((StatusCode::CREATED, Json(record)))
}

pub async fn admin_patch_provider(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(id): Path<String>,
    Json(body): Json<ProviderPatch>,
) -> AppResult<Json<db::ProviderRecord>> {
    Ok(Json(db::update_provider(
        &state.db,
        &id,
        body.name.as_deref(),
        body.kind.as_deref(),
        body.base_url.as_deref(),
        body.api_key.as_deref(),
        body.clear_api_key,
    )?))
}

pub async fn admin_delete_provider(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    db::delete_provider(&state.db, &id)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ModelBody {
    pub id: String,
    pub provider_id: String,
    pub upstream_model: String,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct ModelPatch {
    pub provider_id: Option<String>,
    pub upstream_model: Option<String>,
    pub display_name: Option<String>,
}

pub async fn admin_list_models(
    State(state): State<AppState>,
    _admin: AdminAuth,
) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "data": db::list_models(&state.db)? })))
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
    )?;
    Ok((StatusCode::CREATED, Json(record)))
}

pub async fn admin_patch_model(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(id): Path<String>,
    Json(body): Json<ModelPatch>,
) -> AppResult<Json<db::ModelRecord>> {
    Ok(Json(db::update_model(
        &state.db,
        &id,
        body.provider_id.as_deref(),
        body.upstream_model.as_deref(),
        body.display_name.as_deref(),
    )?))
}

pub async fn admin_delete_model(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    db::delete_model(&state.db, &id)?;
    Ok(StatusCode::NO_CONTENT)
}
