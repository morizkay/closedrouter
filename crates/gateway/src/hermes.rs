//! Hermes memory API: store, list, and recall by embedding or text.
//!
//! See `docs/HERMES.md` for the contract. Embeddings are 1536-dimensional
//! OpenAI-compatible vectors produced via `/v1/embeddings`.

use crate::auth::ApiAuth;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::upstream;
use crate::AppState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct CreateSessionBody {
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Deserialize)]
pub struct StoreMemoryBody {
    pub content: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub embedding: Option<Vec<f32>>,
    /// When true, embed `content` via the configured OpenAI-compat embedding model.
    #[serde(default)]
    pub embed: bool,
}

#[derive(Deserialize)]
pub struct SearchBody {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub embedding: Option<Vec<f32>>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
    /// When true and `query` is set, embed the query before vector search.
    #[serde(default)]
    pub embed: bool,
}

#[derive(Deserialize)]
pub struct ListMemoriesQuery {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

pub async fn create_session(
    State(state): State<AppState>,
    auth: ApiAuth,
    Json(body): Json<CreateSessionBody>,
) -> AppResult<(StatusCode, Json<db::HermesSession>)> {
    let session = db::create_hermes_session(
        &state.db,
        &auth.key_id,
        body.title.as_deref(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(session)))
}

pub async fn list_sessions(
    State(state): State<AppState>,
    auth: ApiAuth,
) -> AppResult<Json<Value>> {
    let data = db::list_hermes_sessions(&state.db, &auth.key_id).await?;
    Ok(Json(json!({ "data": data })))
}

pub async fn store_memory(
    State(state): State<AppState>,
    auth: ApiAuth,
    Json(body): Json<StoreMemoryBody>,
) -> AppResult<(StatusCode, Json<db::HermesMemory>)> {
    let mut embedding = body.embedding;
    if embedding.is_none() && body.embed {
        embedding = Some(embed_via_catalog(&state, &body.content).await?);
    }
    let metadata = body.metadata.unwrap_or_else(|| json!({}));
    let memory = db::store_hermes_memory(
        &state.db,
        &auth.key_id,
        &body.content,
        body.session_id.as_deref(),
        body.agent.as_deref(),
        metadata,
        embedding,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(memory)))
}

pub async fn search_memories(
    State(state): State<AppState>,
    auth: ApiAuth,
    Json(body): Json<SearchBody>,
) -> AppResult<Json<Value>> {
    let mut embedding = body.embedding;
    if embedding.is_none() && body.embed {
        let query = body
            .query
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AppError::BadRequest("embed=true requires query text".into()))?;
        embedding = Some(embed_via_catalog(&state, query).await?);
    }
    let data = db::search_hermes_memories(
        &state.db,
        &auth.key_id,
        body.query.as_deref(),
        embedding,
        body.session_id.as_deref(),
        body.agent.as_deref(),
        body.limit.unwrap_or(8),
    )
    .await?;
    Ok(Json(json!({ "data": data })))
}

pub async fn list_memories(
    State(state): State<AppState>,
    auth: ApiAuth,
    Query(query): Query<ListMemoriesQuery>,
) -> AppResult<Json<Value>> {
    let data = db::list_hermes_memories(
        &state.db,
        &auth.key_id,
        query.session_id.as_deref(),
        query.limit.unwrap_or(50),
    )
    .await?;
    Ok(Json(json!({ "data": data })))
}

async fn embed_via_catalog(state: &AppState, text: &str) -> AppResult<Vec<f32>> {
    let route = db::resolve_embedding_model(
        &state.db,
        state.config.embedding_model.as_deref(),
    )
    .await?;
    upstream::embed_text(
        &state.upstream_url_policy,
        &route.provider,
        &route.upstream_model,
        text,
    )
    .await
}
