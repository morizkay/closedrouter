use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub type Db = Arc<Mutex<Connection>>;

#[derive(Debug, Clone, Serialize)]
pub struct ApiKeyRecord {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub created_at: i64,
    pub revoked_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderRecord {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub has_api_key: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct ProviderSecret {
    #[allow(dead_code)]
    pub id: String,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelRecord {
    pub id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub provider_kind: String,
    pub upstream_model: String,
    pub display_name: String,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct ModelRoute {
    #[allow(dead_code)]
    pub id: String,
    pub upstream_model: String,
    pub provider: ProviderSecret,
}

pub fn open(path: &Path) -> anyhow::Result<Db> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let conn = Connection::open(path)?;
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;
        PRAGMA journal_mode = WAL;
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS api_keys (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            key_prefix TEXT NOT NULL,
            key_hash TEXT NOT NULL UNIQUE,
            created_at INTEGER NOT NULL,
            revoked_at INTEGER
        );
        CREATE TABLE IF NOT EXISTS providers (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            base_url TEXT NOT NULL,
            api_key TEXT,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS models (
            id TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
            upstream_model TEXT NOT NULL,
            display_name TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash);
        "#,
    )?;
    Ok(Arc::new(Mutex::new(conn)))
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn generate_api_key() -> String {
    let mut bytes = [0u8; 24];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut bytes);
    format!("sk-cr-{}", hex::encode(bytes))
}

pub fn generate_admin_token() -> String {
    let mut bytes = [0u8; 24];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut bytes);
    format!("cr-admin-{}", hex::encode(bytes))
}

pub fn get_setting(db: &Db, key: &str) -> AppResult<Option<String>> {
    let conn = db
        .lock()
        .map_err(|_| AppError::Internal("database lock".into()))?;
    let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
    let value = stmt
        .query_row(params![key], |row| row.get::<_, String>(0))
        .ok();
    Ok(value)
}

pub fn set_setting(db: &Db, key: &str, value: &str) -> AppResult<()> {
    let conn = db
        .lock()
        .map_err(|_| AppError::Internal("database lock".into()))?;
    conn.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn create_api_key(db: &Db, name: &str) -> AppResult<(ApiKeyRecord, String)> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest("name is required".into()));
    }
    let plaintext = generate_api_key();
    let record = ApiKeyRecord {
        id: Uuid::new_v4().to_string(),
        name: name.to_string(),
        key_prefix: plaintext.chars().take(12).collect(),
        created_at: now(),
        revoked_at: None,
    };
    let hash = hash_key(&plaintext);
    let conn = db
        .lock()
        .map_err(|_| AppError::Internal("database lock".into()))?;
    conn.execute(
        "INSERT INTO api_keys (id, name, key_prefix, key_hash, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![record.id, record.name, record.key_prefix, hash, record.created_at],
    )?;
    Ok((record, plaintext))
}

pub fn list_api_keys(db: &Db) -> AppResult<Vec<ApiKeyRecord>> {
    let conn = db
        .lock()
        .map_err(|_| AppError::Internal("database lock".into()))?;
    let mut stmt = conn.prepare(
        "SELECT id, name, key_prefix, created_at, revoked_at FROM api_keys ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ApiKeyRecord {
            id: row.get(0)?,
            name: row.get(1)?,
            key_prefix: row.get(2)?,
            created_at: row.get(3)?,
            revoked_at: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn revoke_api_key(db: &Db, id: &str) -> AppResult<()> {
    let conn = db
        .lock()
        .map_err(|_| AppError::Internal("database lock".into()))?;
    let n = conn.execute(
        "UPDATE api_keys SET revoked_at = ?1 WHERE id = ?2 AND revoked_at IS NULL",
        params![now(), id],
    )?;
    if n == 0 {
        return Err(AppError::NotFound("API key not found".into()));
    }
    Ok(())
}

pub fn find_valid_key(db: &Db, bearer: &str) -> AppResult<Option<ApiKeyRecord>> {
    let hash = hash_key(bearer);
    let conn = db
        .lock()
        .map_err(|_| AppError::Internal("database lock".into()))?;
    let mut stmt = conn.prepare(
        "SELECT id, name, key_prefix, created_at, revoked_at FROM api_keys
         WHERE key_hash = ?1 AND revoked_at IS NULL",
    )?;
    let found = stmt
        .query_row(params![hash], |row| {
            Ok(ApiKeyRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                key_prefix: row.get(2)?,
                created_at: row.get(3)?,
                revoked_at: row.get(4)?,
            })
        })
        .ok();
    Ok(found)
}

pub fn create_provider(
    db: &Db,
    name: &str,
    kind: &str,
    base_url: &str,
    api_key: Option<&str>,
) -> AppResult<ProviderRecord> {
    validate_kind(kind)?;
    let name = require_nonempty(name, "name")?;
    let base_url = require_nonempty(base_url, "base_url")?;
    let record = ProviderRecord {
        id: Uuid::new_v4().to_string(),
        name,
        kind: kind.to_string(),
        base_url,
        has_api_key: api_key.map(|k| !k.is_empty()).unwrap_or(false),
        created_at: now(),
    };
    let conn = db
        .lock()
        .map_err(|_| AppError::Internal("database lock".into()))?;
    conn.execute(
        "INSERT INTO providers (id, name, kind, base_url, api_key, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            record.id,
            record.name,
            record.kind,
            record.base_url,
            api_key,
            record.created_at
        ],
    )?;
    Ok(record)
}

pub fn update_provider(
    db: &Db,
    id: &str,
    name: Option<&str>,
    kind: Option<&str>,
    base_url: Option<&str>,
    api_key: Option<&str>,
    clear_api_key: bool,
) -> AppResult<ProviderRecord> {
    if let Some(kind) = kind {
        validate_kind(kind)?;
    }
    let existing = get_provider_secret(db, id)?
        .ok_or_else(|| AppError::NotFound("provider not found".into()))?;
    let name = name
        .map(|v| require_nonempty(v, "name"))
        .transpose()?
        .unwrap_or(existing.name);
    let kind = kind.unwrap_or(&existing.kind).to_string();
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
    let conn = db
        .lock()
        .map_err(|_| AppError::Internal("database lock".into()))?;
    conn.execute(
        "UPDATE providers SET name = ?1, kind = ?2, base_url = ?3, api_key = ?4 WHERE id = ?5",
        params![name, kind, base_url, next_key, id],
    )?;
    drop(conn);
    list_providers(db)?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::NotFound("provider not found".into()))
}

pub fn delete_provider(db: &Db, id: &str) -> AppResult<()> {
    let conn = db
        .lock()
        .map_err(|_| AppError::Internal("database lock".into()))?;
    let n = conn.execute("DELETE FROM providers WHERE id = ?1", params![id])?;
    if n == 0 {
        return Err(AppError::NotFound("provider not found".into()));
    }
    Ok(())
}

pub fn list_providers(db: &Db) -> AppResult<Vec<ProviderRecord>> {
    let conn = db
        .lock()
        .map_err(|_| AppError::Internal("database lock".into()))?;
    let mut stmt = conn.prepare(
        "SELECT id, name, kind, base_url, api_key, created_at FROM providers ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        let api_key: Option<String> = row.get(4)?;
        Ok(ProviderRecord {
            id: row.get(0)?,
            name: row.get(1)?,
            kind: row.get(2)?,
            base_url: row.get(3)?,
            has_api_key: api_key.as_deref().map(|k| !k.is_empty()).unwrap_or(false),
            created_at: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn get_provider_secret(db: &Db, id: &str) -> AppResult<Option<ProviderSecret>> {
    let conn = db
        .lock()
        .map_err(|_| AppError::Internal("database lock".into()))?;
    let mut stmt =
        conn.prepare("SELECT id, name, kind, base_url, api_key FROM providers WHERE id = ?1")?;
    let found = stmt
        .query_row(params![id], |row| {
            Ok(ProviderSecret {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                base_url: row.get(3)?,
                api_key: row.get(4)?,
            })
        })
        .ok();
    Ok(found)
}

pub fn create_model(
    db: &Db,
    id: &str,
    provider_id: &str,
    upstream_model: &str,
    display_name: Option<&str>,
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
    if get_provider_secret(db, provider_id)?.is_none() {
        return Err(AppError::BadRequest("provider not found".into()));
    }
    let display_name = display_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&id)
        .to_string();
    let created_at = now();
    let conn = db
        .lock()
        .map_err(|_| AppError::Internal("database lock".into()))?;
    conn.execute(
        "INSERT INTO models (id, provider_id, upstream_model, display_name, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, provider_id, upstream_model, display_name, created_at],
    )?;
    drop(conn);
    get_model(db, &id)?.ok_or_else(|| AppError::Internal("model insert failed".into()))
}

pub fn update_model(
    db: &Db,
    id: &str,
    provider_id: Option<&str>,
    upstream_model: Option<&str>,
    display_name: Option<&str>,
) -> AppResult<ModelRecord> {
    let existing =
        get_model(db, id)?.ok_or_else(|| AppError::NotFound("model not found".into()))?;
    let provider_id = provider_id.unwrap_or(&existing.provider_id);
    if get_provider_secret(db, provider_id)?.is_none() {
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
    let conn = db
        .lock()
        .map_err(|_| AppError::Internal("database lock".into()))?;
    conn.execute(
        "UPDATE models SET provider_id = ?1, upstream_model = ?2, display_name = ?3 WHERE id = ?4",
        params![provider_id, upstream_model, display_name, id],
    )?;
    drop(conn);
    get_model(db, id)?.ok_or_else(|| AppError::NotFound("model not found".into()))
}

pub fn delete_model(db: &Db, id: &str) -> AppResult<()> {
    let conn = db
        .lock()
        .map_err(|_| AppError::Internal("database lock".into()))?;
    let n = conn.execute("DELETE FROM models WHERE id = ?1", params![id])?;
    if n == 0 {
        return Err(AppError::NotFound("model not found".into()));
    }
    Ok(())
}

pub fn list_models(db: &Db) -> AppResult<Vec<ModelRecord>> {
    let conn = db
        .lock()
        .map_err(|_| AppError::Internal("database lock".into()))?;
    let mut stmt = conn.prepare(
        "SELECT m.id, m.provider_id, p.name, p.kind, m.upstream_model, m.display_name, m.created_at
         FROM models m JOIN providers p ON p.id = m.provider_id
         ORDER BY m.id ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ModelRecord {
            id: row.get(0)?,
            provider_id: row.get(1)?,
            provider_name: row.get(2)?,
            provider_kind: row.get(3)?,
            upstream_model: row.get(4)?,
            display_name: row.get(5)?,
            created_at: row.get(6)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn get_model(db: &Db, id: &str) -> AppResult<Option<ModelRecord>> {
    Ok(list_models(db)?.into_iter().find(|m| m.id == id))
}

pub fn resolve_model(db: &Db, model_id: &str) -> AppResult<ModelRoute> {
    let conn = db
        .lock()
        .map_err(|_| AppError::Internal("database lock".into()))?;
    let mut stmt = conn.prepare(
        "SELECT m.id, m.upstream_model, p.id, p.name, p.kind, p.base_url, p.api_key
         FROM models m JOIN providers p ON p.id = m.provider_id
         WHERE m.id = ?1",
    )?;
    stmt.query_row(params![model_id], |row| {
        Ok(ModelRoute {
            id: row.get(0)?,
            upstream_model: row.get(1)?,
            provider: ProviderSecret {
                id: row.get(2)?,
                name: row.get(3)?,
                kind: row.get(4)?,
                base_url: row.get(5)?,
                api_key: row.get(6)?,
            },
        })
    })
    .map_err(|_| AppError::NotFound(format!("unknown model '{model_id}'")))
}

pub fn counts(db: &Db) -> AppResult<(u32, u32, u32)> {
    let conn = db
        .lock()
        .map_err(|_| AppError::Internal("database lock".into()))?;
    let keys: u32 = conn.query_row(
        "SELECT COUNT(*) FROM api_keys WHERE revoked_at IS NULL",
        [],
        |row| row.get(0),
    )?;
    let providers: u32 = conn.query_row("SELECT COUNT(*) FROM providers", [], |row| row.get(0))?;
    let models: u32 = conn.query_row("SELECT COUNT(*) FROM models", [], |row| row.get(0))?;
    Ok((keys, providers, models))
}

pub fn model_count(db: &Db) -> AppResult<u32> {
    let conn = db
        .lock()
        .map_err(|_| AppError::Internal("database lock".into()))?;
    let n: u32 = conn.query_row("SELECT COUNT(*) FROM models", [], |row| row.get(0))?;
    Ok(n)
}

fn validate_kind(kind: &str) -> AppResult<()> {
    match kind {
        "openai" | "anthropic" => Ok(()),
        _ => Err(AppError::BadRequest(
            "kind must be 'openai' or 'anthropic'".into(),
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
