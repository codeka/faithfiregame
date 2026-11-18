use std::collections::VecDeque;

use shared::character::{CharacterDefinition, CompactCharacterAppearance};
use shared::entity::EntityVisual;
use shared::protocol::ServerMessage;
use tokio::sync::mpsc;

pub const CRYO_PRISON_AREA_ID: &str = "cryo_prison";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InsideInstanceId {
    Shared(String),
    Solo {
        area_id: String,
        owner_player_id: u64,
    },
}

impl InsideInstanceId {
    pub fn area_id(&self) -> &str {
        match self {
            Self::Shared(id) | Self::Solo { area_id: id, .. } => id,
        }
    }
}

pub fn inside_instance_for(area_id: &str, player_id: u64) -> InsideInstanceId {
    if area_id == CRYO_PRISON_AREA_ID {
        InsideInstanceId::Solo {
            area_id: area_id.to_owned(),
            owner_player_id: player_id,
        }
    } else {
        InsideInstanceId::Shared(area_id.to_owned())
    }
}

/// Authoritative spatial membership for an entity (Outside grid area or specific Inside instance).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EntityLocation {
    Outside { area_id: String },
    Inside(InsideInstanceId),
}

impl EntityLocation {
    pub fn area_id(&self) -> &str {
        match self {
            Self::Outside { area_id } => area_id,
            Self::Inside(inst) => inst.area_id(),
        }
    }

    #[inline]
    #[allow(dead_code)]
    pub fn is_outside(&self) -> bool {
        matches!(self, Self::Outside { .. })
    }

    #[inline]
    pub fn inside_instance(&self) -> Option<&InsideInstanceId> {
        match self {
            Self::Outside { .. } => None,
            Self::Inside(inst) => Some(inst),
        }
    }
}

/// World-space position and Y-axis heading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformComponent {
    pub pos: [f32; 3],
    pub yaw: f32,
}

/// Visual presentation state replicated to clients via [`EntityVisual`].
#[derive(Debug, Clone)]
pub enum VisualComponent {
    Character {
        definition: CharacterDefinition,
        compact: CompactCharacterAppearance,
    },
    #[allow(dead_code)]
    Prop {
        asset_id: String,
        scale: [f32; 3],
        cast_shadow: bool,
    },
}

impl VisualComponent {
    pub fn to_wire(&self) -> EntityVisual {
        match self {
            Self::Character { compact, .. } => EntityVisual::Character(compact.clone()),
            Self::Prop {
                asset_id,
                scale,
                cast_shadow,
            } => EntityVisual::Prop {
                asset_id: asset_id.clone(),
                scale: *scale,
                cast_shadow: *cast_shadow,
            },
        }
    }
}

/// Grid-pathfinding waypoint queue and movement speed for movable entities.
#[derive(Debug, Clone)]
pub struct MovementComponent {
    pub speed: f32,
    pub waypoints: VecDeque<[f32; 2]>,
}

impl MovementComponent {
    pub fn new(speed: f32) -> Self {
        Self {
            speed,
            waypoints: VecDeque::new(),
        }
    }

    #[inline]
    pub fn is_moving(&self) -> bool {
        !self.waypoints.is_empty()
    }

    #[allow(dead_code)]
    pub fn clear_path(&mut self) {
        self.waypoints.clear();
    }
}

/// Network/session attachment that promotes an entity into a connected player observer.
#[derive(Debug, Clone)]
pub struct PlayerComponent {
    pub account_id: u64,
    pub conn_id: u64,
    pub tx_out: mpsc::UnboundedSender<ServerMessage>,
}
