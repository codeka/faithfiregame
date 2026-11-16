use std::collections::HashSet;

use glam::{Mat4, Vec3};
use shared::world::{AreaKind, PropPlacement, TILE_SIZE};

use crate::input::InputState;
use crate::render::prop_model_matrix;

use super::state::EditorState;
use super::types::WalkabilityMode;

/// Minimum height above terrain (`meters`) before a prop blocks ground walking (ignores flat decals/rugs).
const MIN_OBSTACLE_CLEARANCE_Y: f32 = 0.18;
/// Maximum height above terrain (`meters`) for a prop's bottom before a player can walk underneath it (overhead pipes/arches).
const MAX_HEADROOM_Y: f32 = 2.05;

impl EditorState {
    pub(super) fn handle_walkability_tool(
        &mut self,
        gl: &glow::Context,
        input: &InputState,
        now_ms: f64,
    ) {
        if !input.left_mouse_down {
            return;
        }
        let Some(hit) = self.hover_world else {
            return;
        };

        let r = self.brush_radius.max(0.45);
        let r2 = r * r;
        let center_cx = hit.x.floor() as i32;
        let center_cz = hit.z.floor() as i32;
        let min_cx = (hit.x - r).floor() as i32;
        let max_cx = (hit.x + r).floor() as i32;
        let min_cz = (hit.z - r).floor() as i32;
        let max_cz = (hit.z + r).floor() as i32;
        let want_walkable = self.walkability_mode == WalkabilityMode::Walkable;

        if self.active_area.kind == AreaKind::Outside {
            let mut touched: HashSet<(i32, i32)> = HashSet::new();
            for cz in min_cz..=max_cz {
                for cx in min_cx..=max_cx {
                    let dx = (cx as f32 + 0.5) - hit.x;
                    let dz = (cz as f32 + 0.5) - hit.z;
                    if (cx != center_cx || cz != center_cz) && (dx * dx + dz * dz) > r2 {
                        continue;
                    }
                    let tx = cx.div_euclid(TILE_SIZE as i32);
                    let ty = cz.div_euclid(TILE_SIZE as i32);
                    let lx = cx.rem_euclid(TILE_SIZE as i32) as usize;
                    let lz = cz.rem_euclid(TILE_SIZE as i32) as usize;
                    if let Some(tile) = self.outside_tiles.get_mut(&(tx, ty)) {
                        let next = want_walkable && tile.vis.is_cell_slope_walkable(lx, lz);
                        if tile.vis.walkable.get(lx, lz) != next {
                            tile.vis.walkable.set(lx, lz, next);
                            tile.dirty = true;
                            touched.insert((tx, ty));
                        }
                    }
                }
            }

            if !touched.is_empty() {
                for (tx, ty) in touched {
                    if let Some(tile) = self.outside_tiles.get(&(tx, ty)) {
                        tile.gpu
                            .upload_walkability(gl, |x, z| tile.vis.walkable.get(x, z));
                    }
                }
                self.mark_unsaved(now_ms);
            }
        } else if let Some(inside) = &mut self.inside_area {
            let w = inside.vis.width as i32;
            let l = inside.vis.length as i32;
            let mut changed = false;
            for cz in min_cz.max(0)..=max_cz.min(l - 1) {
                for cx in min_cx.max(0)..=max_cx.min(w - 1) {
                    let dx = (cx as f32 + 0.5) - hit.x;
                    let dz = (cz as f32 + 0.5) - hit.z;
                    if (cx != center_cx || cz != center_cz) && (dx * dx + dz * dz) > r2 {
                        continue;
                    }
                    let (ux, uz) = (cx as usize, cz as usize);
                    let next = want_walkable && inside.vis.is_cell_slope_walkable(ux, uz);
                    if inside.vis.get_walkable(ux, uz) != next {
                        inside.vis.set_walkable(ux, uz, next);
                        changed = true;
                    }
                }
            }
            if changed {
                inside.dirty = true;
                inside
                    .gpu
                    .upload_walkability(gl, |x, z| inside.vis.get_walkable(x, z));
                self.mark_unsaved(now_ms);
            }
        }
    }

    pub fn reset_walkability_to_slope(&mut self, gl: &glow::Context, now_ms: f64) {
        if self.active_area.kind == AreaKind::Outside {
            let mut any = false;
            for tile in self.outside_tiles.values_mut() {
                tile.vis.recompute_slope_walkability();
                tile.gpu
                    .upload_walkability(gl, |x, z| tile.vis.walkable.get(x, z));
                tile.dirty = true;
                any = true;
            }
            if any {
                self.mark_unsaved(now_ms);
            }
        } else if let Some(inside) = &mut self.inside_area {
            inside.vis.recompute_slope_walkability();
            inside
                .gpu
                .upload_walkability(gl, |x, z| inside.vis.get_walkable(x, z));
            inside.dirty = true;
            self.mark_unsaved(now_ms);
        }
    }

    /// Projects the oriented 3D bounding boxes of ground-level props onto the 1m walkability grid.
    pub fn bake_props_into_walkability(&mut self, gl: &glow::Context, now_ms: f64) {
        if self.active_area.kind == AreaKind::Outside {
            let all_props: Vec<(PropPlacement, Vec3, Vec3)> = self
                .outside_tiles
                .values()
                .flat_map(|t| t.vis.props.iter())
                .filter_map(|p| {
                    let m = self.prop_meshes.get(&p.asset_id)?;
                    Some((p.clone(), m.aabb_min, m.aabb_max))
                })
                .collect();

            let mut touched = Vec::new();
            let keys: Vec<(i32, i32)> = self.outside_tiles.keys().copied().collect();
            for (tx, ty) in keys {
                let ox = (tx * TILE_SIZE as i32) as f32;
                let oz = (ty * TILE_SIZE as i32) as f32;
                let mut changed = false;
                if let Some(tile) = self.outside_tiles.get_mut(&(tx, ty)) {
                    tile.vis.enforce_slope_walkability();
                    for (prop, bmin, bmax) in &all_props {
                        changed |= apply_prop_footprint_to_grid(
                            prop,
                            *bmin,
                            *bmax,
                            ox,
                            oz,
                            TILE_SIZE,
                            TILE_SIZE,
                            |lx, lz| {
                                let stride = TILE_SIZE + 1;
                                tile.vis.heights[lz * stride + lx]
                            },
                            |lx, lz| {
                                if tile.vis.walkable.get(lx, lz) {
                                    tile.vis.walkable.set(lx, lz, false);
                                    true
                                } else {
                                    false
                                }
                            },
                        );
                    }
                    if changed {
                        tile.dirty = true;
                        touched.push((tx, ty));
                    }
                }
            }
            for &(tx, ty) in &touched {
                if let Some(tile) = self.outside_tiles.get(&(tx, ty)) {
                    tile.gpu
                        .upload_walkability(gl, |x, z| tile.vis.walkable.get(x, z));
                }
            }
            if !touched.is_empty() {
                self.mark_unsaved(now_ms);
            }
        } else if let Some(inside) = &mut self.inside_area {
            let prop_bounds: Vec<(PropPlacement, Vec3, Vec3)> = inside
                .vis
                .props
                .iter()
                .filter_map(|p| {
                    let m = self.prop_meshes.get(&p.asset_id)?;
                    Some((p.clone(), m.aabb_min, m.aabb_max))
                })
                .collect();

            inside.vis.enforce_slope_walkability();
            let w = inside.vis.width as usize;
            let l = inside.vis.length as usize;
            let vw = w + 1;
            let mut changed = false;

            for (prop, bmin, bmax) in &prop_bounds {
                let heights_ref = &inside.vis.heights;
                let ground_at = |lx: usize, lz: usize| -> f32 {
                    heights_ref.get(lz * vw + lx).copied().unwrap_or(0.0)
                };
                let mut blocked_cells = Vec::new();
                apply_prop_footprint_to_grid(
                    prop,
                    *bmin,
                    *bmax,
                    0.0,
                    0.0,
                    w,
                    l,
                    ground_at,
                    |lx, lz| {
                        blocked_cells.push((lx, lz));
                        false
                    },
                );
                for (lx, lz) in blocked_cells {
                    if inside.vis.get_walkable(lx, lz) {
                        inside.vis.set_walkable(lx, lz, false);
                        changed = true;
                    }
                }
            }

            if changed {
                inside.dirty = true;
                inside
                    .gpu
                    .upload_walkability(gl, |x, z| inside.vis.get_walkable(x, z));
                self.mark_unsaved(now_ms);
            }
        }
    }
}

fn world_aabb_of_obb(model: &Mat4, bmin: Vec3, bmax: Vec3) -> (Vec3, Vec3) {
    let mut wmin = Vec3::splat(f32::MAX);
    let mut wmax = Vec3::splat(f32::MIN);
    for &x in &[bmin.x, bmax.x] {
        for &y in &[bmin.y, bmax.y] {
            for &z in &[bmin.z, bmax.z] {
                let p = model.transform_point3(Vec3::new(x, y, z));
                wmin = wmin.min(p);
                wmax = wmax.max(p);
            }
        }
    }
    (wmin, wmax)
}

pub(crate) fn prop_obb_overlaps_cell(
    inv_model: &Mat4,
    bmin: Vec3,
    bmax: Vec3,
    cell_wx: f32,
    cell_wz: f32,
    ground_y: f32,
) -> bool {
    const XZ_SAMPLES: [(f32, f32); 5] = [
        (0.5, 0.5),
        (0.2, 0.2),
        (0.8, 0.2),
        (0.2, 0.8),
        (0.8, 0.8),
    ];
    const Y_SAMPLES: [f32; 3] = [0.35, 1.0, 1.75];

    for (ox, oz) in XZ_SAMPLES {
        let wx = cell_wx + ox;
        let wz = cell_wz + oz;
        for oy in Y_SAMPLES {
            let local = inv_model.transform_point3(Vec3::new(wx, ground_y + oy, wz));
            if local.x >= bmin.x
                && local.x <= bmax.x
                && local.y >= bmin.y
                && local.y <= bmax.y
                && local.z >= bmin.z
                && local.z <= bmax.z
            {
                return true;
            }
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn apply_prop_footprint_to_grid(
    prop: &PropPlacement,
    bmin: Vec3,
    bmax: Vec3,
    origin_x: f32,
    origin_z: f32,
    width: usize,
    length: usize,
    ground_at: impl Fn(usize, usize) -> f32,
    mut mark_blocked: impl FnMut(usize, usize) -> bool,
) -> bool {
    let model = prop_model_matrix(prop);
    let det = model.determinant();
    if det.abs() < 1e-8 {
        return false;
    }
    let inv_model = model.inverse();
    let (wmin, wmax) = world_aabb_of_obb(&model, bmin, bmax);

    let min_lx = ((wmin.x - origin_x).floor() as isize).max(0) as usize;
    let max_lx = ((wmax.x - origin_x).ceil() as isize).min(width as isize - 1);
    let min_lz = ((wmin.z - origin_z).floor() as isize).max(0) as usize;
    let max_lz = ((wmax.z - origin_z).ceil() as isize).min(length as isize - 1);
    if max_lx < min_lx as isize || max_lz < min_lz as isize {
        return false;
    }

    let mut changed = false;
    for lz in min_lz..=(max_lz as usize) {
        for lx in min_lx..=(max_lx as usize) {
            let gy = ground_at(lx, lz);
            if wmax.y < gy + MIN_OBSTACLE_CLEARANCE_Y || wmin.y > gy + MAX_HEADROOM_Y {
                continue;
            }
            let cwx = origin_x + lx as f32;
            let cwz = origin_z + lz as f32;
            if prop_obb_overlaps_cell(&inv_model, bmin, bmax, cwx, cwz, gy) {
                changed |= mark_blocked(lx, lz);
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ground_prop_blocks_footprint_while_overhead_pipe_leaves_floor_walkable() {
        let pod = PropPlacement {
            id: 1,
            asset_id: "local:hero_cryo_pod".to_owned(),
            position: [5.5, 0.0, 5.5],
            rotation: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
            cast_shadow: true,
        };
        let mut blocked = Vec::new();
        apply_prop_footprint_to_grid(
            &pod,
            Vec3::new(-0.9, 0.0, -1.4),
            Vec3::new(0.9, 1.8, 1.4),
            0.0,
            0.0,
            12,
            12,
            |_, _| 0.0,
            |x, z| {
                blocked.push((x, z));
                true
            },
        );
        assert!(blocked.contains(&(5, 5)));
        assert!(!blocked.contains(&(1, 1)));

        // Overhead pipe at Y = 4.2m should not block floor cells beneath it.
        let pipe = PropPlacement {
            id: 2,
            asset_id: "local:cave_pipes".to_owned(),
            position: [5.5, 4.2, 5.5],
            rotation: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
            cast_shadow: true,
        };
        let mut pipe_blocked = Vec::new();
        apply_prop_footprint_to_grid(
            &pipe,
            Vec3::new(-3.0, -0.3, -0.3),
            Vec3::new(3.0, 0.3, 0.3),
            0.0,
            0.0,
            12,
            12,
            |_, _| 0.0,
            |x, z| {
                pipe_blocked.push((x, z));
                true
            },
        );
        assert!(pipe_blocked.is_empty());
    }
}
