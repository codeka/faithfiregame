use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::world::prettify_asset_slug;

use super::types::{PropertyBag, TriggerConfig};

/// Authored visual model configuration on an entity template or instance override.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualConfig {
    Character {
        preset_id: String,
    },
    Prop {
        asset_id: String,
        scale: [f32; 3],
        cast_shadow: bool,
    },
}

/// Authored movement configuration on an entity template or instance override.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MovementConfig {
    pub speed: f32,
}

impl Default for MovementConfig {
    fn default() -> Self {
        Self { speed: 3.5 }
    }
}

/// Authored TypeScript trigger script and compiled JS payload on an entity template or override.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptConfig {
    pub source: String,
    #[serde(default)]
    pub compiled_js: String,
}

impl ScriptConfig {
    pub fn effective_js(&self) -> &str {
        if self.compiled_js.trim().is_empty() {
            &self.source
        } else {
            &self.compiled_js
        }
    }
}

impl Default for ScriptConfig {
    fn default() -> Self {
        let source = "function onTrigger(ctx: TriggerContext): void {\n  ctx.showPopup(ctx.entity.name, `Triggered via ${ctx.way}!`);\n}\n".to_owned();
        let compiled_js = "function onTrigger(ctx) {\n  ctx.showPopup(ctx.entity.name, `Triggered via ${ctx.way}!`);\n}\n".to_owned();
        Self {
            source,
            compiled_js,
        }
    }
}

/// Reusable entity archetype authored in Entity Templates mode and saved as JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityTemplate {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub properties: PropertyBag,
    #[serde(default)]
    pub visual: Option<VisualConfig>,
    #[serde(default)]
    pub movement: Option<MovementConfig>,
    #[serde(default)]
    pub trigger: Option<TriggerConfig>,
    #[serde(default)]
    pub script: Option<ScriptConfig>,
}

/// Per-instance override state for an optional component derived from an [`EntityTemplate`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentOverride<T> {
    #[default]
    Inherit,
    Set(T),
    Remove,
}

impl<T> ComponentOverride<T> {
    pub fn resolve<'a>(&'a self, base: Option<&'a T>) -> Option<&'a T> {
        match self {
            Self::Inherit => base,
            Self::Set(val) => Some(val),
            Self::Remove => None,
        }
    }
}

/// Per-instance customizations layered over a referenced [`EntityTemplate`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EntityOverrides {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub properties: PropertyBag,
    #[serde(default)]
    pub removed_properties: BTreeSet<String>,
    #[serde(default)]
    pub visual: ComponentOverride<VisualConfig>,
    #[serde(default)]
    pub movement: ComponentOverride<MovementConfig>,
    #[serde(default)]
    pub trigger: ComponentOverride<TriggerConfig>,
    #[serde(default)]
    pub script: ComponentOverride<ScriptConfig>,
}

impl EntityOverrides {
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.properties.is_empty()
            && self.removed_properties.is_empty()
            && matches!(self.visual, ComponentOverride::Inherit)
            && matches!(self.movement, ComponentOverride::Inherit)
            && matches!(self.trigger, ComponentOverride::Inherit)
            && matches!(self.script, ComponentOverride::Inherit)
    }
}

/// World-placed entity reference stored in tile/inside `sim.bin` and linked to a template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlacedEntity {
    pub id: u64,
    pub template_id: String,
    pub position: [f32; 3],
    pub rotation_y: f32,
    #[serde(default)]
    pub overrides: EntityOverrides,
}

/// Effective entity configuration obtained by merging a [`PlacedEntity`] over its [`EntityTemplate`].
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedEntityData {
    pub name: String,
    pub properties: PropertyBag,
    pub visual: Option<VisualConfig>,
    pub movement: Option<MovementConfig>,
    pub trigger: Option<TriggerConfig>,
    pub script: Option<ScriptConfig>,
}

impl PlacedEntity {
    pub fn resolve(&self, template: Option<&EntityTemplate>) -> ResolvedEntityData {
        let base_name = template
            .map(|t| t.name.clone())
            .unwrap_or_else(|| prettify_asset_slug(&self.template_id));
        let name = self.overrides.name.clone().unwrap_or(base_name);

        let mut properties = template
            .map(|t| t.properties.clone())
            .unwrap_or_default();
        for key in &self.overrides.removed_properties {
            properties.remove(key);
        }
        for (key, value) in &self.overrides.properties.0 {
            properties.set(key.clone(), value.clone());
        }

        let visual = self
            .overrides
            .visual
            .resolve(template.and_then(|t| t.visual.as_ref()))
            .cloned();
        let movement = self
            .overrides
            .movement
            .resolve(template.and_then(|t| t.movement.as_ref()))
            .cloned();
        let trigger = self
            .overrides
            .trigger
            .resolve(template.and_then(|t| t.trigger.as_ref()))
            .cloned();
        let script = self
            .overrides
            .script
            .resolve(template.and_then(|t| t.script.as_ref()))
            .cloned();

        ResolvedEntityData {
            name,
            properties,
            visual,
            movement,
            trigger,
            script,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::types::{PropertyValue, TriggerActivation};

    fn sample_template() -> EntityTemplate {
        let mut properties = PropertyBag::new();
        properties.set("faction", PropertyValue::String("faith".to_owned()));
        properties.set("level", PropertyValue::Int(4));
        properties.set("hostile", PropertyValue::Bool(false));

        EntityTemplate {
            id: "village_guard".to_owned(),
            name: "Village Guard".to_owned(),
            properties,
            visual: Some(VisualConfig::Character {
                preset_id: "faith_villager".to_owned(),
            }),
            movement: Some(MovementConfig { speed: 3.5 }),
            trigger: Some(TriggerConfig {
                size: [1.8, 2.2, 1.8],
                offset: [0.0, 0.0, 0.5],
                yaw_deg: 15.0,
                activation: TriggerActivation::Click,
                interaction_range: 2.5,
            }),
            script: Some(ScriptConfig::default()),
        }
    }

    #[test]
    fn placed_entity_inherits_template_edits_and_applies_overrides() {
        let mut tpl = sample_template();
        assert!(tpl.script.as_ref().unwrap().effective_js().contains("ctx.showPopup"));

        let mut placed = PlacedEntity {
            id: 7,
            template_id: tpl.id.clone(),
            position: [12.0, 0.0, 18.0],
            rotation_y: 1.2,
            overrides: EntityOverrides::default(),
        };
        assert!(placed.overrides.is_empty());

        // Inherits template values verbatim initially.
        let r0 = placed.resolve(Some(&tpl));
        assert_eq!(r0.name, "Village Guard");
        assert_eq!(r0.properties.get_int("level"), Some(4));
        assert_eq!(r0.movement.as_ref().map(|m| m.speed), Some(3.5));

        // Template updates propagate automatically to non-overridden fields.
        tpl.name = "Town Watchman".to_owned();
        tpl.properties.set("level", PropertyValue::Int(6));
        tpl.movement = Some(MovementConfig { speed: 4.2 });
        let r1 = placed.resolve(Some(&tpl));
        assert_eq!(r1.name, "Town Watchman");
        assert_eq!(r1.properties.get_int("level"), Some(6));
        assert_eq!(r1.movement.as_ref().map(|m| m.speed), Some(4.2));

        // Instance overrides mask specific fields, remove components/props, and preserve unrelated inherited fields.
        placed.overrides.name = Some("Captain Vance".to_owned());
        placed
            .overrides
            .properties
            .set("level", PropertyValue::Int(12));
        placed.overrides.removed_properties.insert("hostile".to_owned());
        placed.overrides.movement = ComponentOverride::Remove;
        placed.overrides.visual = ComponentOverride::Set(VisualConfig::Prop {
            asset_id: "ruin_stone".to_owned(),
            scale: [1.2, 1.2, 1.2],
            cast_shadow: false,
        });
        assert!(!placed.overrides.is_empty());

        let r2 = placed.resolve(Some(&tpl));
        assert_eq!(r2.name, "Captain Vance");
        assert_eq!(r2.properties.get_str("faction"), Some("faith"));
        assert_eq!(r2.properties.get_int("level"), Some(12));
        assert_eq!(r2.properties.get("hostile"), None);
        assert_eq!(r2.movement, None);
        assert!(matches!(r2.visual, Some(VisualConfig::Prop { .. })));
        assert_eq!(r2.trigger, tpl.trigger);
        assert_eq!(r2.script, tpl.script);

        placed.overrides.script = ComponentOverride::Set(ScriptConfig {
            source: "function onTrigger(ctx: TriggerContext) {}".to_owned(),
            compiled_js: String::new(),
        });
        let r3 = placed.resolve(Some(&tpl));
        assert_eq!(
            r3.script.as_ref().map(|s| s.effective_js()),
            Some("function onTrigger(ctx: TriggerContext) {}")
        );

        // Postcard roundtrip for template and placed entity.
        let tpl_bytes = postcard::to_allocvec(&tpl).unwrap();
        assert_eq!(
            postcard::from_bytes::<EntityTemplate>(&tpl_bytes).unwrap(),
            tpl
        );
        let placed_bytes = postcard::to_allocvec(&placed).unwrap();
        assert_eq!(
            postcard::from_bytes::<PlacedEntity>(&placed_bytes).unwrap(),
            placed
        );
    }

    #[cfg(feature = "json-wire")]
    #[test]
    fn template_and_placed_entity_json_roundtrip() {
        let tpl = sample_template();
        let json = serde_json::to_string(&tpl).unwrap();
        let decoded_tpl: EntityTemplate = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded_tpl, tpl);

        let mut placed = PlacedEntity {
            id: 9,
            template_id: tpl.id.clone(),
            position: [1.0, 2.0, 3.0],
            rotation_y: 0.5,
            overrides: EntityOverrides::default(),
        };
        placed.overrides.trigger = ComponentOverride::Remove;
        placed.overrides.movement = ComponentOverride::Set(MovementConfig { speed: 5.0 });
        let pjson = serde_json::to_string(&placed).unwrap();
        let decoded_placed: PlacedEntity = serde_json::from_str(&pjson).unwrap();
        assert_eq!(decoded_placed, placed);
    }
}
