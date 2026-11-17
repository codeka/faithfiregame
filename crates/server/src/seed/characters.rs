use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use shared::character::{
    CharacterCatalog, CharacterDefinition, CharacterPalette, CharacterPartDef, PartAttachment,
    SkeletonDef, SkinnedSlotDef, SocketDef,
};

const STARTER_CHARACTER_GLBS: [(&str, &[u8]); 13] = [
    (
        "human_base_body.glb",
        include_bytes!("../../assets/characters/human_base_body.glb"),
    ),
    (
        "head_standard.glb",
        include_bytes!("../../assets/characters/head_standard.glb"),
    ),
    (
        "head_aug_visor.glb",
        include_bytes!("../../assets/characters/head_aug_visor.glb"),
    ),
    (
        "hair_short_crop.glb",
        include_bytes!("../../assets/characters/hair_short_crop.glb"),
    ),
    (
        "hair_ponytail.glb",
        include_bytes!("../../assets/characters/hair_ponytail.glb"),
    ),
    (
        "torso_cryo_jumpsuit.glb",
        include_bytes!("../../assets/characters/torso_cryo_jumpsuit.glb"),
    ),
    (
        "torso_faith_tunic.glb",
        include_bytes!("../../assets/characters/torso_faith_tunic.glb"),
    ),
    (
        "torso_fire_rig.glb",
        include_bytes!("../../assets/characters/torso_fire_rig.glb"),
    ),
    (
        "legs_cargo_pants.glb",
        include_bytes!("../../assets/characters/legs_cargo_pants.glb"),
    ),
    (
        "legs_exo_greaves.glb",
        include_bytes!("../../assets/characters/legs_exo_greaves.glb"),
    ),
    (
        "weapon_guard_rifle.glb",
        include_bytes!("../../assets/characters/weapon_guard_rifle.glb"),
    ),
    (
        "weapon_scrap_blade.glb",
        include_bytes!("../../assets/characters/weapon_scrap_blade.glb"),
    ),
    (
        "pack_salvage_rig.glb",
        include_bytes!("../../assets/characters/pack_salvage_rig.glb"),
    ),
];

fn starter_humanoid_skeleton() -> SkeletonDef {
    SkeletonDef {
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
                id: "socket_chest".to_owned(),
                name: "Chest Socket".to_owned(),
                parent_bone: "spine_02".to_owned(),
                offset: [0.0, 0.06, 0.12],
                rotation_deg: [0.0, 0.0, 0.0],
                scale: 1.0,
            },
            SocketDef {
                id: "socket_back".to_owned(),
                name: "Back Socket".to_owned(),
                parent_bone: "spine_02".to_owned(),
                offset: [0.0, 0.08, -0.12],
                rotation_deg: [0.0, 0.0, 0.0],
                scale: 1.0,
            },
            SocketDef {
                id: "socket_hand_l".to_owned(),
                name: "Left Hand".to_owned(),
                parent_bone: "hand_l".to_owned(),
                offset: [0.0, -0.06, 0.04],
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
    }
}

fn skinned_part(id: &str, name: &str, slot: &str, category: &str, file: &str) -> CharacterPartDef {
    CharacterPartDef {
        id: id.to_owned(),
        name: name.to_owned(),
        skeleton_id: "humanoid".to_owned(),
        attachment: PartAttachment::Skinned {
            slot: slot.to_owned(),
        },
        path: format!("/world/assets/characters/{file}"),
        category: category.to_owned(),
    }
}

fn socket_part(
    id: &str,
    name: &str,
    socket_id: &str,
    category: &str,
    file: &str,
) -> CharacterPartDef {
    CharacterPartDef {
        id: id.to_owned(),
        name: name.to_owned(),
        skeleton_id: "humanoid".to_owned(),
        attachment: PartAttachment::Socket {
            socket_id: socket_id.to_owned(),
            offset: [0.0, 0.0, 0.0],
            rotation_deg: [0.0, 0.0, 0.0],
            scale: 1.0,
        },
        path: format!("/world/assets/characters/{file}"),
        category: category.to_owned(),
    }
}

fn starter_parts() -> Vec<CharacterPartDef> {
    vec![
        skinned_part(
            "head_standard",
            "Standard Head",
            "head",
            "Head",
            "head_standard.glb",
        ),
        skinned_part(
            "head_aug_visor",
            "Augmented Visor Head",
            "head",
            "Head",
            "head_aug_visor.glb",
        ),
        skinned_part(
            "torso_cryo_jumpsuit",
            "Cryo Jumpsuit",
            "torso",
            "Torso",
            "torso_cryo_jumpsuit.glb",
        ),
        skinned_part(
            "torso_faith_tunic",
            "Faith Tunic",
            "torso",
            "Torso",
            "torso_faith_tunic.glb",
        ),
        skinned_part(
            "torso_fire_rig",
            "Fire Exo-Rig",
            "torso",
            "Torso",
            "torso_fire_rig.glb",
        ),
        skinned_part(
            "legs_cargo_pants",
            "Cargo Pants",
            "legs",
            "Legs",
            "legs_cargo_pants.glb",
        ),
        skinned_part(
            "legs_exo_greaves",
            "Exo Greaves",
            "legs",
            "Legs",
            "legs_exo_greaves.glb",
        ),
        socket_part(
            "hair_short_crop",
            "Short Crop Hair",
            "socket_head",
            "Hair",
            "hair_short_crop.glb",
        ),
        socket_part(
            "hair_ponytail",
            "Ponytail Hair",
            "socket_head",
            "Hair",
            "hair_ponytail.glb",
        ),
        socket_part(
            "weapon_guard_rifle",
            "Guard Carbine",
            "socket_hand_r",
            "Weapons",
            "weapon_guard_rifle.glb",
        ),
        socket_part(
            "weapon_scrap_blade",
            "Scrap Cleaver",
            "socket_hand_r",
            "Weapons",
            "weapon_scrap_blade.glb",
        ),
        socket_part(
            "pack_salvage_rig",
            "Salvage Rig Pack",
            "socket_back",
            "Back",
            "pack_salvage_rig.glb",
        ),
    ]
}

fn starter_presets() -> Vec<CharacterDefinition> {
    let cryo = {
        let mut skinned = BTreeMap::new();
        skinned.insert("head".to_owned(), "head_standard".to_owned());
        skinned.insert("torso".to_owned(), "torso_cryo_jumpsuit".to_owned());
        skinned.insert("legs".to_owned(), "legs_cargo_pants".to_owned());
        let mut socketed = BTreeMap::new();
        socketed.insert("socket_head".to_owned(), "hair_short_crop".to_owned());
        CharacterDefinition {
            id: "cryo_prisoner".to_owned(),
            name: "Cryo Prisoner".to_owned(),
            skeleton_id: "humanoid".to_owned(),
            show_base_mesh: true,
            skinned_parts: skinned,
            socket_parts: socketed,
            palette: CharacterPalette::default(),
        }
    };

    let faith = {
        let mut skinned = BTreeMap::new();
        skinned.insert("head".to_owned(), "head_standard".to_owned());
        skinned.insert("torso".to_owned(), "torso_faith_tunic".to_owned());
        skinned.insert("legs".to_owned(), "legs_cargo_pants".to_owned());
        let mut socketed = BTreeMap::new();
        socketed.insert("socket_head".to_owned(), "hair_ponytail".to_owned());
        CharacterDefinition {
            id: "faith_villager".to_owned(),
            name: "Faith Villager".to_owned(),
            skeleton_id: "humanoid".to_owned(),
            show_base_mesh: true,
            skinned_parts: skinned,
            socket_parts: socketed,
            palette: CharacterPalette {
                skin_color: Some([214, 168, 134]),
                hair_color: Some([78, 46, 30]),
                primary_color: Some([206, 194, 166]),
                secondary_color: Some([84, 72, 58]),
                accent_color: Some([56, 124, 168]),
                ..Default::default()
            },
        }
    };

    let fire = {
        let mut skinned = BTreeMap::new();
        skinned.insert("head".to_owned(), "head_aug_visor".to_owned());
        skinned.insert("torso".to_owned(), "torso_fire_rig".to_owned());
        skinned.insert("legs".to_owned(), "legs_exo_greaves".to_owned());
        let mut socketed = BTreeMap::new();
        socketed.insert("socket_hand_r".to_owned(), "weapon_guard_rifle".to_owned());
        socketed.insert("socket_back".to_owned(), "pack_salvage_rig".to_owned());
        CharacterDefinition {
            id: "fire_raider".to_owned(),
            name: "Fire Raider".to_owned(),
            skeleton_id: "humanoid".to_owned(),
            show_base_mesh: true,
            skinned_parts: skinned,
            socket_parts: socketed,
            palette: CharacterPalette::default(),
        }
    };

    let scavenger = {
        let mut skinned = BTreeMap::new();
        skinned.insert("head".to_owned(), "head_standard".to_owned());
        skinned.insert("torso".to_owned(), "torso_cryo_jumpsuit".to_owned());
        skinned.insert("legs".to_owned(), "legs_exo_greaves".to_owned());
        let mut socketed = BTreeMap::new();
        socketed.insert("socket_head".to_owned(), "hair_short_crop".to_owned());
        socketed.insert("socket_hand_r".to_owned(), "weapon_scrap_blade".to_owned());
        socketed.insert("socket_back".to_owned(), "pack_salvage_rig".to_owned());
        CharacterDefinition {
            id: "scrap_scavenger".to_owned(),
            name: "Scrap Scavenger".to_owned(),
            skeleton_id: "humanoid".to_owned(),
            show_base_mesh: true,
            skinned_parts: skinned,
            socket_parts: socketed,
            palette: CharacterPalette {
                skin_color: Some([176, 132, 102]),
                hair_color: Some([34, 30, 28]),
                primary_color: Some([104, 110, 82]),
                secondary_color: Some([62, 58, 52]),
                accent_color: Some([184, 96, 44]),
                ..Default::default()
            },
        }
    };

    vec![cryo, faith, fire, scavenger]
}

/// Ensures starter character `.glb` assets, `world/characters/catalog.json`, and starter presets exist.
pub fn seed_characters(world_dir: &Path) -> anyhow::Result<()> {
    let char_assets_dir = world_dir.join("assets").join("characters");
    fs::create_dir_all(&char_assets_dir)?;

    for (filename, bytes) in STARTER_CHARACTER_GLBS {
        let dest = char_assets_dir.join(filename);
        if !dest.exists() {
            fs::write(&dest, bytes)?;
        }
    }

    let chars_dir = world_dir.join("characters");
    let presets_dir = chars_dir.join("presets");
    fs::create_dir_all(&presets_dir)?;

    let catalog_path = chars_dir.join("catalog.json");
    let starter_skel = starter_humanoid_skeleton();
    let starter_part_list = starter_parts();

    if catalog_path.exists() {
        let raw = fs::read(&catalog_path)?;
        let mut catalog: CharacterCatalog = serde_json::from_slice(&raw)?;
        let mut modified = false;

        if !catalog.skeletons.iter().any(|s| s.id == starter_skel.id) {
            catalog.skeletons.push(starter_skel);
            modified = true;
        }
        for part in starter_part_list {
            if !catalog.parts.iter().any(|p| p.id == part.id) {
                catalog.parts.push(part);
                modified = true;
            }
        }
        if modified {
            fs::write(&catalog_path, serde_json::to_vec_pretty(&catalog)?)?;
        }
    } else {
        let catalog = CharacterCatalog {
            skeletons: vec![starter_skel],
            parts: starter_part_list,
        };
        fs::write(&catalog_path, serde_json::to_vec_pretty(&catalog)?)?;
    }

    for preset in starter_presets() {
        let path = presets_dir.join(format!("{}.json", preset.id));
        if !path.exists() {
            fs::write(&path, serde_json::to_vec_pretty(&preset)?)?;
        }
    }

    Ok(())
}
