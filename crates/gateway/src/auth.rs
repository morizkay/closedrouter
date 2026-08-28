//! Bearer / `x-api-key` auth for client keys and admin token.

use crate::db;
use crate::error::AppError;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::HeaderMap;

/// Authenticated ClosedRouter API key (`sk-cr-…`).
#[derive(Clone)]
pub struct ApiAuth {
    pub key_id: String,
    pub key_name: String,
}

/// Dashboard / admin token (`ADMIN_TOKEN` or `x-admin-token`).
#[derive(Clone)]
pub struct AdminAuth;

fn bearer_from(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers.get(axum::http::header::AUTHORIZATION) {
        if let Ok(raw) = value.to_str() {
            let raw = raw.trim();
            if let Some(token) = raw
                .strip_prefix("Bearer ")
                .or_else(|| raw.strip_prefix("bearer "))
            {
                let token = token.trim();
                if !token.is_empty() {
                    return Some(token.to_string());
                }
            }
        }
    }
    if let Some(value) = headers.get("x-api-key") {
        if let Ok(raw) = value.to_str() {
            let token = raw.trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

fn admin_token_from(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers.get("x-admin-token") {
        if let Ok(raw) = value.to_str() {
            let token = raw.trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    bearer_from(headers)
}

impl FromRequestParts<crate::AppState> for ApiAuth {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &crate::AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer_from(&parts.headers).ok_or_else(|| {
            AppError::Unauthorized("missing API key (Authorization Bearer or x-api-key)".into())
        })?;
        let key = db::find_valid_key(&state.db, &token)
            .await?
            .ok_or_else(|| AppError::Unauthorized("invalid API key".into()))?;
        Ok(ApiAuth {
            key_id: key.id,
            key_name: key.name,
        })
    }
}

impl FromRequestParts<crate::AppState> for AdminAuth {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &crate::AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = admin_token_from(&parts.headers)
            .ok_or_else(|| AppError::Unauthorized("missing admin token".into()))?;
        if token != state.admin_token {
            return Err(AppError::Unauthorized("invalid admin token".into()));
        }
        Ok(AdminAuth)
    }
}
