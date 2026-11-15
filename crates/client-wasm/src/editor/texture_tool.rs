use std::collections::HashSet;

use shared::world::{AreaKind, TILE_SIZE, pack_splat, unpack_splat};

use crate::input::InputState;

use super::state::EditorState;

impl EditorState {
    pub(super) fn handle_texture_tool(
        &mut self,
        gl: &glow::Context,
        input: &InputState,
        dt: f32,
        now_ms: f64,
    ) {
        if !input.left_mouse_down {
            return;
        }
        let Some(hit) = self.hover_world else {
            return;
        };

        let r = self.brush_radius.max(0.5);
        let min_cx = (hit.x - r).floor() as i32;
        let max_cx = (hit.x + r).floor() as i32;
        let min_cz = (hit.z - r).floor() as i32;
        let max_cz = (hit.z + r).floor() as i32;
        let strength = (self.brush_strength * 0.45 * dt).clamp(0.02, 0.85);
        let layer = self.texture_layer.min(3);

        if self.active_area.kind == AreaKind::Outside {
            let mut touched: HashSet<(i32, i32)> = HashSet::new();
            for cz in min_cz..=max_cz {
                for cx in min_cx..=max_cx {
                    let dx = (cx as f32 + 0.5) - hit.x;
                    let dz = (cz as f32 + 0.5) - hit.z;
                    let dist = (dx * dx + dz * dz).sqrt();
                    if dist > r {
                        continue;
                    }
                    let alpha = (1.0 - dist / r).powi(2) * strength;
                    let tx = cx.div_euclid(TILE_SIZE as i32);
                    let ty = cz.div_euclid(TILE_SIZE as i32);
                    let lx = cx.rem_euclid(TILE_SIZE as i32) as usize;
                    let lz = cz.rem_euclid(TILE_SIZE as i32) as usize;

                    if let Some(tile) = self.outside_tiles.get_mut(&(tx, ty)) {
                        let idx = lz * TILE_SIZE + lx;
                        tile.vis.splat_map[idx] =
                            blend_splat(tile.vis.splat_map[idx], layer, alpha);
                        tile.dirty = true;
                        touched.insert((tx, ty));
                    }
                }
            }
            for key in touched {
                if let Some(tile) = self.outside_tiles.get(&key) {
                    tile.gpu.upload_splat(gl, &tile.vis.splat_map);
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
                    let dist = (dx * dx + dz * dz).sqrt();
                    if dist > r {
                        continue;
                    }
                    let alpha = (1.0 - dist / r).powi(2) * strength;
                    let idx = cz as usize * w as usize + cx as usize;
                    inside.vis.splat_map[idx] =
                        blend_splat(inside.vis.splat_map[idx], layer, alpha);
                    changed = true;
                }
            }
            if changed {
                inside.dirty = true;
                inside.gpu.upload_splat(gl, &inside.vis.splat_map);
                self.mark_unsaved(now_ms);
            }
        }
    }
}

fn blend_splat(packed: u32, target_layer: usize, alpha: f32) -> u32 {
    let raw = unpack_splat(packed);
    let mut w = [
        raw[0] as f32 / 255.0,
        raw[1] as f32 / 255.0,
        raw[2] as f32 / 255.0,
        raw[3] as f32 / 255.0,
    ];
    for (i, val) in w.iter_mut().enumerate() {
        let goal = if i == target_layer { 1.0 } else { 0.0 };
        *val += (goal - *val) * alpha;
    }
    let sum: f32 = w.iter().sum();
    if sum > 1e-4 {
        for val in &mut w {
            *val /= sum;
        }
    } else {
        w = [1.0, 0.0, 0.0, 0.0];
    }
    pack_splat(
        (w[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (w[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (w[2].clamp(0.0, 1.0) * 255.0).round() as u8,
        (w[3].clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}
