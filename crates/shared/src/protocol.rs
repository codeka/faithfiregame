use serde::{Deserialize, Serialize};

use crate::entity::{EntityCapabilities, EntityId, EntityVisual, TriggerConfig};
use crate::world::AreaKind;

pub const PROTOCOL_VERSION: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PopupTone {
    Info,
    Success,
    Warning,
    Danger,
}

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
    Interact {
        entity_id: EntityId,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntitySnapshot {
    pub entity_id: EntityId,
    pub name: Option<String>,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub moving: bool,
    pub capabilities: EntityCapabilities,
    pub visual: Option<EntityVisual>,
    pub trigger: Option<TriggerConfig>,
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
        entity_id: EntityId,
        area_id: String,
        area_name: String,
        area_kind: AreaKind,
        dimensions: Option<(u32, u32)>,
        pos: [f32; 3],
        yaw: f32,
        visual: EntityVisual,
        fresh_cryo_wake: bool,
    },
    WorldSnapshot {
        entities: Vec<EntitySnapshot>,
    },
    TileUpdated {
        area_id: String,
        tile_x: i32,
        tile_y: i32,
    },
    AreaUpdated {
        area_id: String,
    },
    Popup {
        title: String,
        body: String,
        tone: PopupTone,
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
    use crate::character::{CharacterPalette, CompactCharacterAppearance};
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
        let char_visual = EntityVisual::Character(appearance);
        let enter = ServerMessage::EnterArea {
            entity_id: EntityId::new(1),
            area_id: "cryo_prison".to_owned(),
            area_name: "Cryo Prison".to_owned(),
            area_kind: AreaKind::Inside(InsideKind::Ephemeral),
            dimensions: Some((24, 20)),
            pos: [7.4, 0.0, 7.4],
            yaw: 1.5,
            visual: char_visual.clone(),
            fresh_cryo_wake: true,
        };
        let ebytes = encode_server_msg(&enter).unwrap();
        assert_eq!(decode_server_msg(&ebytes).unwrap(), enter);

        let snap = ServerMessage::WorldSnapshot {
            entities: vec![
                EntitySnapshot {
                    entity_id: EntityId::new(1),
                    name: Some("Player".to_owned()),
                    pos: [7.4, 0.0, 7.4],
                    yaw: 1.5,
                    moving: false,
                    capabilities: EntityCapabilities::PLAYER | EntityCapabilities::MOVABLE,
                    visual: Some(char_visual),
                    trigger: None,
                },
                EntitySnapshot {
                    entity_id: EntityId::new(2),
                    name: Some("Cryo Terminal".to_owned()),
                    pos: [9.0, 0.0, 7.5],
                    yaw: 0.0,
                    moving: false,
                    capabilities: EntityCapabilities::INTERACTABLE | EntityCapabilities::TRIGGER,
                    visual: Some(EntityVisual::Prop {
                        asset_id: "local:cryo_terminal".to_owned(),
                        scale: [1.0, 1.0, 1.0],
                        cast_shadow: true,
                    }),
                    trigger: Some(TriggerConfig::default()),
                },
            ],
        };
        let snap_bytes = encode_server_msg(&snap).unwrap();
        assert_eq!(decode_server_msg(&snap_bytes).unwrap(), snap);

        let interact = ClientMessage::Interact {
            entity_id: EntityId::new(2),
        };
        let ibytes = encode_client_msg(&interact).unwrap();
        assert_eq!(decode_client_msg(&ibytes).unwrap(), interact);

        let popup = ServerMessage::Popup {
            title: "Cryo Terminal".to_owned(),
            body: "Welcome back.".to_owned(),
            tone: PopupTone::Success,
        };
        let pbytes = encode_server_msg(&popup).unwrap();
        assert_eq!(decode_server_msg(&pbytes).unwrap(), popup);
    }
}
