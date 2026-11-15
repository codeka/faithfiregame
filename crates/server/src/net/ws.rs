use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use shared::protocol::{
    ClientMessage, PROTOCOL_VERSION, ServerMessage, decode_client_msg, encode_server_msg,
};
use tracing::{debug, trace, warn};

use crate::db::Storage;

use super::state::AppState;

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
