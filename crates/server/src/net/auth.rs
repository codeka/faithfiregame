use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use crate::db::Storage;

use super::state::AppState;

pub const SESSION_COOKIE_NAME: &str = "ff_session";

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct AuthSessionResponse {
    pub player_id: u64,
    pub session_token: String,
}

fn build_session_cookie(token: &str) -> Result<HeaderValue, StatusCode> {
    let raw = format!("{SESSION_COOKIE_NAME}={token}; Path=/; Max-Age=2592000; SameSite=Lax");
    HeaderValue::from_str(&raw).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn clear_session_cookie() -> HeaderValue {
    HeaderValue::from_static("ff_session=; Path=/; Max-Age=0; SameSite=Lax")
}

fn extract_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let header_val = headers.get(header::COOKIE)?.to_str().ok()?;
    for pair in header_val.split(';') {
        let trimmed = pair.trim();
        if let Some((k, v)) = trimmed.split_once('=') {
            if k.trim() == name {
                let val = v.trim();
                if !val.is_empty() {
                    return Some(val.to_owned());
                }
            }
        }
    }
    None
}

pub async fn play_guest_handler(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, StatusCode> {
    let (player_id, session_token) = state
        .storage
        .create_guest_session()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let cookie = build_session_cookie(&session_token)?;
    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(AuthSessionResponse {
            player_id,
            session_token,
        }),
    ))
}

pub async fn login_handler(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let clean = req.username.trim();
    if clean.is_empty() || clean.len() > 32 || req.password.is_empty() || req.password.len() > 128
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let Some((player_id, session_token)) = state
        .storage
        .login_with_password(clean, &req.password)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    let cookie = build_session_cookie(&session_token)?;
    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(AuthSessionResponse {
            player_id,
            session_token,
        }),
    ))
}

pub async fn logout_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    if let Some(token) = extract_cookie(&headers, SESSION_COOKIE_NAME) {
        let _ = state.storage.delete_session(&token).await;
    }
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, clear_session_cookie())],
    ))
}
