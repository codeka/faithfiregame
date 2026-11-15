use glam::Vec3;
use shared::world::{AreaKind, PropPlacement, TILE_SIZE};

use crate::camera::Camera;
use crate::input::InputState;
use crate::render::prop_model_matrix;

use super::state::EditorState;
use super::types::DecorationSubMode;

impl EditorState {
    pub(super) fn handle_decoration_tool(
        &mut self,
        camera: &Camera,
        input: &InputState,
        now_ms: f64,
    ) {
        match self.decoration_submode {
            DecorationSubMode::Place => {
                if input.left_just_pressed {
                    if let Some(hit) = self.hover_world {
                        let id = self.next_prop_id;
                        self.next_prop_id += 1;
                        let placement = PropPlacement {
                            id,
                            asset_id: self.active_asset_id.clone(),
                            position: hit.to_array(),
                            rotation_y: self.preview_rotation_y,
                            scale: [self.preview_scale; 3],
                        };
                        if self.insert_prop(placement) {
                            self.selected_prop_id = Some(id);
                            self.mark_unsaved(now_ms);
                        }
                    }
                }
            }
            DecorationSubMode::Move => {
                if input.left_just_pressed {
                    if let Some((orig, dir)) = camera.screen_ray(
                        input.mouse_x,
                        input.mouse_y,
                        input.canvas_width,
                        input.canvas_height,
                    ) {
                        self.selected_prop_id = self.raycast_prop(orig, dir);
                        self.dragging_prop = self.selected_prop_id.is_some();
                    }
                }
                if input.left_mouse_down && self.dragging_prop {
                    if let (Some(id), Some(hit)) = (self.selected_prop_id, self.hover_world) {
                        if self.move_prop_to(id, hit) {
                            self.mark_unsaved(now_ms);
                        }
                    }
                }
            }
            DecorationSubMode::Rotate => {
                if input.left_just_pressed {
                    if let Some((orig, dir)) = camera.screen_ray(
                        input.mouse_x,
                        input.mouse_y,
                        input.canvas_width,
                        input.canvas_height,
                    ) {
                        if let Some(hit_id) = self.raycast_prop(orig, dir) {
                            self.selected_prop_id = Some(hit_id);
                        }
                        self.rotating_prop = self.selected_prop_id.is_some();
                    }
                }
                if input.left_mouse_down && self.rotating_prop && input.mouse_delta.0.abs() > 0.0 {
                    if let Some(id) = self.selected_prop_id {
                        self.rotate_prop_by(id, input.mouse_delta.0 * 0.015, now_ms);
                    }
                }
            }
        }
    }

    fn insert_prop(&mut self, prop: PropPlacement) -> bool {
        if self.active_area.kind == AreaKind::Outside {
            let tx = (prop.position[0] / TILE_SIZE as f32).floor() as i32;
            let ty = (prop.position[2] / TILE_SIZE as f32).floor() as i32;
            if let Some(tile) = self.outside_tiles.get_mut(&(tx, ty)) {
                tile.vis.props.push(prop);
                tile.dirty = true;
                return true;
            }
            false
        } else if let Some(inside) = &mut self.inside_area {
            inside.vis.props.push(prop);
            inside.dirty = true;
            true
        } else {
            false
        }
    }

    fn move_prop_to(&mut self, prop_id: u64, pos: Vec3) -> bool {
        if self.active_area.kind == AreaKind::Outside {
            let new_tx = (pos.x / TILE_SIZE as f32).floor() as i32;
            let new_ty = (pos.z / TILE_SIZE as f32).floor() as i32;
            if !self.outside_tiles.contains_key(&(new_tx, new_ty)) {
                return false;
            }
            let mut found_old: Option<(i32, i32, usize)> = None;
            for (&key, tile) in &self.outside_tiles {
                if let Some(idx) = tile.vis.props.iter().position(|p| p.id == prop_id) {
                    found_old = Some((key.0, key.1, idx));
                    break;
                }
            }
            let Some((old_tx, old_ty, idx)) = found_old else {
                return false;
            };
            if old_tx == new_tx && old_ty == new_ty {
                if let Some(tile) = self.outside_tiles.get_mut(&(old_tx, old_ty)) {
                    tile.vis.props[idx].position = pos.to_array();
                    tile.dirty = true;
                }
            } else {
                let mut moved = self
                    .outside_tiles
                    .get_mut(&(old_tx, old_ty))
                    .unwrap()
                    .vis
                    .props
                    .remove(idx);
                self.outside_tiles
                    .get_mut(&(old_tx, old_ty))
                    .unwrap()
                    .dirty = true;
                moved.position = pos.to_array();
                let dst = self.outside_tiles.get_mut(&(new_tx, new_ty)).unwrap();
                dst.vis.props.push(moved);
                dst.dirty = true;
            }
            true
        } else if let Some(inside) = &mut self.inside_area {
            if let Some(prop) = inside.vis.props.iter_mut().find(|p| p.id == prop_id) {
                prop.position = pos.to_array();
                inside.dirty = true;
                return true;
            }
            false
        } else {
            false
        }
    }

    pub(super) fn rotate_prop_by(&mut self, prop_id: u64, delta_rad: f32, now_ms: f64) {
        if self.modify_prop(prop_id, |p| {
            p.rotation_y = (p.rotation_y + delta_rad).rem_euclid(std::f32::consts::TAU);
        }) {
            self.mark_unsaved(now_ms);
        }
    }

    pub fn set_selected_or_preview_rotation_deg(&mut self, degrees: f32, now_ms: f64) {
        let rad = degrees.to_radians().rem_euclid(std::f32::consts::TAU);
        if let Some(id) = self.selected_prop_id {
            if self.modify_prop(id, |p| p.rotation_y = rad) {
                self.mark_unsaved(now_ms);
            }
        } else {
            self.preview_rotation_y = rad;
        }
    }

    pub fn set_selected_or_preview_scale(&mut self, scale: f32, now_ms: f64) {
        let s = scale.clamp(0.25, 4.0);
        if let Some(id) = self.selected_prop_id {
            if self.modify_prop(id, |p| p.scale = [s, s, s]) {
                self.mark_unsaved(now_ms);
            }
        } else {
            self.preview_scale = s;
        }
    }

    pub fn delete_selected_prop(&mut self, now_ms: f64) {
        let Some(prop_id) = self.selected_prop_id.take() else {
            return;
        };
        if self.active_area.kind == AreaKind::Outside {
            for tile in self.outside_tiles.values_mut() {
                let before = tile.vis.props.len();
                tile.vis.props.retain(|p| p.id != prop_id);
                if tile.vis.props.len() != before {
                    tile.dirty = true;
                    self.mark_unsaved(now_ms);
                    break;
                }
            }
        } else if let Some(inside) = &mut self.inside_area {
            let before = inside.vis.props.len();
            inside.vis.props.retain(|p| p.id != prop_id);
            if inside.vis.props.len() != before {
                inside.dirty = true;
                self.mark_unsaved(now_ms);
            }
        }
    }

    fn modify_prop(&mut self, prop_id: u64, mut f: impl FnMut(&mut PropPlacement)) -> bool {
        if self.active_area.kind == AreaKind::Outside {
            for tile in self.outside_tiles.values_mut() {
                if let Some(p) = tile.vis.props.iter_mut().find(|p| p.id == prop_id) {
                    f(p);
                    tile.dirty = true;
                    return true;
                }
            }
            false
        } else if let Some(inside) = &mut self.inside_area {
            if let Some(p) = inside.vis.props.iter_mut().find(|p| p.id == prop_id) {
                f(p);
                inside.dirty = true;
                return true;
            }
            false
        } else {
            false
        }
    }

    pub(super) fn find_selected_prop(&self) -> Option<&PropPlacement> {
        let id = self.selected_prop_id?;
        if self.active_area.kind == AreaKind::Outside {
            for tile in self.outside_tiles.values() {
                if let Some(p) = tile.vis.props.iter().find(|p| p.id == id) {
                    return Some(p);
                }
            }
            None
        } else {
            self.inside_area
                .as_ref()
                .and_then(|inside| inside.vis.props.iter().find(|p| p.id == id))
        }
    }

    fn raycast_prop(&self, ray_orig: Vec3, ray_dir: Vec3) -> Option<u64> {
        let mut best: Option<(f32, u64)> = None;
        let check = |props: &[PropPlacement], best: &mut Option<(f32, u64)>| {
            for p in props {
                let (bmin, bmax) = self
                    .prop_meshes
                    .get(&p.asset_id)
                    .map(|m| (m.aabb_min, m.aabb_max))
                    .unwrap_or((Vec3::new(-0.8, 0.0, -0.8), Vec3::new(0.8, 2.2, 0.8)));

                let inv = prop_model_matrix(p).inverse();
                let lo = inv.transform_point3(ray_orig);
                let ld = inv.transform_vector3(ray_dir);
                if let Some(t) = intersect_aabb(lo, ld, bmin, bmax) {
                    if best.map_or(true, |(bt, _)| t < bt) {
                        *best = Some((t, p.id));
                    }
                }
            }
        };

        if self.active_area.kind == AreaKind::Outside {
            for tile in self.outside_tiles.values() {
                check(&tile.vis.props, &mut best);
            }
        } else if let Some(inside) = &self.inside_area {
            check(&inside.vis.props, &mut best);
        }
        best.map(|(_, id)| id)
    }
}

fn intersect_aabb(orig: Vec3, dir: Vec3, bmin: Vec3, bmax: Vec3) -> Option<f32> {
    let mut tmin = 0.0f32;
    let mut tmax = f32::MAX;
    for axis in 0..3 {
        let o = orig[axis];
        let d = dir[axis];
        let min_v = bmin[axis];
        let max_v = bmax[axis];
        if d.abs() < 1e-6 {
            if o < min_v || o > max_v {
                return None;
            }
        } else {
            let inv = 1.0 / d;
            let mut t1 = (min_v - o) * inv;
            let mut t2 = (max_v - o) * inv;
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            tmin = tmin.max(t1);
            tmax = tmax.min(t2);
            if tmin > tmax {
                return None;
            }
        }
    }
    Some(tmin)
}
