use std::fs;
use std::path::Path;

use shared::world::{
    AreaKind, AreaMetadata, AssetEntry, InsideKind, InsideVisData, PropPlacement, TILE_SIZE,
    TILE_VERTICES, TileVisData, compute_inside_sim_from_vis, compute_sim_from_vis, pack_splat,
};

use super::decorations::starter_decoration_assets;

/// Ensures starter `.glb` decoration assets, `catalog.json`, and default `main` / `village_shop` areas exist.
pub fn ensure_world_seeded(world_dir: &Path) -> anyhow::Result<()> {
    let dec_dir = world_dir.join("assets").join("decorations");
    fs::create_dir_all(&dec_dir)?;

    let assets = starter_decoration_assets();
    let mut catalog = Vec::with_capacity(assets.len());
    for (id, name, category, glb_bytes) in assets {
        let filename = format!("{id}.glb");
        let glb_path = dec_dir.join(&filename);
        if !glb_path.exists() {
            fs::write(&glb_path, glb_bytes)?;
        }
        catalog.push(AssetEntry {
            id: id.to_owned(),
            name: name.to_owned(),
            path: format!("/world/assets/decorations/{filename}"),
            category: category.to_owned(),
        });
    }

    let catalog_path = world_dir.join("assets").join("catalog.json");
    if !catalog_path.exists() {
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
            let mut vis = TileVisData::flat_default();

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
                for z in 24..44 {
                    for x in 29..36 {
                        let d = ((x as f32 - 32.0).abs() / 3.0).min(1.0);
                        let dirt = ((1.0 - d) * 210.0) as u8;
                        let grass = 255u8.saturating_sub(dirt);
                        vis.splat_map[z * TILE_SIZE + x] = pack_splat(grass, dirt, 0, 0);
                    }
                }

                vis.props = vec![
                    PropPlacement {
                        id: 1,
                        asset_id: "ruin_stone".to_owned(),
                        position: [32.0, 0.0, 22.0],
                        rotation_y: 0.0,
                        scale: [1.0, 1.0, 1.0],
                    },
                    PropPlacement {
                        id: 2,
                        asset_id: "pine_tree".to_owned(),
                        position: [20.0, 0.4, 18.0],
                        rotation_y: 0.7,
                        scale: [1.1, 1.1, 1.1],
                    },
                    PropPlacement {
                        id: 3,
                        asset_id: "oak_tree".to_owned(),
                        position: [44.0, 0.0, 26.0],
                        rotation_y: 2.1,
                        scale: [1.0, 1.0, 1.0],
                    },
                    PropPlacement {
                        id: 4,
                        asset_id: "rock_cluster".to_owned(),
                        position: [24.0, 0.0, 42.0],
                        rotation_y: 1.2,
                        scale: [1.0, 1.0, 1.0],
                    },
                    PropPlacement {
                        id: 5,
                        asset_id: "bush".to_owned(),
                        position: [38.0, 0.0, 38.0],
                        rotation_y: 0.4,
                        scale: [1.0, 1.0, 1.0],
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
    for z in 0..l as usize {
        for x in 0..w as usize {
            let border = x < 2 || z < 2 || x >= (w as usize - 2) || z >= (l as usize - 2);
            vis.splat_map[z * w as usize + x] = if border {
                pack_splat(0, 0, 255, 0) // Rock floor border
            } else {
                pack_splat(0, 255, 0, 0) // Dirt/plank interior floor
            };
        }
    }
    vis.props.push(PropPlacement {
        id: 101,
        asset_id: "ruin_stone".to_owned(),
        position: [12.0, 0.0, 6.0],
        rotation_y: 0.0,
        scale: [1.0, 1.0, 1.0],
    });
    vis.recompute_slope_walkability();
    let sim = compute_inside_sim_from_vis(&vis, None);

    fs::write(shop_dir.join("vis.bin"), postcard::to_allocvec(&vis)?)?;
    fs::write(shop_dir.join("sim.bin"), postcard::to_allocvec(&sim)?)?;
    Ok(())
}
