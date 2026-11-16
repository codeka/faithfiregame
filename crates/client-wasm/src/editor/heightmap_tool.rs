use std::collections::HashSet;

use shared::world::{AreaKind, TILE_SIZE, TILE_VERTICES};

use crate::input::InputState;

use super::state::EditorState;
use super::types::HeightMode;

impl EditorState {
    pub(super) fn handle_heightmap_tool(
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

        if self.height_mode == HeightMode::Flatten && self.flatten_target_y.is_none() {
            self.flatten_target_y = Some(hit.y);
        }

        let r = self.brush_radius.max(0.5);
        let min_wx = (hit.x - r).floor() as i32;
        let max_wx = (hit.x + r).ceil() as i32;
        let min_wz = (hit.z - r).floor() as i32;
        let max_wz = (hit.z + r).ceil() as i32;
        let step_amount = self.brush_strength * dt;

        // Precompute new vertex heights in world grid coordinates so shared tile borders stay 100% locked.
        let mut updates: Vec<(i32, i32, f32)> = Vec::new();
        for wz in min_wz..=max_wz {
            for wx in min_wx..=max_wx {
                let dx = wx as f32 - hit.x;
                let dz = wz as f32 - hit.z;
                let dist = (dx * dx + dz * dz).sqrt();
                if dist > r {
                    continue;
                }
                let falloff = (1.0 - dist / r).powi(2);
                let Some(cur_h) = self.sample_vertex_height(wx, wz) else {
                    continue;
                };

                let new_h = match self.height_mode {
                    HeightMode::Raise => cur_h + step_amount * falloff,
                    HeightMode::Lower => cur_h - step_amount * falloff,
                    HeightMode::Flatten => {
                        let target = self.flatten_target_y.unwrap_or(hit.y);
                        let t = (falloff * (self.brush_strength * 0.6) * dt).clamp(0.0, 1.0);
                        cur_h + (target - cur_h) * t
                    }
                    HeightMode::Smooth => {
                        let mut sum = 0.0;
                        let mut count = 0.0;
                        for oz in -1..=1 {
                            for ox in -1..=1 {
                                if let Some(nh) = self.sample_vertex_height(wx + ox, wz + oz) {
                                    sum += nh;
                                    count += 1.0;
                                }
                            }
                        }
                        let avg = if count > 0.0 { sum / count } else { cur_h };
                        let t = (falloff * (self.brush_strength * 0.8) * dt).clamp(0.0, 1.0);
                        cur_h + (avg - cur_h) * t
                    }
                };
                updates.push((wx, wz, new_h));
            }
        }

        if updates.is_empty() {
            return;
        }

        if self.active_area.kind == AreaKind::Outside {
            let mut touched_tiles: HashSet<(i32, i32)> = HashSet::new();
            for (wx, wz, h) in updates {
                for (&(tx, ty), tile) in &mut self.outside_tiles {
                    let ox = tx * TILE_SIZE as i32;
                    let oz = ty * TILE_SIZE as i32;
                    if wx >= ox && wx <= ox + TILE_SIZE as i32 && wz >= oz && wz <= oz + TILE_SIZE as i32
                    {
                        let vx = (wx - ox) as usize;
                        let vz = (wz - oz) as usize;
                        tile.vis.heights[vz * TILE_VERTICES + vx] = h;
                        tile.dirty = true;
                        touched_tiles.insert((tx, ty));
                    }
                }
            }

            if !touched_tiles.is_empty() {
                for &(tx, ty) in &touched_tiles {
                    if let Some(tile) = self.outside_tiles.get_mut(&(tx, ty)) {
                        let ox = tx * TILE_SIZE as i32;
                        let oz = ty * TILE_SIZE as i32;
                        let cmin_x = (min_wx - 1 - ox).max(0) as usize;
                        let cmax_x = (max_wx - ox).clamp(0, TILE_SIZE as i32 - 1) as usize;
                        let cmin_z = (min_wz - 1 - oz).max(0) as usize;
                        let cmax_z = (max_wz - oz).clamp(0, TILE_SIZE as i32 - 1) as usize;
                        tile.vis
                            .recompute_slope_walkability_in_rect(cmin_x, cmin_z, cmax_x, cmax_z);
                        tile.gpu
                            .upload_walkability(gl, |x, z| tile.vis.walkable.get(x, z));
                    }
                }
                let mut refresh = HashSet::new();
                for &(tx, ty) in &touched_tiles {
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let k = (tx + dx, ty + dy);
                            if self.outside_tiles.contains_key(&k) {
                                refresh.insert(k);
                            }
                        }
                    }
                }
                let refresh_vec: Vec<_> = refresh.into_iter().collect();
                self.refresh_outside_tile_meshes(gl, &refresh_vec);
                self.mark_unsaved(now_ms);
            }
        } else if let Some(inside) = &mut self.inside_area {
            let w = inside.vis.width as i32;
            let l = inside.vis.length as i32;
            let vw = w as usize + 1;
            let mut changed = false;
            for (wx, wz, h) in updates {
                if wx >= 0 && wx <= w && wz >= 0 && wz <= l {
                    inside.vis.heights[wz as usize * vw + wx as usize] = h;
                    changed = true;
                }
            }
            if changed {
                let cmin_x = (min_wx - 1).max(0) as usize;
                let cmax_x = max_wx.clamp(0, (w - 1).max(0)) as usize;
                let cmin_z = (min_wz - 1).max(0) as usize;
                let cmax_z = max_wz.clamp(0, (l - 1).max(0)) as usize;
                inside
                    .vis
                    .recompute_slope_walkability_in_rect(cmin_x, cmin_z, cmax_x, cmax_z);
                inside
                    .gpu
                    .upload_walkability(gl, |x, z| inside.vis.get_walkable(x, z));
                inside.dirty = true;
                self.refresh_inside_mesh(gl);
                self.mark_unsaved(now_ms);
            }
        }
    }
}
