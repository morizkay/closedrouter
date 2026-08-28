//! PostgreSQL + pgvector access for keys, catalog, logs, and Hermes memory.

use crate::error::{AppError, AppResult};
use chrono::{DateTime, Utc};
use pgvector::Vector;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use uuid::Uuid;

const EMBEDDING_DIM: usize = 1536;
const SCHEMA_SQL: &str = include_str!("schema.sql");

/// Public API key row (never includes the hash).
#[derive(Debug, Clone, Serialize)]
pub struct ApiKeyRecord {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub created_at: i64,
    pub revoked_at: Option<i64>,
}

/// Provider as shown in the dashboard (secret redacted).
#[derive(Debug, Clone, Serialize)]
pub struct ProviderRecord {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub has_api_key: bool,
    pub created_at: i64,
}

/// Provider including the upstream credential, used only on the proxy path.
#[derive(Debug, Clone)]
pub struct ProviderSecret {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub api_key: Option<String>,
}

/// Catalog model mapping.
#[derive(Debug, Clone, Serialize)]
pub struct ModelRecord {
    pub id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub provider_kind: String,
    pub upstream_model: String,
    pub display_name: String,
    pub capability: String,
    pub created_at: i64,
}

/// Resolved route for a client-facing model id.
#[derive(Debug, Clone)]
pub struct ModelRoute {
    pub id: String,
    pub upstream_model: String,
    pub capability: String,
    pub provider: ProviderSecret,
}

/// Bounded request log row.
#[derive(Debug, Clone, Serialize)]
pub struct RequestLogRecord {
    pub id: String,
    pub key_id: Option<String>,
    pub protocol: String,
    pub model: Option<String>,
    pub upstream_model: Option<String>,
    pub status: Option<i32>,
    pub latency_ms: Option<i32>,
    pub prompt_tokens: Option<i32>,
    pub completion_tokens: Option<i32>,
    pub error: Option<String>,
    pub created_at: i64,
}

/// Hermes session/thread.
#[derive(Debug, Clone, Serialize)]
pub struct HermesSession {
    pub id: String,
    pub key_id: String,
    pub title: Option<String>,
    pub created_at: String,
}

/// Stored memory without the raw embedding vector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HermesMemory {
    pub id: String,
    pub key_id: String,
    pub session_id: Option<String>,
    pub agent: Option<String>,
    pub content: String,
    pub metadata: Value,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance: Option<f64>,
}

/// Connect and apply schema (idempotent).
pub async fn connect(database_url: &str) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;
    apply_schema(&pool).await?;
    Ok(pool)
}

/// Apply `schema.sql`. Safe to run on every boot.
pub async fn apply_schema(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::raw_sql(SCHEMA_SQL).execute(pool).await?;
    Ok(())
}

fn ts(dt: DateTime<Utc>) -> i64 {
    dt.timestamp()
}

fn rfc3339(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339()
}

/// SHA-256 hex of a ClosedRouter API key.
pub fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

/// Mint a `sk-cr-…` client key.
pub fn generate_api_key() -> String {
    let mut bytes = [0u8; 24];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut bytes);
    format!("sk-cr-{}", hex::encode(bytes))
}

/// Mint an admin token when `ADMIN_TOKEN` is unset.
pub fn generate_admin_token() -> String {
    let mut bytes = [0u8; 24];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut bytes);
    format!("cr-admin-{}", hex::encode(bytes))
}

pub async fn get_setting(pool: &PgPool, key: &str) -> AppResult<Option<String>> {
    let row = sqlx::query("SELECT value FROM settings WHERE key = $1")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row
        .map(|r| r.try_get::<String, _>("value"))
        .transpose()?)
}

pub async fn set_setting(pool: &PgPool, key: &str, value: &str) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO settings(key, value) VALUES ($1, $2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn create_api_key(pool: &PgPool, name: &str) -> AppResult<(ApiKeyRecord, String)> {
    let name = require_nonempty(name, "name")?;
    let plaintext = generate_api_key();
    let id = Uuid::new_v4().to_string();
    let key_prefix: String = plaintext.chars().take(12).collect();
    let hash = hash_key(&plaintext);
    let row = sqlx::query(
        "INSERT INTO api_keys (id, name, key_prefix, key_hash)
         VALUES ($1, $2, $3, $4)
         RETURNING id, name, key_prefix, created_at, revoked_at",
    )
    .bind(&id)
    .bind(&name)
    .bind(&key_prefix)
    .bind(&hash)
    .fetch_one(pool)
    .await?;
    Ok((api_key_from_row(&row)?, plaintext))
}

pub async fn list_api_keys(pool: &PgPool) -> AppResult<Vec<ApiKeyRecord>> {
    let rows = sqlx::query(
        "SELECT id, name, key_prefix, created_at, revoked_at FROM api_keys ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    rows.iter().map(api_key_from_row).collect()
}

pub async fn revoke_api_key(pool: &PgPool, id: &str) -> AppResult<()> {
    let n = sqlx::query(
        "UPDATE api_keys SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL",
    )
    .bind(id)
    .execute(pool)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound("API key not found".into()));
    }
    Ok(())
}

pub async fn find_valid_key(pool: &PgPool, bearer: &str) -> AppResult<Option<ApiKeyRecord>> {
    let hash = hash_key(bearer);
    let row = sqlx::query(
        "SELECT id, name, key_prefix, created_at, revoked_at FROM api_keys
         WHERE key_hash = $1 AND revoked_at IS NULL",
    )
    .bind(hash)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(api_key_from_row).transpose()
}

pub async fn create_provider(
    pool: &PgPool,
    name: &str,
    kind: &str,
    base_url: &str,
    api_key: Option<&str>,
) -> AppResult<ProviderRecord> {
    let kind = normalize_kind(kind)?;
    let name = require_nonempty(name, "name")?;
    let base_url = require_nonempty(base_url, "base_url")?;
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO providers (id, name, kind, base_url, api_key) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&kind)
    .bind(&base_url)
    .bind(api_key)
    .execute(pool)
    .await?;
    get_provider(pool, &id)
        .await?
        .ok_or_else(|| AppError::Internal("provider insert failed".into()))
}

pub async fn update_provider(
    pool: &PgPool,
    id: &str,
    name: Option<&str>,
    kind: Option<&str>,
    base_url: Option<&str>,
    api_key: Option<&str>,
    clear_api_key: bool,
) -> AppResult<ProviderRecord> {
    let kind = match kind {
        Some(k) => Some(normalize_kind(k)?),
        None => None,
    };
    let existing = get_provider_secret(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("provider not found".into()))?;
    let name = name
        .map(|v| require_nonempty(v, "name"))
        .transpose()?
        .unwrap_or(existing.name);
    let kind = kind.unwrap_or(existing.kind);
    let base_url = base_url
        .map(|v| require_nonempty(v, "base_url"))
        .transpose()?
        .unwrap_or(existing.base_url);
    let next_key = if clear_api_key {
        None
    } else if let Some(key) = api_key {
        if key.is_empty() {
            existing.api_key
        } else {
            Some(key.to_string())
        }
    } else {
        existing.api_key
    };
    sqlx::query(
        "UPDATE providers SET name = $1, kind = $2, base_url = $3, api_key = $4 WHERE id = $5",
    )
    .bind(&name)
    .bind(&kind)
    .bind(&base_url)
    .bind(&next_key)
    .bind(id)
    .execute(pool)
    .await?;
    get_provider(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("provider not found".into()))
}

pub async fn delete_provider(pool: &PgPool, id: &str) -> AppResult<()> {
    let n = sqlx::query("DELETE FROM providers WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound("provider not found".into()));
    }
    Ok(())
}

pub async fn list_providers(pool: &PgPool) -> AppResult<Vec<ProviderRecord>> {
    let rows = sqlx::query(
        "SELECT id, name, kind, base_url, api_key, created_at FROM providers ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    rows.iter().map(provider_from_row).collect()
}

pub async fn get_provider(pool: &PgPool, id: &str) -> AppResult<Option<ProviderRecord>> {
    let row = sqlx::query(
        "SELECT id, name, kind, base_url, api_key, created_at FROM providers WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(provider_from_row).transpose()
}

pub async fn get_provider_secret(pool: &PgPool, id: &str) -> AppResult<Option<ProviderSecret>> {
    let row = sqlx::query("SELECT id, name, kind, base_url, api_key FROM providers WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(provider_secret_from_row).transpose()
}

pub async fn create_model(
    pool: &PgPool,
    id: &str,
    provider_id: &str,
    upstream_model: &str,
    display_name: Option<&str>,
    capability: Option<&str>,
) -> AppResult<ModelRecord> {
    let id = require_nonempty(id, "id")?;
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/')
    {
        return Err(AppError::BadRequest(
            "model id may only contain letters, numbers, dash, underscore, dot, or slash".into(),
        ));
    }
    let upstream_model = require_nonempty(upstream_model, "upstream_model")?;
    if get_provider_secret(pool, provider_id).await?.is_none() {
        return Err(AppError::BadRequest("provider not found".into()));
    }
    let capability = normalize_capability(capability.unwrap_or("chat"))?;
    let display_name = display_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&id)
        .to_string();
    sqlx::query(
        "INSERT INTO models (id, provider_id, upstream_model, display_name, capability)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(&id)
    .bind(provider_id)
    .bind(&upstream_model)
    .bind(&display_name)
    .bind(&capability)
    .execute(pool)
    .await?;
    get_model(pool, &id)
        .await?
        .ok_or_else(|| AppError::Internal("model insert failed".into()))
}

pub async fn update_model(
    pool: &PgPool,
    id: &str,
    provider_id: Option<&str>,
    upstream_model: Option<&str>,
    display_name: Option<&str>,
    capability: Option<&str>,
) -> AppResult<ModelRecord> {
    let existing = get_model(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("model not found".into()))?;
    let provider_id = provider_id.unwrap_or(&existing.provider_id);
    if get_provider_secret(pool, provider_id).await?.is_none() {
        return Err(AppError::BadRequest("provider not found".into()));
    }
    let upstream_model = upstream_model
        .map(|v| require_nonempty(v, "upstream_model"))
        .transpose()?
        .unwrap_or(existing.upstream_model);
    let display_name = display_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&existing.display_name)
        .to_string();
    let capability = match capability {
        Some(c) => normalize_capability(c)?,
        None => existing.capability,
    };
    sqlx::query(
        "UPDATE models SET provider_id = $1, upstream_model = $2, display_name = $3, capability = $4 WHERE id = $5",
    )
    .bind(provider_id)
    .bind(&upstream_model)
    .bind(&display_name)
    .bind(&capability)
    .bind(id)
    .execute(pool)
    .await?;
    get_model(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("model not found".into()))
}

pub async fn delete_model(pool: &PgPool, id: &str) -> AppResult<()> {
    let n = sqlx::query("DELETE FROM models WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound("model not found".into()));
    }
    Ok(())
}

pub async fn list_models(pool: &PgPool) -> AppResult<Vec<ModelRecord>> {
    let rows = sqlx::query(
        "SELECT m.id, m.provider_id, p.name AS provider_name, p.kind AS provider_kind,
                m.upstream_model, m.display_name, m.capability, m.created_at
         FROM models m JOIN providers p ON p.id = m.provider_id
         ORDER BY m.id ASC",
    )
    .fetch_all(pool)
    .await?;
    rows.iter().map(model_from_row).collect()
}

pub async fn get_model(pool: &PgPool, id: &str) -> AppResult<Option<ModelRecord>> {
    let row = sqlx::query(
        "SELECT m.id, m.provider_id, p.name AS provider_name, p.kind AS provider_kind,
                m.upstream_model, m.display_name, m.capability, m.created_at
         FROM models m JOIN providers p ON p.id = m.provider_id
         WHERE m.id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(model_from_row).transpose()
}

pub async fn resolve_model(pool: &PgPool, model_id: &str) -> AppResult<ModelRoute> {
    let row = sqlx::query(
        "SELECT m.id, m.upstream_model, m.capability, p.id AS provider_id, p.name, p.kind, p.base_url, p.api_key
         FROM models m JOIN providers p ON p.id = m.provider_id
         WHERE m.id = $1",
    )
    .bind(model_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("unknown model '{model_id}'")))?;
    Ok(ModelRoute {
        id: row.try_get("id")?,
        upstream_model: row.try_get("upstream_model")?,
        capability: row.try_get("capability")?,
        provider: ProviderSecret {
            id: row.try_get("provider_id")?,
            name: row.try_get("name")?,
            kind: row.try_get("kind")?,
            base_url: row.try_get("base_url")?,
            api_key: row.try_get("api_key")?,
        },
    })
}

/// First embedding-capable catalog model, optionally preferring `preferred_id`.
pub async fn resolve_embedding_model(
    pool: &PgPool,
    preferred_id: Option<&str>,
) -> AppResult<ModelRoute> {
    if let Some(id) = preferred_id.filter(|s| !s.is_empty()) {
        let route = resolve_model(pool, id).await?;
        if route.capability != "embedding" {
            tracing::warn!(
                model = %id,
                capability = %route.capability,
                "embedding_model is not marked capability=embedding; using it anyway"
            );
        }
        return Ok(route);
    }
    let row = sqlx::query(
        "SELECT m.id, m.upstream_model, m.capability, p.id AS provider_id, p.name, p.kind, p.base_url, p.api_key
         FROM models m JOIN providers p ON p.id = m.provider_id
         WHERE m.capability = 'embedding'
         ORDER BY m.id ASC
         LIMIT 1",
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| {
        AppError::BadRequest(
            "no embedding model configured: add a catalog model with capability=embedding or set EMBEDDING_MODEL"
                .into(),
        )
    })?;
    Ok(ModelRoute {
        id: row.try_get("id")?,
        upstream_model: row.try_get("upstream_model")?,
        capability: row.try_get("capability")?,
        provider: ProviderSecret {
            id: row.try_get("provider_id")?,
            name: row.try_get("name")?,
            kind: row.try_get("kind")?,
            base_url: row.try_get("base_url")?,
            api_key: row.try_get("api_key")?,
        },
    })
}

pub async fn counts(pool: &PgPool) -> AppResult<(i64, i64, i64)> {
    let keys: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_keys WHERE revoked_at IS NULL")
        .fetch_one(pool)
        .await?;
    let providers: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM providers")
        .fetch_one(pool)
        .await?;
    let models: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM models")
        .fetch_one(pool)
        .await?;
    Ok((keys, providers, models))
}

pub async fn model_count(pool: &PgPool) -> AppResult<i64> {
    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM models")
        .fetch_one(pool)
        .await?)
}

/// Fields written to the bounded `request_logs` table.
pub struct RequestLogWrite<'a> {
    pub key_id: Option<&'a str>,
    pub protocol: &'a str,
    pub model: Option<&'a str>,
    pub upstream_model: Option<&'a str>,
    pub status: i32,
    pub latency_ms: i32,
    pub prompt_tokens: Option<i32>,
    pub completion_tokens: Option<i32>,
    pub error: Option<&'a str>,
}

pub async fn insert_request_log(
    pool: &PgPool,
    entry: RequestLogWrite<'_>,
    limit: i64,
) -> AppResult<()> {
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO request_logs
         (id, key_id, protocol, model, upstream_model, status, latency_ms, prompt_tokens, completion_tokens, error)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(&id)
    .bind(entry.key_id)
    .bind(entry.protocol)
    .bind(entry.model)
    .bind(entry.upstream_model)
    .bind(entry.status)
    .bind(entry.latency_ms)
    .bind(entry.prompt_tokens)
    .bind(entry.completion_tokens)
    .bind(entry.error)
    .execute(pool)
    .await?;

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM request_logs")
        .fetch_one(pool)
        .await?;
    if count > limit {
        let extra = (count - limit).min(1000);
        sqlx::query(
            "DELETE FROM request_logs WHERE id IN (
                SELECT id FROM request_logs ORDER BY created_at ASC LIMIT $1
            )",
        )
        .bind(extra)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn create_hermes_session(
    pool: &PgPool,
    key_id: &str,
    title: Option<&str>,
) -> AppResult<HermesSession> {
    let id = Uuid::new_v4().to_string();
    let row = sqlx::query(
        "INSERT INTO hermes_sessions (id, key_id, title) VALUES ($1, $2, $3)
         RETURNING id, key_id, title, created_at",
    )
    .bind(&id)
    .bind(key_id)
    .bind(title)
    .fetch_one(pool)
    .await?;
    session_from_row(&row)
}

pub async fn list_hermes_sessions(pool: &PgPool, key_id: &str) -> AppResult<Vec<HermesSession>> {
    let rows = sqlx::query(
        "SELECT id, key_id, title, created_at FROM hermes_sessions
         WHERE key_id = $1 ORDER BY created_at DESC",
    )
    .bind(key_id)
    .fetch_all(pool)
    .await?;
    rows.iter().map(session_from_row).collect()
}

pub async fn store_hermes_memory(
    pool: &PgPool,
    key_id: &str,
    content: &str,
    session_id: Option<&str>,
    agent: Option<&str>,
    metadata: Value,
    embedding: Option<Vec<f32>>,
) -> AppResult<HermesMemory> {
    let content = require_nonempty(content, "content")?;
    if let Some(sid) = session_id {
        let exists: Option<String> = sqlx::query_scalar(
            "SELECT id FROM hermes_sessions WHERE id = $1 AND key_id = $2",
        )
        .bind(sid)
        .bind(key_id)
        .fetch_optional(pool)
        .await?;
        if exists.is_none() {
            return Err(AppError::NotFound("session not found for this API key".into()));
        }
    }
    let vector = match embedding {
        Some(values) => Some(vector_from_values(values)?),
        None => None,
    };
    let id = Uuid::new_v4().to_string();
    let row = sqlx::query(
        "INSERT INTO hermes_memories (id, key_id, session_id, agent, content, embedding, metadata)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, key_id, session_id, agent, content, metadata, created_at",
    )
    .bind(&id)
    .bind(key_id)
    .bind(session_id)
    .bind(agent)
    .bind(&content)
    .bind(vector)
    .bind(metadata)
    .fetch_one(pool)
    .await?;
    let mut memory = memory_from_row(&row)?;
    memory.distance = None;
    Ok(memory)
}

pub async fn list_hermes_memories(
    pool: &PgPool,
    key_id: &str,
    session_id: Option<&str>,
    limit: i64,
) -> AppResult<Vec<HermesMemory>> {
    let limit = limit.clamp(1, 200);
    let rows = if let Some(sid) = session_id {
        sqlx::query(
            "SELECT id, key_id, session_id, agent, content, metadata, created_at
             FROM hermes_memories
             WHERE key_id = $1 AND session_id = $2
             ORDER BY created_at DESC
             LIMIT $3",
        )
        .bind(key_id)
        .bind(sid)
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            "SELECT id, key_id, session_id, agent, content, metadata, created_at
             FROM hermes_memories
             WHERE key_id = $1
             ORDER BY created_at DESC
             LIMIT $2",
        )
        .bind(key_id)
        .bind(limit)
        .fetch_all(pool)
        .await?
    };
    rows.iter().map(memory_from_row).collect()
}

pub async fn search_hermes_memories(
    pool: &PgPool,
    key_id: &str,
    query_text: Option<&str>,
    embedding: Option<Vec<f32>>,
    session_id: Option<&str>,
    agent: Option<&str>,
    limit: i64,
) -> AppResult<Vec<HermesMemory>> {
    let limit = limit.clamp(1, 50);
    if let Some(values) = embedding {
        let vector = vector_from_values(values)?;
        let rows = sqlx::query(
            "SELECT id, key_id, session_id, agent, content, metadata, created_at,
                    (embedding <=> $1) AS distance
             FROM hermes_memories
             WHERE key_id = $2
               AND embedding IS NOT NULL
               AND ($3::text IS NULL OR session_id = $3)
               AND ($4::text IS NULL OR agent = $4)
             ORDER BY embedding <=> $1
             LIMIT $5",
        )
        .bind(vector)
        .bind(key_id)
        .bind(session_id)
        .bind(agent)
        .bind(limit)
        .fetch_all(pool)
        .await?;
        return rows
            .iter()
            .map(|row| {
                let mut memory = memory_from_row(row)?;
                memory.distance = row.try_get::<Option<f64>, _>("distance")?;
                Ok(memory)
            })
            .collect();
    }

    let needle = query_text
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            AppError::BadRequest("search requires query text or a 1536-d embedding".into())
        })?;
    let like = format!("%{needle}%");
    let rows = sqlx::query(
        "SELECT id, key_id, session_id, agent, content, metadata, created_at
         FROM hermes_memories
         WHERE key_id = $1
           AND content ILIKE $2
           AND ($3::text IS NULL OR session_id = $3)
           AND ($4::text IS NULL OR agent = $4)
         ORDER BY created_at DESC
         LIMIT $5",
    )
    .bind(key_id)
    .bind(&like)
    .bind(session_id)
    .bind(agent)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    rows.iter().map(memory_from_row).collect()
}

/// Required embedding dimensionality (OpenAI `text-embedding-3-small` default).
pub fn embedding_dim() -> usize {
    EMBEDDING_DIM
}

pub fn vector_from_values(values: Vec<f32>) -> AppResult<Vector> {
    if values.len() != EMBEDDING_DIM {
        return Err(AppError::BadRequest(format!(
            "embedding must have {EMBEDDING_DIM} dimensions (got {})",
            values.len()
        )));
    }
    Ok(Vector::from(values))
}

fn api_key_from_row(row: &sqlx::postgres::PgRow) -> AppResult<ApiKeyRecord> {
    let created: DateTime<Utc> = row.try_get("created_at")?;
    let revoked: Option<DateTime<Utc>> = row.try_get("revoked_at")?;
    Ok(ApiKeyRecord {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        key_prefix: row.try_get("key_prefix")?,
        created_at: ts(created),
        revoked_at: revoked.map(ts),
    })
}

fn provider_from_row(row: &sqlx::postgres::PgRow) -> AppResult<ProviderRecord> {
    let api_key: Option<String> = row.try_get("api_key")?;
    let created: DateTime<Utc> = row.try_get("created_at")?;
    Ok(ProviderRecord {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        kind: row.try_get("kind")?,
        base_url: row.try_get("base_url")?,
        has_api_key: api_key.as_deref().is_some_and(|k| !k.is_empty()),
        created_at: ts(created),
    })
}

fn provider_secret_from_row(row: &sqlx::postgres::PgRow) -> AppResult<ProviderSecret> {
    Ok(ProviderSecret {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        kind: row.try_get("kind")?,
        base_url: row.try_get("base_url")?,
        api_key: row.try_get("api_key")?,
    })
}

fn model_from_row(row: &sqlx::postgres::PgRow) -> AppResult<ModelRecord> {
    let created: DateTime<Utc> = row.try_get("created_at")?;
    Ok(ModelRecord {
        id: row.try_get("id")?,
        provider_id: row.try_get("provider_id")?,
        provider_name: row.try_get("provider_name")?,
        provider_kind: row.try_get("provider_kind")?,
        upstream_model: row.try_get("upstream_model")?,
        display_name: row.try_get("display_name")?,
        capability: row.try_get("capability")?,
        created_at: ts(created),
    })
}

fn session_from_row(row: &sqlx::postgres::PgRow) -> AppResult<HermesSession> {
    let created: DateTime<Utc> = row.try_get("created_at")?;
    Ok(HermesSession {
        id: row.try_get("id")?,
        key_id: row.try_get("key_id")?,
        title: row.try_get("title")?,
        created_at: rfc3339(created),
    })
}

fn memory_from_row(row: &sqlx::postgres::PgRow) -> AppResult<HermesMemory> {
    let created: DateTime<Utc> = row.try_get("created_at")?;
    Ok(HermesMemory {
        id: row.try_get("id")?,
        key_id: row.try_get("key_id")?,
        session_id: row.try_get("session_id")?,
        agent: row.try_get("agent")?,
        content: row.try_get("content")?,
        metadata: row.try_get("metadata")?,
        created_at: rfc3339(created),
        distance: None,
    })
}

fn normalize_kind(kind: &str) -> AppResult<String> {
    match kind.trim().to_ascii_lowercase().as_str() {
        "openai" => Ok("openai".into()),
        "anthropic" => Ok("anthropic".into()),
        "deepseek" => Ok("deepseek".into()),
        "glm" | "zhipu" | "chatglm" => Ok("glm".into()),
        _ => Err(AppError::BadRequest(
            "kind must be openai, anthropic, deepseek, or glm".into(),
        )),
    }
}

fn normalize_capability(capability: &str) -> AppResult<String> {
    match capability.trim().to_ascii_lowercase().as_str() {
        "chat" => Ok("chat".into()),
        "embedding" => Ok("embedding".into()),
        _ => Err(AppError::BadRequest(
            "capability must be 'chat' or 'embedding'".into(),
        )),
    }
}

fn require_nonempty(value: &str, field: &str) -> AppResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest(format!("{field} is required")));
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_aliases() {
        assert_eq!(normalize_kind("Zhipu").unwrap(), "glm");
        assert_eq!(normalize_kind("deepseek").unwrap(), "deepseek");
        assert!(normalize_kind("foo").is_err());
    }

    #[test]
    fn embedding_dim_rejected() {
        assert!(vector_from_values(vec![0.1, 0.2]).is_err());
        assert!(vector_from_values(vec![0.0; EMBEDDING_DIM]).is_ok());
    }

    #[tokio::test]
    async fn hermes_store_list_and_text_search() {
        let Some(url) = crate::test_support::postgres_url().await else {
            eprintln!("skipping postgres hermes test");
            return;
        };
        let pool = connect(&url).await.expect("connect");
        let (key, _plain) = create_api_key(&pool, "hermes-test")
            .await
            .expect("key");
        let session = create_hermes_session(&pool, &key.id, Some("t"))
            .await
            .expect("session");
        store_hermes_memory(
            &pool,
            &key.id,
            "user prefers terse answers",
            Some(&session.id),
            Some("hermes"),
            serde_json::json!({"k": 1}),
            None,
        )
        .await
        .expect("store");
        let listed = list_hermes_memories(&pool, &key.id, Some(&session.id), 10)
            .await
            .expect("list");
        assert_eq!(listed.len(), 1);
        let found = search_hermes_memories(
            &pool,
            &key.id,
            Some("terse"),
            None,
            Some(&session.id),
            Some("hermes"),
            8,
        )
        .await
        .expect("search");
        assert_eq!(found.len(), 1);
        assert!(found[0].content.contains("terse"));
    }
}
