use std::fs;
use std::path::Path;

use shared::world::{
    AreaKind, AreaMetadata, AssetCatalog, AssetEntry, InsideKind, InsideVisData, MAT_COBBLESTONE,
    MAT_DIRT, PropPlacement, TILE_VERTICES, TerrainTextureEntry, TileVisData, blend_splat_sample,
    compute_inside_sim_from_vis, compute_sim_from_vis,
};

use super::decorations::starter_decoration_assets;
use super::terrain_textures::starter_terrain_textures;

/// Ensures starter `.glb` decorations, terrain `.png` textures, `catalog.json`, and default `main` / `village_shop` areas exist.
pub fn ensure_world_seeded(world_dir: &Path) -> anyhow::Result<()> {
    let dec_dir = world_dir.join("assets").join("decorations");
    fs::create_dir_all(&dec_dir)?;
    let tex_dir = world_dir.join("assets").join("terrain");
    fs::create_dir_all(&tex_dir)?;

    let assets = starter_decoration_assets();
    let mut starter_decorations = Vec::with_capacity(assets.len());
    for (id, name, category, glb_bytes) in assets {
        let filename = format!("{id}.glb");
        let glb_path = dec_dir.join(&filename);
        if !glb_path.exists() {
            fs::write(&glb_path, glb_bytes)?;
        }
        starter_decorations.push(AssetEntry {
            id: id.to_owned(),
            name: name.to_owned(),
            path: format!("/world/assets/decorations/{filename}"),
            category: category.to_owned(),
        });
    }

    let terrains = starter_terrain_textures()?;
    let mut starter_terrain = Vec::with_capacity(terrains.len());
    for tex in terrains {
        let albedo_name = format!("{}.png", tex.id);
        let height_name = format!("{}_height.png", tex.id);
        let albedo_file = tex_dir.join(&albedo_name);
        let height_file = tex_dir.join(&height_name);
        if !albedo_file.exists() {
            fs::write(&albedo_file, tex.albedo_png)?;
        }
        if !height_file.exists() {
            fs::write(&height_file, tex.height_png)?;
        }
        starter_terrain.push(TerrainTextureEntry {
            layer: tex.layer,
            id: tex.id.to_owned(),
            name: tex.name.to_owned(),
            category: tex.category.to_owned(),
            albedo_path: format!("/world/assets/terrain/{albedo_name}"),
            height_path: Some(format!("/world/assets/terrain/{height_name}")),
        });
    }

    let catalog_path = world_dir.join("assets").join("catalog.json");
    if catalog_path.exists() {
        let raw = fs::read(&catalog_path)?;
        let is_legacy_array = raw
            .iter()
            .copied()
            .find(|b| !b.is_ascii_whitespace())
            == Some(b'[');
        let mut existing: AssetCatalog = serde_json::from_slice(&raw)?;
        let mut modified = is_legacy_array;

        for entry in starter_decorations {
            if !existing.decorations.iter().any(|e| e.id == entry.id) {
                existing.decorations.push(entry);
                modified = true;
            }
        }
        for entry in starter_terrain {
            if !existing
                .terrain_textures
                .iter()
                .any(|e| e.layer == entry.layer || e.id == entry.id)
            {
                existing.terrain_textures.push(entry);
                modified = true;
            }
        }
        if modified {
            fs::write(&catalog_path, serde_json::to_vec_pretty(&existing)?)?;
        }
    } else {
        let catalog = AssetCatalog {
            decorations: starter_decorations,
            terrain_textures: starter_terrain,
        };
        fs::write(&catalog_path, serde_json::to_vec_pretty(&catalog)?)?;
    }

    seed_main_outside_area(world_dir)?;
    seed_village_shop_inside_area(world_dir)?;
    Ok(())
}

fn seed_main_outside_area(world_dir: &Path) -> anyhow::Result<()> {
    let main_dir = world_dir.join("areas").join("main");
    let area_bin = main_dir.join("area.bin");
    if area_bin.exists() {
        return Ok(());
    }

    fs::create_dir_all(&main_dir)?;
    let meta = AreaMetadata {
        id: "main".to_owned(),
        name: "Main World".to_owned(),
        kind: AreaKind::Outside,
        dimensions: None,
    };
    fs::write(&area_bin, postcard::to_allocvec(&meta)?)?;

    // Seed the initial 3x3 Outside window (-1..=1, -1..=1) centered on (0, 0).
    for ty in -1..=1 {
        for tx in -1..=1 {
            let mut vis = TileVisData::seeded_grass("main", tx, ty);

            // Gentle welcoming features on starter tile (0, 0) while keeping center spawn (32, 32) flat.
            if tx == 0 && ty == 0 {
                for vz in 0..TILE_VERTICES {
                    for vx in 0..TILE_VERTICES {
                        let dx = vx as f32 - 14.0;
                        let dz = vz as f32 - 14.0;
                        let dist2 = dx * dx + dz * dz;
                        if dist2 < 100.0 {
                            let bump = (1.0 - dist2 / 100.0).powi(2) * 2.4;
                            vis.heights[vz * TILE_VERTICES + vx] = bump;
                        }
                    }
                }

                // A subtle dirt path leading south-east from spawn (32, 32)
                for vz in 24..=44 {
                    for vx in 29..=36 {
                        let d = ((vx as f32 - 32.0).abs() / 3.0).min(1.0);
                        let dirt = (1.0 - d) * 0.82;
                        let idx = vz * TILE_VERTICES + vx;
                        let (ids, weights) = blend_splat_sample(
                            vis.splat_ids[idx],
                            vis.splat_weights[idx],
                            MAT_DIRT,
                            dirt,
                        );
                        vis.splat_ids[idx] = ids;
                        vis.splat_weights[idx] = weights;
                    }
                }

                vis.props = vec![
                    PropPlacement {
                        id: 1,
                        asset_id: "ruin_stone".to_owned(),
                        position: [32.0, 0.0, 22.0],
                        rotation: [0.0, 0.0, 0.0],
                        scale: [1.0, 1.0, 1.0],
                        cast_shadow: true,
                    },
                    PropPlacement {
                        id: 2,
                        asset_id: "pine_tree".to_owned(),
                        position: [20.0, 0.4, 18.0],
                        rotation: [0.0, 0.7, 0.0],
                        scale: [1.1, 1.1, 1.1],
                        cast_shadow: true,
                    },
                    PropPlacement {
                        id: 3,
                        asset_id: "oak_tree".to_owned(),
                        position: [44.0, 0.0, 26.0],
                        rotation: [0.0, 2.1, 0.0],
                        scale: [1.0, 1.0, 1.0],
                        cast_shadow: true,
                    },
                    PropPlacement {
                        id: 4,
                        asset_id: "rock_cluster".to_owned(),
                        position: [24.0, 0.0, 42.0],
                        rotation: [0.0, 1.2, 0.0],
                        scale: [1.0, 1.0, 1.0],
                        cast_shadow: true,
                    },
                    PropPlacement {
                        id: 5,
                        asset_id: "bush".to_owned(),
                        position: [38.0, 0.0, 38.0],
                        rotation: [0.0, 0.4, 0.0],
                        scale: [1.0, 1.0, 1.0],
                        cast_shadow: true,
                    },
                ];
            }

            vis.recompute_slope_walkability();
            let sim = compute_sim_from_vis(&vis, None);

            let tile_dir = main_dir.join(format!("{tx},{ty}"));
            fs::create_dir_all(&tile_dir)?;
            fs::write(tile_dir.join("vis.bin"), postcard::to_allocvec(&vis)?)?;
            fs::write(tile_dir.join("sim.bin"), postcard::to_allocvec(&sim)?)?;
        }
    }

    Ok(())
}

fn seed_village_shop_inside_area(world_dir: &Path) -> anyhow::Result<()> {
    let shop_dir = world_dir.join("areas").join("village_shop");
    let area_bin = shop_dir.join("area.bin");
    if area_bin.exists() {
        return Ok(());
    }

    fs::create_dir_all(&shop_dir)?;
    let (w, l) = (24u32, 24u32);
    let meta = AreaMetadata {
        id: "village_shop".to_owned(),
        name: "Village Shop".to_owned(),
        kind: AreaKind::Inside(InsideKind::Ephemeral),
        dimensions: Some((w, l)),
    };
    fs::write(&area_bin, postcard::to_allocvec(&meta)?)?;

    let mut vis = InsideVisData::flat_default(w, l);
    let vw = w as usize + 1;
    let vh = l as usize + 1;
    for vz in 0..vh {
        for vx in 0..vw {
            let border = vx <= 2 || vz <= 2 || vx >= (w as usize - 2) || vz >= (l as usize - 2);
            let mat = if border { MAT_COBBLESTONE } else { MAT_DIRT };
            let idx = vz * vw + vx;
            vis.splat_ids[idx] = [mat, 0, 0, 0];
            vis.splat_weights[idx] = [255, 0, 0, 0];
        }
    }
    vis.props.push(PropPlacement {
        id: 101,
        asset_id: "ruin_stone".to_owned(),
        position: [12.0, 0.0, 6.0],
        rotation: [0.0, 0.0, 0.0],
        scale: [1.0, 1.0, 1.0],
        cast_shadow: true,
    });
    vis.recompute_slope_walkability();
    let sim = compute_inside_sim_from_vis(&vis, None);

    fs::write(shop_dir.join("vis.bin"), postcard::to_allocvec(&vis)?)?;
    fs::write(shop_dir.join("sim.bin"), postcard::to_allocvec(&sim)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_catalog_deserializes_legacy_array_and_structured_object() {
        let legacy_json = r#"[
            { "id": "pine_tree", "name": "Pine Tree", "path": "/world/assets/decorations/pine_tree.glb", "category": "Nature" }
        ]"#;
        let legacy: AssetCatalog = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(legacy.decorations.len(), 1);
        assert_eq!(legacy.decorations[0].id, "pine_tree");
        assert!(legacy.terrain_textures.is_empty());

        let structured = AssetCatalog {
            decorations: legacy.decorations,
            terrain_textures: vec![TerrainTextureEntry {
                layer: 0,
                id: "grass".to_owned(),
                name: "Grass".to_owned(),
                category: "Grass".to_owned(),
                albedo_path: "/world/assets/terrain/grass.png".to_owned(),
                height_path: Some("/world/assets/terrain/grass_height.png".to_owned()),
            }],
        };
        let encoded = serde_json::to_string(&structured).unwrap();
        let decoded: AssetCatalog = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, structured);
    }

    #[test]
    fn ensure_world_seeded_merges_missing_starters_preserving_custom_entries() {
        let unique = format!(
            "ff_seed_merge_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let assets_dir = root.join("assets");
        fs::create_dir_all(&assets_dir).unwrap();

        let initial = vec![
            AssetEntry {
                id: "pine_tree".to_owned(),
                name: "Pine Tree".to_owned(),
                path: "/world/assets/decorations/pine_tree.glb".to_owned(),
                category: "Nature".to_owned(),
            },
            AssetEntry {
                id: "rock_boulder".to_owned(),
                name: "Boulder".to_owned(),
                path: "/world/assets/decorations/rock_boulder.glb".to_owned(),
                category: "Rocks".to_owned(),
            },
            AssetEntry {
                id: "custom_altar".to_owned(),
                name: "Custom Altar".to_owned(),
                path: "/world/assets/decorations/custom_altar.glb".to_owned(),
                category: "Props".to_owned(),
            },
        ];
        let catalog_path = assets_dir.join("catalog.json");
        fs::write(&catalog_path, serde_json::to_vec_pretty(&initial).unwrap()).unwrap();

        ensure_world_seeded(&root).unwrap();

        let raw = fs::read(&catalog_path).unwrap();
        let first_non_ws = raw.iter().copied().find(|b| !b.is_ascii_whitespace());
        assert_eq!(first_non_ws, Some(b'{'));
        let mut merged: AssetCatalog = serde_json::from_slice(&raw).unwrap();
        assert_eq!(merged.decorations.len(), 15);
        assert!(merged.decorations.iter().any(|e| e.id == "custom_altar"));
        assert!(merged.decorations.iter().any(|e| e.id == "cliff_wall"));
        assert!(merged.decorations.iter().any(|e| e.id == "rock_scree"));

        assert_eq!(merged.terrain_textures.len(), 9);
        let terrain_dir = assets_dir.join("terrain");
        for entry in &merged.terrain_textures {
            assert!(terrain_dir.join(format!("{}.png", entry.id)).exists());
            assert!(terrain_dir.join(format!("{}_height.png", entry.id)).exists());
        }

        // Simulate artist replacing grass.png and adding a custom terrain layer in catalog.json
        let custom_png = b"ARTIST_CUSTOM_GRASS_PNG".to_vec();
        let grass_path = terrain_dir.join("grass.png");
        fs::write(&grass_path, &custom_png).unwrap();
        merged.terrain_textures.push(TerrainTextureEntry {
            layer: 9,
            id: "snow".to_owned(),
            name: "Snow".to_owned(),
            category: "Earth".to_owned(),
            albedo_path: "/world/assets/terrain/snow.png".to_owned(),
            height_path: None,
        });
        fs::write(&catalog_path, serde_json::to_vec_pretty(&merged).unwrap()).unwrap();

        ensure_world_seeded(&root).unwrap();

        assert_eq!(fs::read(&grass_path).unwrap(), custom_png);
        let after: AssetCatalog = serde_json::from_slice(&fs::read(&catalog_path).unwrap()).unwrap();
        assert_eq!(after.terrain_textures.len(), 10);
        assert!(after.terrain_textures.iter().any(|t| t.layer == 9 && t.id == "snow"));

        let _ = fs::remove_dir_all(&root);
    }
}
