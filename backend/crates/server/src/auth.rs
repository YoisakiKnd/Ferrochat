use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    body::Body,
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, request::Parts, StatusCode},
    response::Response,
};
use ferrochat_core::{permissions, AppError};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::App;

#[derive(Clone)]
pub struct Keys {
    pub encoding: EncodingKey,
    pub decoding: DecodingKey,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    id: String,
    exp: usize,
}

#[derive(Clone, Debug)]
pub struct CurrentUser {
    pub id: String,
    pub email: String,
    pub name: String,
    pub role: String,
    pub profile_image_url: String,
}

impl Keys {
    pub fn token(&self, user_id: &str) -> Result<String, AppError> {
        let exp = ferrochat_core::now() as usize + 60 * 60 * 24 * 14;
        encode(
            &Header::default(),
            &Claims {
                id: user_id.to_string(),
                exp,
            },
            &self.encoding,
        )
        .map_err(|e| AppError::Internal(e.to_string()))
    }

    pub fn user_id(&self, token: &str) -> Result<String, AppError> {
        decode::<Claims>(token, &self.decoding, &Validation::default())
            .map(|d| d.claims.id)
            .map_err(|_| AppError::Unauthorized("invalid token".into()))
    }
}

pub fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::encode_b64(uuid::Uuid::new_v4().as_bytes())
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(e.to_string()))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .ok()
        .and_then(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .ok()
        })
        .is_some()
}

pub fn session_json(keys: &Keys, user: &CurrentUser) -> Result<Value, AppError> {
    let token = keys.token(&user.id)?;
    Ok(json!({
        "token": token,
        "token_type": "Bearer",
        "expires_at": ferrochat_core::now() + 60 * 60 * 24 * 14,
        "id": user.id,
        "email": user.email,
        "name": user.name,
        "role": user.role,
        "profile_image_url": user.profile_image_url,
        "permissions": permissions(),
    }))
}

impl FromRequestParts<Arc<App>> for CurrentUser {
    type Rejection = Response;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<App>,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|s| s.to_string());
        let cookie = parts
            .headers
            .get("cookie")
            .and_then(|v| v.to_str().ok())
            .and_then(|c| {
                c.split(';').find_map(|p| {
                    p.trim()
                        .strip_prefix("ferrochat_token=")
                        .map(|s| s.to_string())
                })
            });
        let token = header
            .or(cookie)
            .ok_or_else(|| err(AppError::Unauthorized("not authenticated".into())))?;
        let id = state.keys.user_id(&token).map_err(err)?;
        let value = state.db.user_by_id(&id).await.map_err(err)?;
        Ok(CurrentUser {
            id: value["id"].as_str().unwrap_or_default().to_string(),
            email: value["email"].as_str().unwrap_or_default().to_string(),
            name: value["name"].as_str().unwrap_or_default().to_string(),
            role: value["role"].as_str().unwrap_or("user").to_string(),
            profile_image_url: value["profile_image_url"]
                .as_str()
                .unwrap_or("/user.png")
                .to_string(),
        })
    }
}

pub fn err(error: AppError) -> Response {
    Response::builder()
        .status(StatusCode::from_u16(error.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))
        .header("content-type", "application/json")
        .body(Body::from(error.json().to_string()))
        .unwrap()
}
