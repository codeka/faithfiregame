use std::cell::RefCell;
use std::rc::Rc;

use shared::world::{
    AssetCatalog, MAT_BRICK, MAT_COBBLESTONE, MAT_DARK_GRASS, MAT_DIRT, MAT_GRASS,
    MAT_LIGHT_GRASS, MAT_PATCHY_GRASS, MAT_ROCK, MAT_SAND, MAX_TERRAIN_MATERIALS,
    TerrainTextureEntry,
};
use wasm_bindgen_futures::spawn_local;
use web_sys::console;

use crate::engine::EngineInner;
use crate::net::http_get_bytes;
use crate::render::{TerrainLayerImage, decode_albedo_png, decode_height_png};

fn default_terrain_texture_entries() -> Vec<TerrainTextureEntry> {
    const SPECS: [(u8, &str, &str, &str); 9] = [
        (MAT_GRASS, "grass", "Grass", "Grass"),
        (MAT_LIGHT_GRASS, "light_grass", "Light Grass", "Grass"),
        (MAT_DARK_GRASS, "dark_grass", "Dark Grass", "Grass"),
        (MAT_PATCHY_GRASS, "patchy_grass", "Patchy Grass", "Grass"),
        (MAT_DIRT, "dirt", "Dirt", "Earth"),
        (MAT_SAND, "sand", "Sand", "Earth"),
        (MAT_ROCK, "rock", "Rock", "Stone"),
        (MAT_COBBLESTONE, "cobblestone", "Cobblestone", "Stone"),
        (MAT_BRICK, "brick", "Brick", "Stone"),
    ];
    SPECS
        .into_iter()
        .map(|(layer, id, name, category)| TerrainTextureEntry {
            layer,
            id: id.to_owned(),
            name: name.to_owned(),
            category: category.to_owned(),
            albedo_path: format!("/world/assets/terrain/{id}.png"),
            height_path: Some(format!("/world/assets/terrain/{id}_height.png")),
        })
        .collect()
}

#[allow(dead_code)]
pub(crate) fn spawn_asset_catalog_load(inner: &Rc<RefCell<EngineInner>>) {
    let weak = Rc::clone(inner);
    spawn_local(async move {
        let mut catalog = match http_get_bytes("/world/assets/catalog.json").await {
            Ok((200, bytes)) => match serde_json::from_slice::<AssetCatalog>(&bytes) {
                Ok(c) => c,
                Err(e) => {
                    console::warn_1(
                        &format!("Failed parsing catalog.json ({e}); using default terrain entries").into(),
                    );
                    AssetCatalog::default()
                }
            },
            Ok((status, _)) => {
                console::warn_1(
                    &format!("HTTP {status} fetching /world/assets/catalog.json; using default terrain entries")
                        .into(),
                );
                AssetCatalog::default()
            }
            Err(e) => {
                console::warn_1(
                    &format!("Error fetching /world/assets/catalog.json ({e}); using default terrain entries")
                        .into(),
                );
                AssetCatalog::default()
            }
        };

        if catalog.terrain_textures.is_empty() {
            console::warn_1(
                &"catalog.json contained no terrain_textures; falling back to default /world/assets/terrain/*.png entries"
                    .into(),
            );
            catalog.terrain_textures = default_terrain_texture_entries();
        }

        #[cfg(feature = "editor")]
        weak.borrow_mut().editor.set_catalog(&catalog);

        let mut images = Vec::with_capacity(catalog.terrain_textures.len());
        for entry in &catalog.terrain_textures {
            if entry.layer as usize >= MAX_TERRAIN_MATERIALS {
                console::warn_1(
                    &format!(
                        "Skipping terrain texture '{}' at layer {} (max supported: {})",
                        entry.id,
                        entry.layer,
                        MAX_TERRAIN_MATERIALS - 1
                    )
                    .into(),
                );
                continue;
            }

            let albedo_bytes = match http_get_bytes(&entry.albedo_path).await {
                Ok((200, data)) => data,
                Ok((status, _)) => {
                    console::warn_1(
                        &format!("Failed loading {}: HTTP {status}", entry.albedo_path).into(),
                    );
                    continue;
                }
                Err(e) => {
                    console::warn_1(
                        &format!("Error loading {}: {e}", entry.albedo_path).into(),
                    );
                    continue;
                }
            };

            let (albedo_width, albedo_height, albedo_rgb) = match decode_albedo_png(&albedo_bytes) {
                Ok(decoded) => decoded,
                Err(e) => {
                    console::warn_1(
                        &format!("Invalid albedo PNG {}: {e}", entry.albedo_path).into(),
                    );
                    continue;
                }
            };

            let height_map = if let Some(h_path) = &entry.height_path {
                match http_get_bytes(h_path).await {
                    Ok((200, h_bytes)) => match decode_height_png(&h_bytes) {
                        Ok(decoded) => Some(decoded),
                        Err(e) => {
                            console::warn_1(
                                &format!("Invalid height PNG {h_path}: {e}").into(),
                            );
                            None
                        }
                    },
                    Ok((status, _)) => {
                        console::warn_1(
                            &format!("Failed loading {h_path}: HTTP {status}").into(),
                        );
                        None
                    }
                    Err(e) => {
                        console::warn_1(&format!("Error loading {h_path}: {e}").into());
                        None
                    }
                }
            } else {
                None
            };

            images.push(TerrainLayerImage {
                layer: entry.layer,
                albedo_width,
                albedo_height,
                albedo_rgb,
                height_map,
            });
        }

        if images.is_empty() {
            console::error_1(
                &"No terrain PNG textures could be loaded from /world/assets/terrain/".into(),
            );
        } else if let Err(e) = weak.borrow_mut().renderer.set_terrain_textures(&images) {
            console::error_1(&format!("Failed uploading terrain textures: {e}").into());
        }
    });
}
