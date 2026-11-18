use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use shared::protocol::{
    ClientMessage, PROTOCOL_VERSION, ServerMessage, decode_client_msg, encode_server_msg,
};
use tokio::sync::mpsc;
use tracing::{debug, trace, warn};

use crate::db::{CharacterSaveRecord, Storage};
use crate::sim::CRYO_PRISON_AREA_ID;

use super::state::AppState;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws_socket(socket, state))
}

async fn send_server_msg(socket: &mut WebSocket, msg: &ServerMessage) -> bool {
    let Ok(bytes) = encode_server_msg(msg) else {
        return false;
    };
    socket.send(Message::Binary(bytes.into())).await.is_ok()
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
        let _ = send_server_msg(
            &mut socket,
            &ServerMessage::VersionMismatch {
                server_version: PROTOCOL_VERSION,
            },
        )
        .await;
        return;
    }

    let player_id = if cfg!(feature = "editor") && session_token == "editor" {
        0
    } else {
        match state.storage.validate_session(&session_token).await {
            Ok(Some(id)) => id,
            _ => {
                let _ = send_server_msg(&mut socket, &ServerMessage::Unauthorized).await;
                return;
            }
        }
    };

    if !send_server_msg(&mut socket, &ServerMessage::HandshakeAccepted { player_id }).await {
        return;
    }

    let mut rx_broadcast = state.tx_broadcast.subscribe();

    if player_id == 0 {
        loop {
            tokio::select! {
                inbound = socket.recv() => {
                    match inbound {
                        Some(Ok(Message::Binary(bytes))) => {
                            match decode_client_msg(&bytes) {
                                Ok(ClientMessage::Ping(ts)) => {
                                    trace!("recv Ping({ts}) from editor");
                                    if !send_server_msg(&mut socket, &ServerMessage::Pong(ts)).await {
                                        break;
                                    }
                                }
                                Ok(msg) => {
                                    debug!("recv message from editor: {:?}", msg);
                                }
                                Err(e) => {
                                    warn!("invalid editor client message: {e}");
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => break,
                        _ => {}
                    }
                }
                broadcast_msg = rx_broadcast.recv() => {
                    if let Ok(msg) = broadcast_msg {
                        if !send_server_msg(&mut socket, &msg).await {
                            break;
                        }
                    }
                }
            }
        }
        return;
    }

    let saved = state
        .storage
        .load_character_save(player_id)
        .await
        .ok()
        .flatten();
    let (tx_player, mut rx_player) = mpsc::unbounded_channel();
    let (conn_id, enter_msg, snap_msg) = {
        let mut sim = state.sim.write().await;
        sim.spawn_player(player_id, saved, tx_player)
    };

    if !send_server_msg(&mut socket, &enter_msg).await
        || !send_server_msg(&mut socket, &snap_msg).await
    {
        let _ = state.sim.write().await.remove_player(player_id, conn_id);
        return;
    }

    loop {
        tokio::select! {
            inbound = socket.recv() => {
                match inbound {
                    Some(Ok(Message::Binary(bytes))) => {
                        match decode_client_msg(&bytes) {
                            Ok(ClientMessage::Ping(ts)) => {
                                trace!("recv Ping({ts}) from player {player_id}");
                                if !send_server_msg(&mut socket, &ServerMessage::Pong(ts)).await {
                                    break;
                                }
                            }
                            Ok(ClientMessage::MoveTo { x, z }) => {
                                state.sim.write().await.move_player_to(player_id, x, z);
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
            sim_msg = rx_player.recv() => {
                match sim_msg {
                    Some(msg) => {
                        if !send_server_msg(&mut socket, &msg).await {
                            break;
                        }
                    }
                    None => break,
                }
            }
            broadcast_msg = rx_broadcast.recv() => {
                if let Ok(msg) = broadcast_msg {
                    if !send_server_msg(&mut socket, &msg).await {
                        break;
                    }
                }
            }
        }
    }

    if let Some(removed) = state.sim.write().await.remove_player(player_id, conn_id) {
        // Players who disconnect inside cryo_prison restart from scratch; do not persist character state.
        if removed.area_id != CRYO_PRISON_AREA_ID {
            let record = CharacterSaveRecord {
                account_id: removed.account_id,
                area_id: removed.area_id,
                pos: removed.pos,
                yaw: removed.yaw,
                appearance: removed.appearance_def,
            };
            if let Err(err) = state.storage.save_character_state(&record).await {
                warn!("failed to save character state for player {player_id}: {err}");
            }
        }
    }
}
