use std::collections::HashSet;

use shared::world::{AreaKind, TERRAIN_MATERIAL_COUNT, TILE_SIZE, TILE_VERTICES, blend_splat_sample};

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
        let min_wx = (hit.x - r).floor() as i32;
        let max_wx = (hit.x + r).ceil() as i32;
        let min_wz = (hit.z - r).floor() as i32;
        let max_wz = (hit.z + r).ceil() as i32;
        let strength = (self.brush_strength * 0.45 * dt).clamp(0.02, 0.85);
        let target_mat = self.texture_layer.min(TERRAIN_MATERIAL_COUNT - 1) as u8;

        // Precompute new vertex splats in world grid coordinates so shared tile borders stay 100% locked.
        let mut updates: Vec<(i32, i32, [u8; 4], [u8; 4])> = Vec::new();
        for wz in min_wz..=max_wz {
            for wx in min_wx..=max_wx {
                let dx = wx as f32 - hit.x;
                let dz = wz as f32 - hit.z;
                let dist = (dx * dx + dz * dz).sqrt();
                if dist > r {
                    continue;
                }
                let alpha = (1.0 - dist / r).powi(2) * strength;
                let Some((ids, weights)) = self.sample_vertex_splat(wx, wz) else {
                    continue;
                };
                let (new_ids, new_weights) = blend_splat_sample(ids, weights, target_mat, alpha);
                updates.push((wx, wz, new_ids, new_weights));
            }
        }

        if updates.is_empty() {
            return;
        }

        if self.active_area.kind == AreaKind::Outside {
            let mut touched: HashSet<(i32, i32)> = HashSet::new();
            for (wx, wz, new_ids, new_weights) in updates {
                for (&(tx, ty), tile) in &mut self.outside_tiles {
                    let ox = tx * TILE_SIZE as i32;
                    let oz = ty * TILE_SIZE as i32;
                    if wx >= ox
                        && wx <= ox + TILE_SIZE as i32
                        && wz >= oz
                        && wz <= oz + TILE_SIZE as i32
                    {
                        let vx = (wx - ox) as usize;
                        let vz = (wz - oz) as usize;
                        let idx = vz * TILE_VERTICES + vx;
                        tile.vis.splat_ids[idx] = new_ids;
                        tile.vis.splat_weights[idx] = new_weights;
                        tile.dirty = true;
                        touched.insert((tx, ty));
                    }
                }
            }
            for key in touched {
                if let Some(tile) = self.outside_tiles.get(&key) {
                    tile.gpu
                        .upload_splat(gl, &tile.vis.splat_ids, &tile.vis.splat_weights);
                }
                self.mark_unsaved(now_ms);
            }
        } else if let Some(inside) = &mut self.inside_area {
            let w = inside.vis.width as i32;
            let l = inside.vis.length as i32;
            let vw = w as usize + 1;
            let expected = vw * (l as usize + 1);
            if inside.vis.splat_ids.len() != expected || inside.vis.splat_weights.len() != expected {
                return;
            }
            let mut changed = false;
            for (wx, wz, new_ids, new_weights) in updates {
                if wx >= 0 && wx <= w && wz >= 0 && wz <= l {
                    let idx = wz as usize * vw + wx as usize;
                    inside.vis.splat_ids[idx] = new_ids;
                    inside.vis.splat_weights[idx] = new_weights;
                    changed = true;
                }
            }
            if changed {
                inside.dirty = true;
                inside
                    .gpu
                    .upload_splat(gl, &inside.vis.splat_ids, &inside.vis.splat_weights);
                self.mark_unsaved(now_ms);
            }
        }
    }
}
