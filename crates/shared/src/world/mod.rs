mod inside;
mod outside;
mod road;
mod splat;
mod types;
mod walk_mask;

pub use inside::*;
pub use outside::*;
pub use road::*;
pub use splat::*;
pub use types::*;
pub use walk_mask::*;

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;

    #[test]
    fn all_world_binary_files_decode_without_error() {
        let world_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("world");
        let areas_dir = world_dir.join("areas");
        if !areas_dir.exists() {
            return;
        }
        for area_entry in fs::read_dir(&areas_dir).unwrap() {
            let area_path = area_entry.unwrap().path();
            if !area_path.is_dir() {
                continue;
            }
            let meta_bytes = fs::read(area_path.join("area.bin")).unwrap();
            let meta: AreaMetadata = postcard::from_bytes(&meta_bytes).unwrap();
            match meta.kind {
                AreaKind::Outside => {
                    for tile_entry in fs::read_dir(&area_path).unwrap() {
                        let tile_path = tile_entry.unwrap().path();
                        if !tile_path.is_dir() {
                            continue;
                        }
                        let vis_p = tile_path.join("vis.bin");
                        if vis_p.exists() {
                            let bytes = fs::read(vis_p).unwrap();
                            TileVisData::from_postcard_bytes(&bytes).unwrap();
                        }
                        let sim_p = tile_path.join("sim.bin");
                        if sim_p.exists() {
                            let bytes = fs::read(sim_p).unwrap();
                            TileSimData::from_postcard_bytes(&bytes).unwrap();
                        }
                    }
                }
                AreaKind::Inside(_) => {
                    let vis_p = area_path.join("vis.bin");
                    if vis_p.exists() {
                        let bytes = fs::read(vis_p).unwrap();
                        InsideVisData::from_postcard_bytes(&bytes).unwrap();
                    }
                    let sim_p = area_path.join("sim.bin");
                    if sim_p.exists() {
                        let bytes = fs::read(sim_p).unwrap();
                        InsideSimData::from_postcard_bytes(&bytes).unwrap();
                    }
                }
            }
        }
    }

    #[cfg(feature = "json-wire")]
    #[test]
    fn all_world_json_catalogs_and_templates_decode_without_error() {
        let world_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("world");
        if !world_dir.exists() {
            return;
        }
        let char_cat_bytes = fs::read(world_dir.join("characters/catalog.json")).unwrap();
        let _: crate::character::CharacterCatalog =
            serde_json::from_slice(&char_cat_bytes).unwrap();

        let item_cat_bytes = fs::read(world_dir.join("items/catalog.json")).unwrap();
        let item_cat: crate::item::ItemCatalog = serde_json::from_slice(&item_cat_bytes).unwrap();
        assert!(!item_cat.items.is_empty());

        for entry in fs::read_dir(world_dir.join("entities/templates")).unwrap() {
            let p = entry.unwrap().path();
            if p.extension().and_then(|s| s.to_str()) == Some("json") {
                let b = fs::read(&p).unwrap();
                let _: crate::entity::EntityTemplate = serde_json::from_slice(&b).unwrap();
            }
        }
        for entry in fs::read_dir(world_dir.join("characters/presets")).unwrap() {
            let p = entry.unwrap().path();
            if p.extension().and_then(|s| s.to_str()) == Some("json") {
                let b = fs::read(&p).unwrap();
                let _: crate::character::CharacterDefinition = serde_json::from_slice(&b).unwrap();
            }
        }
    }
}
