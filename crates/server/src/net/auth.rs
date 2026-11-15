use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::db::Storage;

use super::state::AppState;

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub player_id: u64,
    pub session_token: String,
}

pub async fn login_handler(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, StatusCode> {
    let clean = req.username.trim();
    if clean.is_empty() || clean.len() > 32 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let (player_id, session_token) = state
        .storage
        .create_dev_session(clean)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(LoginResponse {
        player_id,
        session_token,
    }))
}
