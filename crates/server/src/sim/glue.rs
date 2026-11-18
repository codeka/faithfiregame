use shared::entity::{EntityId, PropertyValue};

use super::components::EntityLocation;

/// Deferred intent commands queued against [`SimWorld`](super::world::SimWorld) to avoid borrow conflicts.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum SimCommand {
    MoveTo {
        entity_id: EntityId,
        target_x: f32,
        target_z: f32,
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
}
