use shared::entity::{EntityId, PropertyValue};
use shared::protocol::PopupTone;

use super::components::EntityLocation;

/// How a trigger entity was activated by an actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerWay {
    Click,
    WalkInto,
}

/// Deferred intent commands queued against [`SimWorld`](super::world::SimWorld) to avoid borrow conflicts.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum SimCommand {
    MoveTo {
        entity_id: EntityId,
        target_x: f32,
        target_z: f32,
    },
    Interact {
        actor_id: EntityId,
        target_id: EntityId,
    },
    LootTake {
        actor_id: EntityId,
        target_id: EntityId,
        item_id: String,
    },
    LootTakeAll {
        actor_id: EntityId,
        target_id: EntityId,
    },
    StopMovement {
        entity_id: EntityId,
    },
    Teleport {
        entity_id: EntityId,
        location: EntityLocation,
        pos: [f32; 3],
        yaw: Option<f32>,
    },
    SetProperty {
        entity_id: EntityId,
        key: String,
        value: PropertyValue,
    },
    RemoveProperty {
        entity_id: EntityId,
        key: String,
    },
    TransferToArea {
        entity_id: EntityId,
        area_id: String,
        pos: [f32; 3],
        yaw: Option<f32>,
    },
    TeleportToAreaTarget {
        entity_id: EntityId,
        area_id: String,
        target_name: String,
    },
    ShowPopup {
        player_entity_id: EntityId,
        title: String,
        body: String,
        tone: PopupTone,
    },
    Despawn {
        entity_id: EntityId,
    },
}

/// Domain events produced while applying commands and ticking entities.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum SimEvent {
    EntitySpawned {
        entity_id: EntityId,
    },
    EntityDespawned {
        entity_id: EntityId,
    },
    EntityMoved {
        entity_id: EntityId,
        from: [f32; 3],
        to: [f32; 3],
    },
    EntityArrived {
        entity_id: EntityId,
        pos: [f32; 3],
    },
    EntityTriggered {
        trigger_entity_id: EntityId,
        actor_entity_id: EntityId,
        way: TriggerWay,
    },
    ScriptError {
        entity_id: EntityId,
        message: String,
    },
}
