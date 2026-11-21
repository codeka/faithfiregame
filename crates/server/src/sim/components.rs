use std::collections::{BTreeSet, VecDeque};

use shared::character::{CharacterDefinition, CompactCharacterAppearance};
use shared::entity::{
    EntityId, EntityVisual, InventoryConfig, InventoryEntry, LootableConfig, ScriptConfig,
    TriggerConfig,
};
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
    #[allow(dead_code)]
    pub fn inside_instance(&self) -> Option<&InsideInstanceId> {
        match self {
            Self::Outside { .. } => None,
            Self::Inside(inst) => Some(inst),
        }
    }
}

/// Records the authored world-file placement that spawned a non-player entity so hot reloads stay synced.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SpawnOrigin {
    OutsideTile {
        area_id: String,
        tx: i32,
        ty: i32,
        placed_id: u64,
    },
    InsideArea {
        instance: InsideInstanceId,
        placed_id: u64,
    },
}

impl SpawnOrigin {
    pub fn placed_id(&self) -> u64 {
        match self {
            Self::OutsideTile { placed_id, .. } | Self::InsideArea { placed_id, .. } => *placed_id,
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
        pose_clip: Option<String>,
    },
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

    pub fn clear_path(&mut self) {
        self.waypoints.clear();
    }
}

/// Oriented box volume that detects player click interactions and/or walk-into entry events.
#[derive(Debug, Clone)]
pub struct TriggerComponent {
    pub config: TriggerConfig,
    pub occupants: BTreeSet<EntityId>,
}

impl TriggerComponent {
    pub fn new(config: TriggerConfig) -> Self {
        Self {
            config,
            occupants: BTreeSet::new(),
        }
    }
}

/// Custom TypeScript/JS trigger handler attached to an entity.
#[derive(Debug, Clone)]
pub struct ScriptComponent {
    pub config: ScriptConfig,
}

impl ScriptComponent {
    pub fn new(config: ScriptConfig) -> Self {
        Self { config }
    }
}

/// Authoritative item storage attached to players, NPCs, corpses, and containers.
#[derive(Debug, Clone, Default)]
pub struct InventoryComponent {
    pub items: Vec<InventoryEntry>,
}

impl InventoryComponent {
    pub fn from_config(cfg: &InventoryConfig) -> Self {
        let items = cfg
            .items
            .iter()
            .filter(|e| !e.item_id.trim().is_empty() && e.count > 0)
            .cloned()
            .collect();
        Self { items }
    }

    pub fn to_config(&self) -> InventoryConfig {
        InventoryConfig {
            items: self
                .items
                .iter()
                .filter(|e| e.count > 0)
                .cloned()
                .collect(),
        }
    }

    pub fn take_item(&mut self, item_id: &str) -> Option<InventoryEntry> {
        let pos = self
            .items
            .iter()
            .position(|e| e.item_id == item_id && e.count > 0)?;
        Some(self.items.remove(pos))
    }

    pub fn take_all(&mut self) -> Vec<InventoryEntry> {
        self.items.drain(..).filter(|e| e.count > 0).collect()
    }

    pub fn give_item(&mut self, item_id: &str, count: u32, max_stack: u32, equip_now: bool) {
        if count == 0 || item_id.trim().is_empty() {
            return;
        }
        let stack_cap = max_stack.max(1);
        let mut remaining = count;
        if equip_now {
            self.items.push(InventoryEntry {
                item_id: item_id.to_owned(),
                count: 1,
                equipped: true,
            });
            remaining -= 1;
        }
        if remaining == 0 {
            return;
        }
        for existing in &mut self.items {
            if !existing.equipped && existing.item_id == item_id && existing.count < stack_cap {
                let add = remaining.min(stack_cap - existing.count);
                existing.count += add;
                remaining -= add;
                if remaining == 0 {
                    return;
                }
            }
        }
        while remaining > 0 {
            let chunk = remaining.min(stack_cap);
            self.items.push(InventoryEntry {
                item_id: item_id.to_owned(),
                count: chunk,
                equipped: false,
            });
            remaining -= chunk;
        }
    }
}

/// Click-to-loot behavior opening an interactive dialog over the entity's inventory.
#[derive(Debug, Clone, Default)]
pub struct LootableComponent {
    pub config: LootableConfig,
}

impl LootableComponent {
    pub fn new(config: LootableConfig) -> Self {
        Self { config }
    }
}

/// Network/session attachment that promotes an entity into a connected player observer.
#[derive(Debug, Clone)]
pub struct PlayerComponent {
    pub account_id: u64,
    pub conn_id: u64,
    pub tx_out: mpsc::UnboundedSender<ServerMessage>,
    pub pending_interaction: Option<EntityId>,
}
