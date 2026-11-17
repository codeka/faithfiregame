use serde::{Deserialize, Serialize};

use crate::character::CompactCharacterAppearance;
use crate::world::AreaKind;

pub const PROTOCOL_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientMessage {
    Handshake {
        protocol_version: u32,
        session_token: String,
    },
    Ping(u64),
    MoveTo {
        x: f32,
        z: f32,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerSnapshot {
    pub player_id: u64,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub moving: bool,
    pub appearance: CompactCharacterAppearance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServerMessage {
    HandshakeAccepted {
        player_id: u64,
    },
    VersionMismatch {
        server_version: u32,
    },
    Unauthorized,
    Pong(u64),
    EnterArea {
        area_id: String,
        area_name: String,
        area_kind: AreaKind,
        dimensions: Option<(u32, u32)>,
        pos: [f32; 3],
        yaw: f32,
        appearance: CompactCharacterAppearance,
        fresh_cryo_wake: bool,
    },
    WorldSnapshot {
        players: Vec<PlayerSnapshot>,
    },
    TileUpdated {
        area_id: String,
        tile_x: i32,
        tile_y: i32,
    },
    AreaUpdated {
        area_id: String,
    },
}

#[cfg(not(feature = "json-wire"))]
pub fn encode_client_msg(msg: &ClientMessage) -> Result<Vec<u8>, String> {
    postcard::to_allocvec(msg).map_err(|e| e.to_string())
}

#[cfg(not(feature = "json-wire"))]
pub fn decode_client_msg(bytes: &[u8]) -> Result<ClientMessage, String> {
    postcard::from_bytes(bytes).map_err(|e| e.to_string())
}

#[cfg(not(feature = "json-wire"))]
pub fn encode_server_msg(msg: &ServerMessage) -> Result<Vec<u8>, String> {
    postcard::to_allocvec(msg).map_err(|e| e.to_string())
}

#[cfg(not(feature = "json-wire"))]
pub fn decode_server_msg(bytes: &[u8]) -> Result<ServerMessage, String> {
    postcard::from_bytes(bytes).map_err(|e| e.to_string())
}

#[cfg(feature = "json-wire")]
pub fn encode_client_msg(msg: &ClientMessage) -> Result<Vec<u8>, String> {
    serde_json::to_vec(msg).map_err(|e| e.to_string())
}

#[cfg(feature = "json-wire")]
pub fn decode_client_msg(bytes: &[u8]) -> Result<ClientMessage, String> {
    serde_json::from_slice(bytes).map_err(|e| e.to_string())
}

#[cfg(feature = "json-wire")]
pub fn encode_server_msg(msg: &ServerMessage) -> Result<Vec<u8>, String> {
    serde_json::to_vec(msg).map_err(|e| e.to_string())
}

#[cfg(feature = "json-wire")]
pub fn decode_server_msg(bytes: &[u8]) -> Result<ServerMessage, String> {
    serde_json::from_slice(bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::CharacterPalette;
    use crate::world::InsideKind;

    #[test]
    fn protocol_roundtrip() {
        let c = ClientMessage::Handshake {
            protocol_version: PROTOCOL_VERSION,
            session_token: "dev-token".to_owned(),
        };
        let bytes = encode_client_msg(&c).unwrap();
        assert_eq!(decode_client_msg(&bytes).unwrap(), c);

        let s = ServerMessage::TileUpdated {
            area_id: "main".to_owned(),
            tile_x: 0,
            tile_y: 0,
        };
        let sbytes = encode_server_msg(&s).unwrap();
        assert_eq!(decode_server_msg(&sbytes).unwrap(), s);

        let appearance = CompactCharacterAppearance {
            skeleton_idx: 0,
            show_base_mesh: true,
            skinned_part_indices: vec![(0, 0)],
            socket_part_indices: vec![(0, 7)],
            palette: CharacterPalette::default(),
        };
        let enter = ServerMessage::EnterArea {
            area_id: "cryo_prison".to_owned(),
            area_name: "Cryo Prison".to_owned(),
            area_kind: AreaKind::Inside(InsideKind::Ephemeral),
            dimensions: Some((24, 20)),
            pos: [7.4, 0.0, 7.4],
            yaw: 1.5,
            appearance: appearance.clone(),
            fresh_cryo_wake: true,
        };
        let ebytes = encode_server_msg(&enter).unwrap();
        assert_eq!(decode_server_msg(&ebytes).unwrap(), enter);

        let snap = ServerMessage::WorldSnapshot {
            players: vec![PlayerSnapshot {
                player_id: 7,
                pos: [7.4, 0.0, 7.4],
                yaw: 1.5,
                moving: false,
                appearance,
            }],
        };
        let snap_bytes = encode_server_msg(&snap).unwrap();
        assert_eq!(decode_server_msg(&snap_bytes).unwrap(), snap);
    }
}
