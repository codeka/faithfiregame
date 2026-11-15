use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

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
pub enum ServerMessage {
    HandshakeAccepted {
        player_id: u64,
    },
    VersionMismatch {
        server_version: u32,
    },
    Unauthorized,
    Pong(u64),
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
    }
}
