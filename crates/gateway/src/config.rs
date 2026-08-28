//! Runtime configuration from YAML plus environment overrides.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// Top-level gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    /// PostgreSQL connection string. Preferred over the legacy SQLite path.
    #[serde(default)]
    pub database_url: Option<String>,
    /// Legacy field kept so old YAML still parses; ignored at runtime.
    #[serde(default)]
    pub database_path: Option<PathBuf>,
    #[serde(default)]
    pub admin_token: Option<String>,
    #[serde(default = "default_cors")]
    pub cors_origins: Vec<String>,
    /// Catalog model id used when Hermes asks ClosedRouter to embed text.
    #[serde(default)]
    pub embedding_model: Option<String>,
    /// Max retained rows in `request_logs` (oldest deleted in batches).
    #[serde(default = "default_log_limit")]
    pub request_log_limit: i64,
    #[serde(default)]
    pub langfuse: LangfuseConfig,
    #[serde(default)]
    pub providers: Vec<SeedProvider>,
}

/// Optional Langfuse OTLP-less ingestion export.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LangfuseConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub public_key: Option<String>,
    #[serde(default)]
    pub secret_key: Option<String>,
}

/// Provider row seeded on first boot when the catalog is empty.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedProvider {
    pub name: String,
    pub kind: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<SeedModel>,
}

/// Model mapping seeded with a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedModel {
    pub id: String,
    pub upstream_model: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub capability: Option<String>,
}

fn default_host() -> String {
    "0.0.0.0".into()
}

fn default_port() -> u16 {
    8080
}

fn default_log_limit() -> i64 {
    10_000
}

fn default_cors() -> Vec<String> {
    vec![
        "http://localhost:3000".into(),
        "http://127.0.0.1:3000".into(),
        "http://localhost:5173".into(),
        "http://127.0.0.1:5173".into(),
        "http://localhost:5174".into(),
    ]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            database_url: None,
            database_path: None,
            admin_token: None,
            cors_origins: default_cors(),
            embedding_model: None,
            request_log_limit: default_log_limit(),
            langfuse: LangfuseConfig::default(),
            providers: Vec::new(),
        }
    }
}

impl Config {
    /// Load YAML (optional) then overlay environment variables.
    pub fn load(config_path: Option<&Path>) -> Result<Self> {
        let mut config = Config::default();

        if let Some(path) = config_path {
            if path.exists() {
                let raw = fs::read_to_string(path)
                    .with_context(|| format!("failed to read {}", path.display()))?;
                config = serde_yaml::from_str(&raw)
                    .with_context(|| format!("invalid YAML in {}", path.display()))?;
            }
        } else if let Ok(path) = env::var("CLOSEDROUTER_CONFIG") {
            let path = PathBuf::from(path);
            if path.exists() {
                let raw = fs::read_to_string(&path)
                    .with_context(|| format!("failed to read {}", path.display()))?;
                config = serde_yaml::from_str(&raw)
                    .with_context(|| format!("invalid YAML in {}", path.display()))?;
            }
        }

        if let Ok(host) = env::var("HOST").or_else(|_| env::var("GATEWAY_HOST")) {
            if !host.is_empty() {
                config.host = host;
            }
        }
        if let Ok(port) = env::var("PORT").or_else(|_| env::var("GATEWAY_PORT")) {
            if let Ok(parsed) = port.parse::<u16>() {
                config.port = parsed;
            }
        }
        if let Ok(url) = env::var("DATABASE_URL") {
            if !url.is_empty() {
                config.database_url = Some(url);
            }
        }
        if let Ok(token) = env::var("ADMIN_TOKEN") {
            config.admin_token = Some(token);
        }
        if let Ok(origins) = env::var("CORS_ORIGINS") {
            if origins.trim() == "*" {
                config.cors_origins = vec!["*".into()];
            } else if !origins.trim().is_empty() {
                config.cors_origins = origins
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
        if let Ok(model) = env::var("EMBEDDING_MODEL") {
            if !model.is_empty() {
                config.embedding_model = Some(model);
            }
        }
        if let Ok(limit) = env::var("REQUEST_LOG_LIMIT") {
            if let Ok(parsed) = limit.parse::<i64>() {
                config.request_log_limit = parsed.max(100);
            }
        }

        if env_flag("LANGFUSE_ENABLED") {
            config.langfuse.enabled = true;
        }
        if let Ok(host) = env::var("LANGFUSE_HOST") {
            if !host.is_empty() {
                config.langfuse.host = Some(host);
            }
        }
        if let Ok(key) = env::var("LANGFUSE_PUBLIC_KEY") {
            if !key.is_empty() {
                config.langfuse.public_key = Some(key);
            }
        }
        if let Ok(key) = env::var("LANGFUSE_SECRET_KEY") {
            if !key.is_empty() {
                config.langfuse.secret_key = Some(key);
            }
        }

        Ok(config)
    }

    /// PostgreSQL URL, required for boot.
    pub fn database_url(&self) -> Result<&str> {
        self.database_url.as_deref().filter(|s| !s.is_empty()).ok_or_else(|| {
            anyhow::anyhow!(
                "DATABASE_URL is required (PostgreSQL + pgvector). Example: postgres://closedrouter:closedrouter@localhost:5432/closedrouter"
            )
        })
    }

    pub fn bind_addr(&self) -> Result<SocketAddr> {
        let addr = format!("{}:{}", self.host, self.port);
        addr.parse()
            .with_context(|| format!("invalid bind address {addr}"))
    }

    pub fn langfuse_enabled(&self) -> bool {
        self.langfuse.enabled
            && self
                .langfuse
                .host
                .as_deref()
                .map(|s| !s.is_empty())
                .unwrap_or(false)
            && self
                .langfuse
                .public_key
                .as_deref()
                .map(|s| !s.is_empty())
                .unwrap_or(false)
            && self
                .langfuse
                .secret_key
                .as_deref()
                .map(|s| !s.is_empty())
                .unwrap_or(false)
    }
}

fn env_flag(name: &str) -> bool {
    env::var(name)
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}
