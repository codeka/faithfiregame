use std::collections::BTreeMap;
use std::fmt;
use std::ops::{BitOr, BitOrAssign};

use glam::{Mat4, Quat, Vec3};
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
    pub const TRIGGER: Self = Self(1 << 6);
    pub const SCRIPTED: Self = Self(1 << 7);
    pub const LOOTABLE: Self = Self(1 << 8);

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

/// Single item stack or equipped entry stored inside an entity's inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryEntry {
    pub item_id: String,
    #[serde(default = "default_one_u32")]
    pub count: u32,
    #[serde(default)]
    pub equipped: bool,
}

fn default_one_u32() -> u32 {
    1
}

/// Authored inventory configuration on an entity template, instance override, or player save.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryConfig {
    #[serde(default)]
    pub items: Vec<InventoryEntry>,
}

/// Authored loot-container metadata enabling click-to-loot interaction against an entity's inventory.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LootableConfig {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
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

/// How players can activate an entity's trigger volume.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerActivation {
    #[default]
    Click,
    WalkInto,
    Both,
}

impl TriggerActivation {
    #[inline]
    pub const fn allows_click(self) -> bool {
        matches!(self, Self::Click | Self::Both)
    }

    #[inline]
    pub const fn allows_walk_into(self) -> bool {
        matches!(self, Self::WalkInto | Self::Both)
    }
}

/// Oriented 3D box volume and interaction behavior for a Trigger component.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerConfig {
    /// Box dimensions `[width_x, height_y, depth_z]` in meters (base rests at local `y = 0`).
    pub size: [f32; 3],
    /// Local `[x, y, z]` offset rotated by entity yaw before applying local box yaw.
    pub offset: [f32; 3],
    /// Additional local Y rotation in degrees relative to entity heading (`yaw`).
    pub yaw_deg: f32,
    pub activation: TriggerActivation,
    pub interaction_range: f32,
}

impl Default for TriggerConfig {
    fn default() -> Self {
        Self {
            size: [1.6, 2.0, 1.6],
            offset: [0.0, 0.0, 0.0],
            yaw_deg: 0.0,
            activation: TriggerActivation::Click,
            interaction_range: 2.5,
        }
    }
}

impl TriggerConfig {
    #[inline]
    pub fn clamped_size(&self) -> [f32; 3] {
        [
            self.size[0].max(0.1),
            self.size[1].max(0.1),
            self.size[2].max(0.1),
        ]
    }

    #[inline]
    pub fn local_bounds(&self) -> (Vec3, Vec3) {
        let [w, h, d] = self.clamped_size();
        (
            Vec3::new(-w * 0.5, 0.0, -d * 0.5),
            Vec3::new(w * 0.5, h, d * 0.5),
        )
    }

    pub fn world_matrix(&self, entity_pos: [f32; 3], entity_yaw_rad: f32) -> Mat4 {
        let entity_rot = Quat::from_rotation_y(entity_yaw_rad);
        let center = Vec3::from(entity_pos) + entity_rot * Vec3::from(self.offset);
        let total_rot = Quat::from_rotation_y(entity_yaw_rad + self.yaw_deg.to_radians());
        Mat4::from_rotation_translation(total_rot, center)
    }

    pub fn contains_point(
        &self,
        entity_pos: [f32; 3],
        entity_yaw_rad: f32,
        point: [f32; 3],
    ) -> bool {
        let [w, h, d] = self.clamped_size();
        let local = self
            .world_matrix(entity_pos, entity_yaw_rad)
            .inverse()
            .transform_point3(Vec3::from(point));
        local.x.abs() <= w * 0.5
            && local.z.abs() <= d * 0.5
            && local.y >= -0.35
            && local.y <= h + 0.35
    }

    pub fn horizontal_distance_to_point(
        &self,
        entity_pos: [f32; 3],
        entity_yaw_rad: f32,
        point: [f32; 3],
    ) -> f32 {
        let [w, _, d] = self.clamped_size();
        let local = self
            .world_matrix(entity_pos, entity_yaw_rad)
            .inverse()
            .transform_point3(Vec3::from(point));
        let dx = (local.x.abs() - w * 0.5).max(0.0);
        let dz = (local.z.abs() - d * 0.5).max(0.0);
        (dx * dx + dz * dz).sqrt()
    }

    pub fn raycast(
        &self,
        entity_pos: [f32; 3],
        entity_yaw_rad: f32,
        ray_orig: Vec3,
        ray_dir: Vec3,
    ) -> Option<f32> {
        let inv = self.world_matrix(entity_pos, entity_yaw_rad).inverse();
        let lo = inv.transform_point3(ray_orig);
        let ld = inv.transform_vector3(ray_dir);
        let (bmin, bmax) = self.local_bounds();
        intersect_local_aabb(lo, ld, bmin, bmax)
    }

    /// Returns `GL_LINES` vertex pairs representing the oriented box and local `+Z` heading arrow.
    pub fn wireframe_segments(&self, entity_pos: [f32; 3], entity_yaw_rad: f32) -> Vec<[f32; 3]> {
        let world = self.world_matrix(entity_pos, entity_yaw_rad);
        let (bmin, bmax) = self.local_bounds();
        let [x0, y0, z0] = bmin.to_array();
        let [x1, y1, z1] = bmax.to_array();

        let corners = [
            Vec3::new(x0, y0, z0),
            Vec3::new(x1, y0, z0),
            Vec3::new(x1, y0, z1),
            Vec3::new(x0, y0, z1),
            Vec3::new(x0, y1, z0),
            Vec3::new(x1, y1, z0),
            Vec3::new(x1, y1, z1),
            Vec3::new(x0, y1, z1),
        ];

        const EDGES: [(usize, usize); 12] = [
            (0, 1),
            (1, 2),
            (2, 3),
            (3, 0),
            (4, 5),
            (5, 6),
            (6, 7),
            (7, 4),
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7),
        ];

        let mut pts = Vec::with_capacity(15 * 2);
        let mut push_seg = |a: Vec3, b: Vec3| {
            pts.push(world.transform_point3(a).to_array());
            pts.push(world.transform_point3(b).to_array());
        };

        for (a, b) in EDGES {
            push_seg(corners[a], corners[b]);
        }

        // Local +Z heading arrow just above the box base
        let arrow_y = (y0 + 0.04).min(y1 * 0.5);
        let tip = Vec3::new(0.0, arrow_y, z1 + 0.45);
        push_seg(Vec3::new(0.0, arrow_y, 0.0), tip);
        push_seg(tip, Vec3::new(-0.16, arrow_y, z1 + 0.23));
        push_seg(tip, Vec3::new(0.16, arrow_y, z1 + 0.23));

        pts
    }
}

pub fn intersect_local_aabb(orig: Vec3, dir: Vec3, bmin: Vec3, bmax: Vec3) -> Option<f32> {
    let mut tmin = 0.0f32;
    let mut tmax = f32::MAX;
    for axis in 0..3 {
        let o = orig[axis];
        let d = dir[axis];
        let min_v = bmin[axis];
        let max_v = bmax[axis];
        if d.abs() < 1e-6 {
            if o < min_v || o > max_v {
                return None;
            }
        } else {
            let inv = 1.0 / d;
            let mut t1 = (min_v - o) * inv;
            let mut t2 = (max_v - o) * inv;
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            tmin = tmin.max(t1);
            tmax = tmax.min(t2);
            if tmin > tmax {
                return None;
            }
        }
    }
    Some(tmin)
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
        caps.insert(EntityCapabilities::TRIGGER);
        caps.insert(EntityCapabilities::SCRIPTED);
        assert!(caps.contains(EntityCapabilities::VENDOR));
        assert!(caps.contains(EntityCapabilities::TRIGGER));
        assert!(caps.contains(EntityCapabilities::SCRIPTED));

        caps.remove(EntityCapabilities::MOVABLE);
        assert!(!caps.contains(EntityCapabilities::MOVABLE));

        let none = EntityCapabilities::NONE;
        assert!(none.is_empty());
    }

    #[test]
    fn trigger_activation_flags_match_modes() {
        assert!(TriggerActivation::Click.allows_click());
        assert!(!TriggerActivation::Click.allows_walk_into());
        assert!(!TriggerActivation::WalkInto.allows_click());
        assert!(TriggerActivation::WalkInto.allows_walk_into());
        assert!(TriggerActivation::Both.allows_click());
        assert!(TriggerActivation::Both.allows_walk_into());
    }

    #[test]
    fn trigger_geometry_handles_offset_yaw_raycast_distance_and_wireframe() {
        let trig = TriggerConfig {
            size: [4.0, 2.5, 2.0],
            offset: [0.0, 0.0, 2.0],
            yaw_deg: 0.0,
            activation: TriggerActivation::Both,
            interaction_range: 1.5,
        };
        let entity_pos = [10.0, 0.0, 10.0];
        // Entity rotated +90 deg around Y: local +Z maps to +X, so trigger center sits at [12.0, 0.0, 10.0],
        // with width=4 along Z ([-2..+2] -> Z in [8..12]) and depth=2 along X ([-1..+1] -> X in [11..13]).
        let entity_yaw = std::f32::consts::FRAC_PI_2;

        assert!(trig.contains_point(entity_pos, entity_yaw, [12.0, 0.5, 10.0]));
        assert!(trig.contains_point(entity_pos, entity_yaw, [12.8, 1.0, 11.8]));
        assert!(!trig.contains_point(entity_pos, entity_yaw, [10.2, 0.5, 10.0]));
        assert!(!trig.contains_point(entity_pos, entity_yaw, [12.0, 3.5, 10.0]));

        assert_eq!(
            trig.horizontal_distance_to_point(entity_pos, entity_yaw, [12.0, 0.0, 10.0]),
            0.0
        );
        let dist_outside =
            trig.horizontal_distance_to_point(entity_pos, entity_yaw, [14.0, 0.0, 10.0]);
        assert!((dist_outside - 1.0).abs() < 1e-4, "got {dist_outside}");

        let hit = trig.raycast(
            entity_pos,
            entity_yaw,
            Vec3::new(12.0, 5.0, 10.0),
            Vec3::new(0.0, -1.0, 0.0),
        );
        assert!(hit.is_some_and(|t| (t - 2.5).abs() < 1e-4), "got {hit:?}");
        assert!(
            trig.raycast(
                entity_pos,
                entity_yaw,
                Vec3::new(8.0, 5.0, 10.0),
                Vec3::new(0.0, -1.0, 0.0),
            )
            .is_none()
        );

        let segs = trig.wireframe_segments(entity_pos, entity_yaw);
        assert_eq!(segs.len(), 30);

        let bytes = postcard::to_allocvec(&trig).unwrap();
        let decoded: TriggerConfig = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, trig);
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
            pose_clip: None,
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
