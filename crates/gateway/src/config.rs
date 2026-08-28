use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_database_path")]
    pub database_path: PathBuf,
    #[serde(default)]
    pub admin_token: Option<String>,
    #[serde(default = "default_cors")]
    pub cors_origins: Vec<String>,
    #[serde(default)]
    pub providers: Vec<SeedProvider>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedModel {
    pub id: String,
    pub upstream_model: String,
    #[serde(default)]
    pub display_name: Option<String>,
}

fn default_host() -> String {
    "0.0.0.0".into()
}

fn default_port() -> u16 {
    8080
}

fn default_database_path() -> PathBuf {
    PathBuf::from("./data/closedrouter.db")
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
            database_path: default_database_path(),
            admin_token: None,
            cors_origins: default_cors(),
            providers: Vec::new(),
        }
    }
}

impl Config {
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
        if let Ok(db) = env::var("DATABASE_PATH") {
            if !db.is_empty() {
                config.database_path = PathBuf::from(db);
            }
        }
        if let Ok(token) = env::var("ADMIN_TOKEN") {
            if !token.is_empty() {
                config.admin_token = Some(token);
            }
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

        Ok(config)
    }

    pub fn bind_addr(&self) -> Result<SocketAddr> {
        let addr = format!("{}:{}", self.host, self.port);
        addr.parse()
            .with_context(|| format!("invalid bind address {addr}"))
    }
}
