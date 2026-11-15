use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use shared::protocol::{
    ClientMessage, PROTOCOL_VERSION, ServerMessage, decode_client_msg, encode_server_msg,
};
use tokio::sync::{RwLock, broadcast};
use tracing::{debug, trace, warn};

use crate::db::{SqliteStorage, Storage};
use crate::sim::SimWorld;

#[derive(Clone)]
pub struct AppState {
    pub world_dir: PathBuf,
    pub storage: Arc<SqliteStorage>,
    #[allow(dead_code)]
    pub sim: Arc<RwLock<SimWorld>>,
    pub tx_broadcast: broadcast::Sender<ServerMessage>,
}

/// Validates that an area ID only contains safe path characters (`[a-zA-Z0-9_-]`).
pub fn is_valid_area_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub fn parse_tile_coord(coord: &str) -> Option<(i32, i32)> {
    let (x_str, y_str) = coord.split_once(',')?;
    let tx = x_str.parse::<i32>().ok()?;
    let ty = y_str.parse::<i32>().ok()?;
    Some((tx, ty))
}

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

pub async fn get_area_bin(
    State(state): State<AppState>,
    Path(area_id): Path<String>,
) -> Result<Vec<u8>, StatusCode> {
    if !is_valid_area_id(&area_id) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let path = state.world_dir.join("areas").join(&area_id).join("area.bin");
    fs::read(path).map_err(|_| StatusCode::NOT_FOUND)
}

pub async fn get_outside_vis_bin(
    State(state): State<AppState>,
    Path((area_id, tile_coord)): Path<(String, String)>,
) -> Result<Vec<u8>, StatusCode> {
    if !is_valid_area_id(&area_id) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let (tx, ty) = parse_tile_coord(&tile_coord).ok_or(StatusCode::BAD_REQUEST)?;
    let path = state
        .world_dir
        .join("areas")
        .join(&area_id)
        .join(format!("{tx},{ty}"))
        .join("vis.bin");
    fs::read(path).map_err(|_| StatusCode::NOT_FOUND)
}

pub async fn get_inside_vis_bin(
    State(state): State<AppState>,
    Path(area_id): Path<String>,
) -> Result<Vec<u8>, StatusCode> {
    if !is_valid_area_id(&area_id) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let path = state.world_dir.join("areas").join(&area_id).join("vis.bin");
    fs::read(path).map_err(|_| StatusCode::NOT_FOUND)
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws_socket(socket, state))
}

async fn handle_ws_socket(mut socket: WebSocket, state: AppState) {
    let first = match socket.recv().await {
        Some(Ok(Message::Binary(bytes))) => decode_client_msg(&bytes),
        Some(Ok(Message::Text(text))) => decode_client_msg(text.as_bytes()),
        _ => return,
    };

    let (protocol_version, session_token) = match first {
        Ok(ClientMessage::Handshake {
            protocol_version,
            session_token,
        }) => (protocol_version, session_token),
        Ok(other) => {
            warn!("unexpected initial WS message: {:?}", other);
            return;
        }
        Err(err) => {
            warn!("failed to decode handshake: {err}");
            return;
        }
    };

    if protocol_version != PROTOCOL_VERSION {
        if let Ok(bytes) = encode_server_msg(&ServerMessage::VersionMismatch {
            server_version: PROTOCOL_VERSION,
        }) {
            let _ = socket.send(Message::Binary(bytes.into())).await;
        }
        return;
    }

    let player_id = if cfg!(feature = "editor") && session_token == "editor" {
        0
    } else {
        match state.storage.validate_session(&session_token).await {
            Ok(Some(id)) => id,
            _ => {
                if let Ok(bytes) = encode_server_msg(&ServerMessage::Unauthorized) {
                    let _ = socket.send(Message::Binary(bytes.into())).await;
                }
                return;
            }
        }
    };

    if let Ok(bytes) = encode_server_msg(&ServerMessage::HandshakeAccepted { player_id }) {
        if socket.send(Message::Binary(bytes.into())).await.is_err() {
            return;
        }
    }

    let mut rx = state.tx_broadcast.subscribe();
    loop {
        tokio::select! {
            inbound = socket.recv() => {
                match inbound {
                    Some(Ok(Message::Binary(bytes))) => {
                        match decode_client_msg(&bytes) {
                            Ok(ClientMessage::Ping(ts)) => {
                                trace!("recv Ping({ts}) from player {player_id}");
                                if let Ok(out) = encode_server_msg(&ServerMessage::Pong(ts)) {
                                    if socket.send(Message::Binary(out.into())).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            Ok(msg) => {
                                debug!("recv message from player {player_id}: {:?}", msg);
                            }
                            Err(e) => {
                                warn!("invalid client message: {e}");
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
            broadcast_msg = rx.recv() => {
                if let Ok(msg) = broadcast_msg {
                    if let Ok(out) = encode_server_msg(&msg) {
                        if socket.send(Message::Binary(out.into())).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    }
}
