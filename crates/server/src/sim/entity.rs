use shared::character::CharacterDefinition;
use shared::entity::{
    EntityCapabilities, EntityId, InventoryConfig, PropertyBag, PropertyValue, TriggerConfig,
};
use shared::protocol::EntitySnapshot;

use super::components::{
    EntityLocation, InventoryComponent, LootableComponent, MovementComponent, PlayerComponent,
    ScriptComponent, SpawnOrigin, TeleportTargetComponent, TransformComponent, TriggerComponent,
    VisualComponent,
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
    pub spawn_origin: Option<SpawnOrigin>,
    pub visual: Option<VisualComponent>,
    pub movement: Option<MovementComponent>,
    pub trigger: Option<TriggerComponent>,
    pub script: Option<ScriptComponent>,
    pub inventory: Option<InventoryComponent>,
    pub lootable: Option<LootableComponent>,
    pub teleport_target: Option<TeleportTargetComponent>,
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
            spawn_origin: None,
            visual: None,
            movement: None,
            trigger: None,
            script: None,
            inventory: None,
            lootable: None,
            teleport_target: None,
            player: None,
        }
    }

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

    pub fn with_spawn_origin(mut self, origin: SpawnOrigin) -> Self {
        self.spawn_origin = Some(origin);
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

    #[allow(dead_code)]
    pub fn with_trigger(mut self, trigger: TriggerComponent) -> Self {
        self.trigger = Some(trigger);
        self
    }

    #[allow(dead_code)]
    pub fn with_script(mut self, script: ScriptComponent) -> Self {
        self.script = Some(script);
        self
    }

    pub fn with_inventory(mut self, inventory: InventoryComponent) -> Self {
        self.inventory = Some(inventory);
        self
    }

    #[allow(dead_code)]
    pub fn with_lootable(mut self, lootable: LootableComponent) -> Self {
        self.lootable = Some(lootable);
        self
    }

    #[allow(dead_code)]
    pub fn with_teleport_target(mut self, tt: TeleportTargetComponent) -> Self {
        self.teleport_target = Some(tt);
        self
    }

    pub fn with_player(mut self, player: PlayerComponent) -> Self {
        self.player = Some(player);
        self
    }

    pub fn effective_click_trigger(&self) -> Option<TriggerConfig> {
        self.trigger
            .as_ref()
            .filter(|t| t.config.activation.allows_click())
            .map(|t| t.config.clone())
            .or_else(|| self.lootable.as_ref().map(|_| TriggerConfig::default()))
    }

    pub fn capabilities(&self) -> EntityCapabilities {
        let mut caps = self.extra_capabilities;
        if self.player.is_some() {
            caps.insert(EntityCapabilities::PLAYER);
        }
        if self.movement.is_some() {
            caps.insert(EntityCapabilities::MOVABLE);
        }
        if let Some(trig) = &self.trigger {
            caps.insert(EntityCapabilities::TRIGGER);
            if trig.config.activation.allows_click() {
                caps.insert(EntityCapabilities::INTERACTABLE);
            }
        }
        if self.script.is_some() {
            caps.insert(EntityCapabilities::SCRIPTED);
        }
        if self.lootable.is_some() {
            caps.insert(EntityCapabilities::LOOTABLE);
            caps.insert(EntityCapabilities::INTERACTABLE);
        }
        caps
    }

    #[inline]
    pub fn is_moving(&self) -> bool {
        self.movement.as_ref().is_some_and(|m| m.is_moving())
    }

    pub fn to_snapshot(&self) -> Option<EntitySnapshot> {
        let click_trigger = self.effective_click_trigger();
        if self.visual.is_none() && click_trigger.is_none() {
            return None;
        }
        Some(EntitySnapshot {
            entity_id: self.id,
            name: self.name.clone(),
            pos: self.transform.pos,
            yaw: self.transform.yaw,
            moving: self.is_moving(),
            capabilities: self.capabilities(),
            visual: self.visual.as_ref().map(|v| v.to_wire()),
            trigger: click_trigger.or_else(|| self.trigger.as_ref().map(|t| t.config.clone())),
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
    pub inventory: InventoryConfig,
}
