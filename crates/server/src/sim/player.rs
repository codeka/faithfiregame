use std::collections::VecDeque;

use shared::character::{CharacterDefinition, CompactCharacterAppearance};
use shared::protocol::ServerMessage;
use shared::world::AreaKind;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct PlayerSimState {
    pub player_id: u64,
    pub conn_id: u64,
    pub area_id: String,
    pub area_kind: AreaKind,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub path_waypoints: VecDeque<[f32; 2]>,
    pub appearance_def: CharacterDefinition,
    pub appearance: CompactCharacterAppearance,
    pub tx_out: mpsc::UnboundedSender<ServerMessage>,
}

impl PlayerSimState {
    #[inline]
    pub fn is_outside(&self) -> bool {
        self.area_kind == AreaKind::Outside
    }
}
