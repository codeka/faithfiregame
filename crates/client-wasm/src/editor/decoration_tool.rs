use glam::{Vec2, Vec3};
use shared::world::{AreaKind, PropPlacement, TILE_SIZE};

use crate::camera::Camera;
use crate::input::InputState;
use crate::render::prop_model_matrix;

use super::gizmo::{
    GizmoDragState, begin_rotate_drag, compute_drag_plane_normal, pick_gizmo_axis,
    pick_rotate_gizmo_axis, project_ray_on_axis, update_rotate_drag,
};
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
                            rotation: self.preview_rotation,
                            scale: [self.preview_scale; 3],
                            cast_shadow: self.preview_cast_shadow,
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
            DecorationSubMode::FreeMove => {
                let ray_opt = camera.screen_ray(
                    input.mouse_x,
                    input.mouse_y,
                    input.canvas_width,
                    input.canvas_height,
                );

                if input.left_just_pressed {
                    if let Some((orig, dir)) = ray_opt {
                        let selected_pos = self.find_selected_prop().map(|p| Vec3::from(p.position));
                        let hit_axis = selected_pos
                            .and_then(|pos| pick_gizmo_axis(camera, pos, orig, dir));
                        if let (Some(pos), Some(axis)) = (selected_pos, hit_axis) {
                            let plane_normal = compute_drag_plane_normal(camera, pos, axis);
                            if let Some(start_s) =
                                project_ray_on_axis(orig, dir, pos, axis.unit_dir(), plane_normal)
                            {
                                self.gizmo_drag = Some(GizmoDragState {
                                    axis,
                                    origin: pos,
                                    plane_normal,
                                    start_s,
                                });
                                self.hovered_gizmo_axis = Some(axis);
                            }
                        } else {
                            self.selected_prop_id = self.raycast_prop(orig, dir);
                            self.gizmo_drag = None;
                            self.hovered_gizmo_axis = None;
                        }
                    }
                }

                if let (Some(id), Some(drag), Some((orig, dir))) = (
                    input.left_mouse_down.then_some(self.selected_prop_id).flatten(),
                    self.gizmo_drag,
                    ray_opt,
                ) {
                    if let Some(s_now) =
                        project_ray_on_axis(orig, dir, drag.origin, drag.axis.unit_dir(), drag.plane_normal)
                    {
                        let delta = (s_now - drag.start_s).clamp(-200.0, 200.0);
                        let mut new_pos = drag.origin + drag.axis.unit_dir() * delta;
                        new_pos.y = new_pos.y.clamp(-64.0, 256.0);
                        if self.move_prop_to(id, new_pos) {
                            self.mark_unsaved(now_ms);
                        }
                    }
                }

                if let Some(drag) = self.gizmo_drag {
                    self.hovered_gizmo_axis = Some(drag.axis);
                } else {
                    let selected_pos = self.find_selected_prop().map(|p| Vec3::from(p.position));
                    self.hovered_gizmo_axis = match (selected_pos, ray_opt) {
                        (Some(pos), Some((orig, dir))) => pick_gizmo_axis(camera, pos, orig, dir),
                        _ => None,
                    };
                }
            }
            DecorationSubMode::Rotate => {
                let ray_opt = camera.screen_ray(
                    input.mouse_x,
                    input.mouse_y,
                    input.canvas_width,
                    input.canvas_height,
                );
                let mouse_px = Vec2::new(input.mouse_x, input.mouse_y);

                if input.left_just_pressed {
                    if let Some((orig, dir)) = ray_opt {
                        let selected = self
                            .find_selected_prop()
                            .map(|p| (Vec3::from(p.position), p.rotation));
                        let hit_axis = selected
                            .and_then(|(pos, _)| pick_rotate_gizmo_axis(camera, pos, orig, dir));
                        if let (Some((pos, start_rot)), Some(axis)) = (selected, hit_axis) {
                            self.rotate_drag = Some(begin_rotate_drag(
                                camera,
                                pos,
                                axis,
                                orig,
                                dir,
                                mouse_px,
                                input.canvas_width,
                                input.canvas_height,
                                start_rot,
                            ));
                            self.hovered_rotate_axis = Some(axis);
                        } else {
                            self.selected_prop_id = self.raycast_prop(orig, dir);
                            self.rotate_drag = None;
                            self.hovered_rotate_axis = None;
                        }
                    }
                }

                if input.left_mouse_down {
                    if let (Some(id), Some(mut drag), Some((orig, dir))) =
                        (self.selected_prop_id, self.rotate_drag, ray_opt)
                    {
                        if let Some(next_rot) = update_rotate_drag(&mut drag, orig, dir, mouse_px) {
                            if self.modify_prop(id, |p| p.rotation = next_rot) {
                                self.mark_unsaved(now_ms);
                            }
                        }
                        self.rotate_drag = Some(drag);
                    }
                }

                if let Some(drag) = self.rotate_drag {
                    self.hovered_rotate_axis = Some(drag.axis);
                } else {
                    let selected_pos = self.find_selected_prop().map(|p| Vec3::from(p.position));
                    self.hovered_rotate_axis = match (selected_pos, ray_opt) {
                        (Some(pos), Some((orig, dir))) => {
                            pick_rotate_gizmo_axis(camera, pos, orig, dir)
                        }
                        _ => None,
                    };
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

    pub(super) fn handle_decoration_hotkeys(&mut self, input: &InputState, dt: f32, now_ms: f64) {
        let rot_speed = 2.0 * dt;
        let mut delta_rot = 0.0;
        if input.is_key_down("KeyQ") {
            delta_rot -= rot_speed;
        }
        if input.is_key_down("KeyE") {
            delta_rot += rot_speed;
        }

        match self.decoration_submode {
            DecorationSubMode::Place => {
                if delta_rot.abs() > 0.0 {
                    self.preview_rotation[1] =
                        (self.preview_rotation[1] + delta_rot).rem_euclid(std::f32::consts::TAU);
                }
            }
            DecorationSubMode::Move
            | DecorationSubMode::FreeMove
            | DecorationSubMode::Rotate => {
                if delta_rot.abs() > 0.0 {
                    if let Some(id) = self.selected_prop_id {
                        self.rotate_prop_by(id, delta_rot, now_ms);
                    }
                }
                if input.is_key_down("Delete") || input.is_key_down("Backspace") {
                    self.delete_selected_prop(now_ms);
                }
            }
        }
    }

    pub(super) fn rotate_prop_by(&mut self, prop_id: u64, delta_rad: f32, now_ms: f64) {
        if self.modify_prop(prop_id, |p| {
            p.rotation[1] = (p.rotation[1] + delta_rad).rem_euclid(std::f32::consts::TAU);
        }) {
            self.mark_unsaved(now_ms);
        }
    }

    fn edit_selected_or_preview(
        &mut self,
        now_ms: f64,
        mutate_prop: impl FnMut(&mut PropPlacement),
        mutate_preview: impl FnOnce(&mut Self),
    ) {
        if self.decoration_submode != DecorationSubMode::Place {
            if let Some(id) = self.selected_prop_id {
                if self.modify_prop(id, mutate_prop) {
                    self.mark_unsaved(now_ms);
                }
                return;
            }
        }
        mutate_preview(self);
    }

    pub fn set_selected_or_preview_rotation_axis_deg(
        &mut self,
        axis: usize,
        degrees: f32,
        now_ms: f64,
    ) {
        if axis >= 3 {
            return;
        }
        let rad = degrees.to_radians().rem_euclid(std::f32::consts::TAU);
        self.edit_selected_or_preview(
            now_ms,
            |p| p.rotation[axis] = rad,
            |st| st.preview_rotation[axis] = rad,
        );
    }

    pub fn reset_selected_or_preview_rotation(&mut self, now_ms: f64) {
        self.edit_selected_or_preview(
            now_ms,
            |p| p.rotation = [0.0; 3],
            |st| st.preview_rotation = [0.0; 3],
        );
    }

    pub fn set_selected_or_preview_scale(&mut self, scale: f32, now_ms: f64) {
        let s = scale.clamp(0.25, 4.0);
        self.edit_selected_or_preview(
            now_ms,
            |p| p.scale = [s, s, s],
            |st| st.preview_scale = s,
        );
    }

    pub fn set_selected_or_preview_cast_shadow(&mut self, cast_shadow: bool, now_ms: f64) {
        self.edit_selected_or_preview(
            now_ms,
            |p| p.cast_shadow = cast_shadow,
            |st| st.preview_cast_shadow = cast_shadow,
        );
    }

    pub fn snap_selected_prop_to_terrain(&mut self, now_ms: f64) {
        let Some((id, pos)) = self.find_selected_prop().map(|p| (p.id, Vec3::from(p.position)))
        else {
            return;
        };
        let Some(ground_y) = self.terrain_height_at(pos.x, pos.z) else {
            return;
        };
        if self.move_prop_to(id, Vec3::new(pos.x, ground_y, pos.z)) {
            self.mark_unsaved(now_ms);
        }
    }

    pub fn duplicate_selected_prop(&mut self, now_ms: f64) {
        let Some(src) = self.find_selected_prop().cloned() else {
            return;
        };

        let horiz_radius = self
            .prop_meshes
            .get(&src.asset_id)
            .map(|m| {
                (m.aabb_max.x - m.aabb_min.x)
                    .max(m.aabb_max.z - m.aabb_min.z)
                    * 0.5
                    * src.scale[0]
            })
            .unwrap_or(1.2);

        let seed = now_ms.to_bits() ^ self.next_prop_id.wrapping_mul(0xD1B5_4A32_D192_ED03);
        let (dx, dz) = compute_duplicate_offset(seed, horiz_radius);

        let src_ground_y = self
            .terrain_height_at(src.position[0], src.position[2])
            .unwrap_or(src.position[1]);
        let rel_y = src.position[1] - src_ground_y;

        let (mut nx, mut nz) = (src.position[0] + dx, src.position[2] + dz);
        if self.active_area.kind == AreaKind::Outside {
            let cand_tx = (nx / TILE_SIZE as f32).floor() as i32;
            let cand_ty = (nz / TILE_SIZE as f32).floor() as i32;
            if !self.outside_tiles.contains_key(&(cand_tx, cand_ty)) {
                nx = src.position[0] - dx;
                nz = src.position[2] - dz;
            }
        } else if let Some(inside) = &self.inside_area {
            let max_x = (inside.vis.width as f32 - 0.5).max(0.5);
            let max_z = (inside.vis.length as f32 - 0.5).max(0.5);
            nx = nx.clamp(0.5, max_x);
            nz = nz.clamp(0.5, max_z);
        }

        let ny = self
            .terrain_height_at(nx, nz)
            .map(|gy| (gy + rel_y).clamp(-64.0, 256.0))
            .unwrap_or(src.position[1]);

        let new_id = self.next_prop_id;
        self.next_prop_id += 1;

        let dup = PropPlacement {
            id: new_id,
            asset_id: src.asset_id,
            position: [nx, ny, nz],
            rotation: src.rotation,
            scale: src.scale,
            cast_shadow: src.cast_shadow,
        };
        if self.insert_prop(dup) {
            self.selected_prop_id = Some(new_id);
            self.mark_unsaved(now_ms);
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

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn compute_duplicate_offset(seed: u64, base_radius: f32) -> (f32, f32) {
    let h1 = splitmix64(seed);
    let h2 = splitmix64(h1);
    let u_angle = (h1 >> 40) as f32 / ((1u64 << 24) as f32);
    let u_dist = (h2 >> 40) as f32 / ((1u64 << 24) as f32);
    let angle = u_angle * std::f32::consts::TAU;
    let min_d = (base_radius * 0.55).clamp(0.85, 2.2);
    let max_d = (base_radius * 0.95).clamp(1.55, 3.2);
    let dist = min_d + u_dist * (max_d - min_d);
    (angle.cos() * dist, angle.sin() * dist)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::types::EditorTool;

    #[test]
    fn duplicate_offset_stays_within_bounds_and_varies_across_ids() {
        let now_ms = 123_456.789_f64;
        let mut prev = None;
        for next_id in 1000u64..1010u64 {
            let seed = now_ms.to_bits() ^ next_id.wrapping_mul(0xD1B5_4A32_D192_ED03);
            let (dx, dz) = compute_duplicate_offset(seed, 1.5);
            let dist = (dx * dx + dz * dz).sqrt();
            assert!((0.84..=3.21).contains(&dist), "dist={dist} out of bounds");
            if let Some(p) = prev {
                assert_ne!((dx, dz), p, "offset should vary across prop ids");
            }
            prev = Some((dx, dz));
        }
    }

    #[test]
    fn place_mode_qe_rotates_preview_even_with_selected_prop_and_ignores_delete() {
        let mut ed = EditorState::new();
        ed.tool = EditorTool::Decoration;
        ed.decoration_submode = DecorationSubMode::Place;
        ed.selected_prop_id = Some(42);

        let mut input = InputState::default();
        input.keys.insert("KeyE".to_owned());
        ed.handle_decoration_hotkeys(&input, 0.25, 1000.0);
        assert!(
            (ed.preview_rotation[1] - 0.5).abs() < 1e-5,
            "E should rotate preview_rotation[1] forward in Place mode even with selected_prop_id"
        );
        assert_eq!(ed.save_state, "Saved");

        input.keys.clear();
        input.keys.insert("KeyQ".to_owned());
        ed.handle_decoration_hotkeys(&input, 0.5, 1100.0);
        let expected_wrap = (0.5f32 - 1.0).rem_euclid(std::f32::consts::TAU);
        assert!(
            (ed.preview_rotation[1] - expected_wrap).abs() < 1e-5,
            "Q should rotate preview_rotation[1] left and wrap around TAU"
        );
        assert_eq!(ed.save_state, "Saved");

        // Delete/Backspace should not clear selection while in Place mode.
        input.keys.clear();
        input.keys.insert("Delete".to_owned());
        ed.handle_decoration_hotkeys(&input, 0.1, 1200.0);
        assert_eq!(ed.selected_prop_id, Some(42));

        // Explicit preview setters also update preview state while in Place mode.
        ed.set_selected_or_preview_rotation_axis_deg(1, 90.0, 1300.0);
        ed.set_selected_or_preview_scale(1.5, 1300.0);
        ed.set_selected_or_preview_cast_shadow(false, 1300.0);
        assert!((ed.preview_rotation[1] - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert!((ed.preview_scale - 1.5).abs() < 1e-5);
        assert!(!ed.preview_cast_shadow);
        assert_eq!(ed.save_state, "Saved");
    }

    #[test]
    fn non_place_modes_do_not_mutate_preview_on_qe_and_allow_delete() {
        for submode in [
            DecorationSubMode::Move,
            DecorationSubMode::FreeMove,
            DecorationSubMode::Rotate,
        ] {
            let mut ed = EditorState::new();
            ed.tool = EditorTool::Decoration;
            ed.decoration_submode = submode;
            ed.selected_prop_id = Some(42);
            ed.preview_rotation[1] = 1.25;

            let mut input = InputState::default();
            input.keys.insert("KeyE".to_owned());
            ed.handle_decoration_hotkeys(&input, 0.25, 1000.0);
            assert!(
                (ed.preview_rotation[1] - 1.25).abs() < 1e-5,
                "preview_rotation should remain unchanged in {submode:?}"
            );

            input.keys.clear();
            input.keys.insert("Delete".to_owned());
            ed.handle_decoration_hotkeys(&input, 0.1, 1100.0);
            assert_eq!(
                ed.selected_prop_id, None,
                "Delete should clear selected_prop_id in {submode:?}"
            );
        }
    }
}
