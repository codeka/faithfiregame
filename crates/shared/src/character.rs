use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Maximum number of skeletal joints uploaded per draw call.
/// 48 `mat4` uniforms = 192 `vec4` vertex uniform vectors, safely below WebGL2's minimum 256 limit.
pub const MAX_CHARACTER_JOINTS: usize = 48;

fn default_true() -> bool {
    true
}

fn default_unit_scale() -> f32 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkinnedSlotDef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SocketDef {
    pub id: String,
    pub name: String,
    pub parent_bone: String,
    pub offset: [f32; 3],
    #[serde(default)]
    pub rotation_deg: [f32; 3],
    #[serde(default = "default_unit_scale")]
    pub scale: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkeletonDef {
    pub id: String,
    pub name: String,
    pub rig_path: String,
    #[serde(default = "default_true")]
    pub render_base_mesh: bool,
    #[serde(default)]
    pub skinned_slots: Vec<SkinnedSlotDef>,
    #[serde(default)]
    pub sockets: Vec<SocketDef>,
    #[serde(default)]
    pub animation_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PartAttachment {
    Skinned {
        slot: String,
    },
    Socket {
        socket_id: String,
        #[serde(default)]
        offset: [f32; 3],
        #[serde(default)]
        rotation_deg: [f32; 3],
        #[serde(default = "default_unit_scale")]
        scale: f32,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharacterPartDef {
    pub id: String,
    pub name: String,
    pub skeleton_id: String,
    pub attachment: PartAttachment,
    pub path: String,
    pub category: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CharacterCatalog {
    #[serde(default)]
    pub skeletons: Vec<SkeletonDef>,
    #[serde(default)]
    pub parts: Vec<CharacterPartDef>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterPalette {
    #[serde(default)]
    pub skin_color: Option<[u8; 3]>,
    #[serde(default)]
    pub hair_color: Option<[u8; 3]>,
    #[serde(default)]
    pub eye_color: Option<[u8; 3]>,
    #[serde(default)]
    pub primary_color: Option<[u8; 3]>,
    #[serde(default)]
    pub secondary_color: Option<[u8; 3]>,
    #[serde(default)]
    pub accent_color: Option<[u8; 3]>,
}

impl CharacterPalette {
    /// Resolves an optional `[0.0..1.0]` RGB tint override for semantic glTF material names.
    pub fn override_for_material(&self, material_name: &str) -> Option<[f32; 3]> {
        let rgb = match material_name {
            "mat_skin" => self.skin_color,
            "mat_hair" => self.hair_color,
            "mat_eyes" => self.eye_color,
            "mat_primary" => self.primary_color,
            "mat_secondary" => self.secondary_color,
            "mat_accent" | "mat_trim" => self.accent_color,
            _ => None,
        }?;
        Some([
            rgb[0] as f32 / 255.0,
            rgb[1] as f32 / 255.0,
            rgb[2] as f32 / 255.0,
        ])
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterDefinition {
    pub id: String,
    pub name: String,
    pub skeleton_id: String,
    #[serde(default = "default_true")]
    pub show_base_mesh: bool,
    #[serde(default)]
    pub skinned_parts: BTreeMap<String, String>,
    #[serde(default)]
    pub socket_parts: BTreeMap<String, String>,
    #[serde(default)]
    pub palette: CharacterPalette,
}

impl CharacterDefinition {
    /// Default bare-plus-underwear awakening character spawned in `cryo_prison`.
    pub fn cryo_awakening_default() -> Self {
        let mut skinned_parts = BTreeMap::new();
        skinned_parts.insert("head".to_owned(), "head_standard".to_owned());
        let mut socket_parts = BTreeMap::new();
        socket_parts.insert("socket_head".to_owned(), "hair_short_crop".to_owned());
        Self {
            id: "cryo_awakening".to_owned(),
            name: "Cryo Prisoner".to_owned(),
            skeleton_id: "humanoid".to_owned(),
            show_base_mesh: true,
            skinned_parts,
            socket_parts,
            palette: CharacterPalette::default(),
        }
    }

    /// Encodes this definition into a compact index-based representation using `catalog`.
    pub fn to_compact(&self, catalog: &CharacterCatalog) -> Option<CompactCharacterAppearance> {
        let skel_pos = catalog
            .skeletons
            .iter()
            .position(|s| s.id == self.skeleton_id)?;
        let skeleton_idx = u16::try_from(skel_pos).ok()?;
        let skeleton = &catalog.skeletons[skel_pos];

        let mut skinned_part_indices = Vec::with_capacity(self.skinned_parts.len());
        for (slot_id, part_id) in &self.skinned_parts {
            let slot_pos = skeleton
                .skinned_slots
                .iter()
                .position(|s| s.id == *slot_id)?;
            let part_pos = catalog.parts.iter().position(|p| p.id == *part_id)?;
            skinned_part_indices.push((u8::try_from(slot_pos).ok()?, u16::try_from(part_pos).ok()?));
        }

        let mut socket_part_indices = Vec::with_capacity(self.socket_parts.len());
        for (socket_id, part_id) in &self.socket_parts {
            let sock_pos = skeleton.sockets.iter().position(|s| s.id == *socket_id)?;
            let part_pos = catalog.parts.iter().position(|p| p.id == *part_id)?;
            socket_part_indices.push((u8::try_from(sock_pos).ok()?, u16::try_from(part_pos).ok()?));
        }

        Some(CompactCharacterAppearance {
            skeleton_idx,
            show_base_mesh: self.show_base_mesh,
            skinned_part_indices,
            socket_part_indices,
            palette: self.palette.clone(),
        })
    }
}

/// Compact network representation of a character's equipped modular parts and palette.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactCharacterAppearance {
    pub skeleton_idx: u16,
    pub show_base_mesh: bool,
    pub skinned_part_indices: Vec<(u8, u16)>,
    pub socket_part_indices: Vec<(u8, u16)>,
    pub palette: CharacterPalette,
}

impl CompactCharacterAppearance {
    /// Reconstructs a full `CharacterDefinition` from compact indices via `catalog`.
    pub fn to_definition(
        &self,
        id: &str,
        name: &str,
        catalog: &CharacterCatalog,
    ) -> Option<CharacterDefinition> {
        let skeleton = catalog.skeletons.get(self.skeleton_idx as usize)?;
        let mut skinned_parts = BTreeMap::new();
        for &(slot_idx, part_idx) in &self.skinned_part_indices {
            let slot = skeleton.skinned_slots.get(slot_idx as usize)?;
            let part = catalog.parts.get(part_idx as usize)?;
            skinned_parts.insert(slot.id.clone(), part.id.clone());
        }

        let mut socket_parts = BTreeMap::new();
        for &(sock_idx, part_idx) in &self.socket_part_indices {
            let sock = skeleton.sockets.get(sock_idx as usize)?;
            let part = catalog.parts.get(part_idx as usize)?;
            socket_parts.insert(sock.id.clone(), part.id.clone());
        }

        Some(CharacterDefinition {
            id: id.to_owned(),
            name: name.to_owned(),
            skeleton_id: skeleton.id.clone(),
            show_base_mesh: self.show_base_mesh,
            skinned_parts,
            socket_parts,
            palette: self.palette.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_catalog() -> CharacterCatalog {
        CharacterCatalog {
            skeletons: vec![SkeletonDef {
                id: "humanoid".to_owned(),
                name: "Humanoid".to_owned(),
                rig_path: "/world/assets/characters/human_base_body.glb".to_owned(),
                render_base_mesh: true,
                skinned_slots: vec![
                    SkinnedSlotDef {
                        id: "head".to_owned(),
                        name: "Head".to_owned(),
                    },
                    SkinnedSlotDef {
                        id: "torso".to_owned(),
                        name: "Torso".to_owned(),
                    },
                    SkinnedSlotDef {
                        id: "legs".to_owned(),
                        name: "Legs".to_owned(),
                    },
                ],
                sockets: vec![
                    SocketDef {
                        id: "socket_head".to_owned(),
                        name: "Head Socket".to_owned(),
                        parent_bone: "head".to_owned(),
                        offset: [0.0, 0.0, 0.0],
                        rotation_deg: [0.0, 0.0, 0.0],
                        scale: 1.0,
                    },
                    SocketDef {
                        id: "socket_hand_r".to_owned(),
                        name: "Right Hand".to_owned(),
                        parent_bone: "hand_r".to_owned(),
                        offset: [0.0, -0.06, 0.04],
                        rotation_deg: [0.0, 0.0, 0.0],
                        scale: 1.0,
                    },
                ],
                animation_paths: Vec::new(),
            }],
            parts: vec![
                CharacterPartDef {
                    id: "head_standard".to_owned(),
                    name: "Standard Head".to_owned(),
                    skeleton_id: "humanoid".to_owned(),
                    attachment: PartAttachment::Skinned {
                        slot: "head".to_owned(),
                    },
                    path: "/world/assets/characters/head_standard.glb".to_owned(),
                    category: "Head".to_owned(),
                },
                CharacterPartDef {
                    id: "torso_cryo_jumpsuit".to_owned(),
                    name: "Cryo Jumpsuit".to_owned(),
                    skeleton_id: "humanoid".to_owned(),
                    attachment: PartAttachment::Skinned {
                        slot: "torso".to_owned(),
                    },
                    path: "/world/assets/characters/torso_cryo_jumpsuit.glb".to_owned(),
                    category: "Torso".to_owned(),
                },
                CharacterPartDef {
                    id: "legs_cargo_pants".to_owned(),
                    name: "Cargo Pants".to_owned(),
                    skeleton_id: "humanoid".to_owned(),
                    attachment: PartAttachment::Skinned {
                        slot: "legs".to_owned(),
                    },
                    path: "/world/assets/characters/legs_cargo_pants.glb".to_owned(),
                    category: "Legs".to_owned(),
                },
                CharacterPartDef {
                    id: "hair_short_crop".to_owned(),
                    name: "Short Crop".to_owned(),
                    skeleton_id: "humanoid".to_owned(),
                    attachment: PartAttachment::Socket {
                        socket_id: "socket_head".to_owned(),
                        offset: [0.0, 0.0, 0.0],
                        rotation_deg: [0.0, 0.0, 0.0],
                        scale: 1.0,
                    },
                    path: "/world/assets/characters/hair_short_crop.glb".to_owned(),
                    category: "Hair".to_owned(),
                },
            ],
        }
    }

    #[test]
    fn palette_maps_semantic_material_names() {
        let pal = CharacterPalette {
            skin_color: Some([255, 128, 0]),
            hair_color: Some([51, 102, 153]),
            eye_color: Some([10, 20, 30]),
            primary_color: Some([40, 80, 120]),
            secondary_color: Some([200, 100, 50]),
            accent_color: Some([255, 255, 0]),
        };
        assert_eq!(pal.override_for_material("mat_skin"), Some([1.0, 128.0 / 255.0, 0.0]));
        assert_eq!(
            pal.override_for_material("mat_hair"),
            Some([51.0 / 255.0, 102.0 / 255.0, 153.0 / 255.0])
        );
        assert_eq!(
            pal.override_for_material("mat_eyes"),
            Some([10.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0])
        );
        assert_eq!(
            pal.override_for_material("mat_primary"),
            Some([40.0 / 255.0, 80.0 / 255.0, 120.0 / 255.0])
        );
        assert_eq!(
            pal.override_for_material("mat_secondary"),
            Some([200.0 / 255.0, 100.0 / 255.0, 50.0 / 255.0])
        );
        assert_eq!(pal.override_for_material("mat_accent"), Some([1.0, 1.0, 0.0]));
        assert_eq!(pal.override_for_material("mat_trim"), Some([1.0, 1.0, 0.0]));
        assert_eq!(pal.override_for_material("mat_armor"), None);
    }

    #[test]
    fn compact_appearance_postcard_roundtrip_is_small() {
        let catalog = sample_catalog();
        let mut skinned = BTreeMap::new();
        skinned.insert("head".to_owned(), "head_standard".to_owned());
        skinned.insert("torso".to_owned(), "torso_cryo_jumpsuit".to_owned());
        skinned.insert("legs".to_owned(), "legs_cargo_pants".to_owned());
        let mut socketed = BTreeMap::new();
        socketed.insert("socket_head".to_owned(), "hair_short_crop".to_owned());

        let def = CharacterDefinition {
            id: "cryo_prisoner".to_owned(),
            name: "Cryo Prisoner".to_owned(),
            skeleton_id: "humanoid".to_owned(),
            show_base_mesh: true,
            skinned_parts: skinned,
            socket_parts: socketed,
            palette: CharacterPalette {
                skin_color: Some([199, 153, 122]),
                hair_color: Some([51, 38, 28]),
                ..Default::default()
            },
        };

        let compact = def.to_compact(&catalog).expect("to_compact succeeds");
        let encoded = postcard::to_allocvec(&compact).expect("postcard encode");
        assert!(
            encoded.len() < 32,
            "compact appearance should be < 32 bytes, got {}",
            encoded.len()
        );
        let decoded: CompactCharacterAppearance =
            postcard::from_bytes(&encoded).expect("postcard decode");
        let rebuilt = decoded
            .to_definition("cryo_prisoner", "Cryo Prisoner", &catalog)
            .expect("to_definition succeeds");
        assert_eq!(rebuilt, def);
    }

    #[test]
    fn cryo_awakening_default_roundtrips_without_clothes() {
        let catalog = sample_catalog();
        let def = CharacterDefinition::cryo_awakening_default();
        assert!(def.show_base_mesh);
        assert_eq!(def.skinned_parts.get("head").map(String::as_str), Some("head_standard"));
        assert!(!def.skinned_parts.contains_key("torso"));
        assert!(!def.skinned_parts.contains_key("legs"));
        assert_eq!(
            def.socket_parts.get("socket_head").map(String::as_str),
            Some("hair_short_crop")
        );

        let compact = def.to_compact(&catalog).expect("to_compact succeeds");
        let rebuilt = compact
            .to_definition("cryo_awakening", "Cryo Prisoner", &catalog)
            .expect("to_definition succeeds");
        assert_eq!(rebuilt, def);
    }
}
