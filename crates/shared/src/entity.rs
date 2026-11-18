use std::collections::BTreeMap;
use std::fmt;
use std::ops::{BitOr, BitOrAssign};

use serde::{Deserialize, Serialize};

use crate::character::CompactCharacterAppearance;

/// Unique runtime identifier assigned to every active entity in a `SimWorld`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EntityId(pub u64);

impl EntityId {
    #[inline]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    #[inline]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Bitflag capability summary replicated to clients so UI/picking know what actions an entity supports.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityCapabilities(pub u16);

impl EntityCapabilities {
    pub const NONE: Self = Self(0);
    pub const PLAYER: Self = Self(1 << 0);
    pub const MOVABLE: Self = Self(1 << 1);
    pub const INTERACTABLE: Self = Self(1 << 2);
    pub const ATTACKABLE: Self = Self(1 << 3);
    pub const QUEST_GIVER: Self = Self(1 << 4);
    pub const VENDOR: Self = Self(1 << 5);

    #[inline]
    pub const fn contains(self, flag: Self) -> bool {
        (self.0 & flag.0) == flag.0
    }

    #[inline]
    pub fn insert(&mut self, flag: Self) {
        self.0 |= flag.0;
    }

    #[inline]
    pub fn remove(&mut self, flag: Self) {
        self.0 &= !flag.0;
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for EntityCapabilities {
    type Output = Self;

    #[inline]
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for EntityCapabilities {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Client-renderable representation attached to a replicated entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EntityVisual {
    Character(CompactCharacterAppearance),
    Prop {
        asset_id: String,
        scale: [f32; 3],
        cast_shadow: bool,
    },
}

/// Typed attribute stored inside an entity's [`PropertyBag`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PropertyValue {
    Bool(bool),
    Int(i64),
    Float(f32),
    String(String),
}

/// Deterministic key-value property container for extensible entity traits and authoring data.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PropertyBag(pub BTreeMap<String, PropertyValue>);

impl PropertyBag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn set(&mut self, key: impl Into<String>, value: PropertyValue) {
        self.0.insert(key.into(), value);
    }

    pub fn remove(&mut self, key: &str) -> Option<PropertyValue> {
        self.0.remove(key)
    }

    pub fn get(&self, key: &str) -> Option<&PropertyValue> {
        self.0.get(key)
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match self.0.get(key) {
            Some(PropertyValue::Bool(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn get_int(&self, key: &str) -> Option<i64> {
        match self.0.get(key) {
            Some(PropertyValue::Int(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn get_float(&self, key: &str) -> Option<f32> {
        match self.0.get(key) {
            Some(PropertyValue::Float(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        match self.0.get(key) {
            Some(PropertyValue::String(v)) => Some(v.as_str()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::CharacterPalette;

    #[test]
    fn entity_capabilities_bitflags_work() {
        let mut caps = EntityCapabilities::PLAYER | EntityCapabilities::MOVABLE;
        assert!(caps.contains(EntityCapabilities::PLAYER));
        assert!(caps.contains(EntityCapabilities::MOVABLE));
        assert!(!caps.contains(EntityCapabilities::ATTACKABLE));
        assert!(!caps.is_empty());

        caps.insert(EntityCapabilities::VENDOR);
        assert!(caps.contains(EntityCapabilities::VENDOR));

        caps.remove(EntityCapabilities::MOVABLE);
        assert!(!caps.contains(EntityCapabilities::MOVABLE));

        let none = EntityCapabilities::NONE;
        assert!(none.is_empty());
    }

    #[test]
    fn property_bag_typed_getters_and_postcard_roundtrip() {
        let mut bag = PropertyBag::new();
        assert!(bag.is_empty());

        bag.set("interactable", PropertyValue::Bool(true));
        bag.set("max_hp", PropertyValue::Int(120));
        bag.set("aggro_radius", PropertyValue::Float(6.5));
        bag.set("dialog_id", PropertyValue::String("cryo_console_01".to_owned()));

        assert_eq!(bag.len(), 4);
        assert_eq!(bag.get_bool("interactable"), Some(true));
        assert_eq!(bag.get_int("max_hp"), Some(120));
        assert_eq!(bag.get_float("aggro_radius"), Some(6.5));
        assert_eq!(bag.get_str("dialog_id"), Some("cryo_console_01"));
        assert_eq!(bag.get_int("interactable"), None);
        assert_eq!(bag.get("missing"), None);

        let removed = bag.remove("max_hp");
        assert_eq!(removed, Some(PropertyValue::Int(120)));
        assert_eq!(bag.len(), 3);

        let bag_bytes = postcard::to_allocvec(&bag).unwrap();
        let decoded_bag: PropertyBag = postcard::from_bytes(&bag_bytes).unwrap();
        assert_eq!(decoded_bag, bag);

        let id = EntityId::new(42);
        assert_eq!(id.raw(), 42);
        assert_eq!(id.to_string(), "42");
        let id_bytes = postcard::to_allocvec(&id).unwrap();
        assert_eq!(postcard::from_bytes::<EntityId>(&id_bytes).unwrap(), id);

        let char_vis = EntityVisual::Character(CompactCharacterAppearance {
            skeleton_idx: 0,
            show_base_mesh: true,
            skinned_part_indices: vec![(0, 1)],
            socket_part_indices: vec![(1, 2)],
            palette: CharacterPalette::default(),
        });
        let char_bytes = postcard::to_allocvec(&char_vis).unwrap();
        assert_eq!(postcard::from_bytes::<EntityVisual>(&char_bytes).unwrap(), char_vis);

        let prop_vis = EntityVisual::Prop {
            asset_id: "local:cryo_terminal".to_owned(),
            scale: [1.0, 1.0, 1.0],
            cast_shadow: true,
        };
        let prop_bytes = postcard::to_allocvec(&prop_vis).unwrap();
        assert_eq!(postcard::from_bytes::<EntityVisual>(&prop_bytes).unwrap(), prop_vis);
    }
}
