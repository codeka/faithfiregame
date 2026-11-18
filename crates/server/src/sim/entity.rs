use shared::character::CharacterDefinition;
use shared::entity::{EntityCapabilities, EntityId, PropertyBag, PropertyValue};
use shared::protocol::EntitySnapshot;

use super::components::{
    EntityLocation, MovementComponent, PlayerComponent, TransformComponent, VisualComponent,
};

/// Composable simulation entity composed of spatial state, properties, and optional components.
#[derive(Debug, Clone)]
pub struct Entity {
    pub id: EntityId,
    pub name: Option<String>,
    pub location: EntityLocation,
    pub transform: TransformComponent,
    pub properties: PropertyBag,
    pub extra_capabilities: EntityCapabilities,
    pub visual: Option<VisualComponent>,
    pub movement: Option<MovementComponent>,
    pub player: Option<PlayerComponent>,
}

impl Entity {
    pub fn new(id: EntityId, location: EntityLocation, pos: [f32; 3], yaw: f32) -> Self {
        Self {
            id,
            name: None,
            location,
            transform: TransformComponent { pos, yaw },
            properties: PropertyBag::new(),
            extra_capabilities: EntityCapabilities::NONE,
            visual: None,
            movement: None,
            player: None,
        }
    }

    #[allow(dead_code)]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    #[allow(dead_code)]
    pub fn with_property(mut self, key: impl Into<String>, value: PropertyValue) -> Self {
        self.properties.set(key, value);
        self
    }

    #[allow(dead_code)]
    pub fn with_capabilities(mut self, capabilities: EntityCapabilities) -> Self {
        self.extra_capabilities |= capabilities;
        self
    }

    pub fn with_visual(mut self, visual: VisualComponent) -> Self {
        self.visual = Some(visual);
        self
    }

    pub fn with_movement(mut self, movement: MovementComponent) -> Self {
        self.movement = Some(movement);
        self
    }

    pub fn with_player(mut self, player: PlayerComponent) -> Self {
        self.player = Some(player);
        self
    }

    pub fn capabilities(&self) -> EntityCapabilities {
        let mut caps = self.extra_capabilities;
        if self.player.is_some() {
            caps.insert(EntityCapabilities::PLAYER);
        }
        if self.movement.is_some() {
            caps.insert(EntityCapabilities::MOVABLE);
        }
        caps
    }

    #[inline]
    pub fn is_moving(&self) -> bool {
        self.movement.as_ref().is_some_and(|m| m.is_moving())
    }

    pub fn to_snapshot(&self) -> Option<EntitySnapshot> {
        let visual = self.visual.as_ref()?;
        Some(EntitySnapshot {
            entity_id: self.id,
            name: self.name.clone(),
            pos: self.transform.pos,
            yaw: self.transform.yaw,
            moving: self.is_moving(),
            capabilities: self.capabilities(),
            visual: visual.to_wire(),
        })
    }
}

/// Snapshot of a disconnected player's state returned for database persistence.
#[derive(Debug, Clone)]
pub struct RemovedPlayer {
    #[allow(dead_code)]
    pub entity_id: EntityId,
    pub account_id: u64,
    pub area_id: String,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub appearance_def: CharacterDefinition,
}
