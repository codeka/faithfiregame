use glam::{Mat4, Vec2, Vec3};
use shared::entity::{EntityOverrides, PlacedEntity, VisualConfig, intersect_local_aabb};
use shared::world::{AreaKind, TILE_SIZE};

use crate::camera::Camera;
use crate::input::InputState;

use super::gizmo::{
    GizmoAxis, GizmoDragState, begin_rotate_drag, compute_drag_plane_normal, pick_gizmo_axis,
    pick_rotate_gizmo_axis, project_ray_on_axis, update_rotate_drag,
};
use super::state::EditorState;
use super::types::EntitySubMode;

impl EditorState {
    pub(super) fn handle_entity_tool(
        &mut self,
        camera: &Camera,
        input: &InputState,
        now_ms: f64,
    ) {
        match self.entity_submode {
            EntitySubMode::Place => {
                if input.left_just_pressed {
                    if self.active_entity_template_id.is_empty()
                        || !self
                            .entity_templates
                            .contains_key(&self.active_entity_template_id)
                    {
                        return;
                    }
                    if let Some(hit) = self.hover_world {
                        let id = self.next_entity_placement_id;
                        self.next_entity_placement_id += 1;
                        let placed = PlacedEntity {
                            id,
                            template_id: self.active_entity_template_id.clone(),
                            position: hit.to_array(),
                            rotation_y: self.entity_preview_yaw,
                            overrides: EntityOverrides::default(),
                        };
                        if self.insert_placed_entity(placed) {
                            self.selected_entity_id = Some(id);
                            self.mark_unsaved(now_ms);
                        }
                    }
                }
            }
            EntitySubMode::Move => {
                if input.left_just_pressed {
                    if let Some((orig, dir)) = camera.screen_ray(
                        input.mouse_x,
                        input.mouse_y,
                        input.canvas_width,
                        input.canvas_height,
                    ) {
                        self.selected_entity_id = self.raycast_placed_entity(orig, dir);
                        self.dragging_entity = self.selected_entity_id.is_some();
                    }
                }
                if input.left_mouse_down && self.dragging_entity {
                    if let (Some(id), Some(hit)) = (self.selected_entity_id, self.hover_world) {
                        if self.move_placed_entity_to(id, hit) {
                            self.mark_unsaved(now_ms);
                        }
                    }
                }
            }
            EntitySubMode::FreeMove => {
                let ray_opt = camera.screen_ray(
                    input.mouse_x,
                    input.mouse_y,
                    input.canvas_width,
                    input.canvas_height,
                );

                if input.left_just_pressed {
                    if let Some((orig, dir)) = ray_opt {
                        let selected_pos = self
                            .find_selected_placed_entity()
                            .map(|e| Vec3::from(e.position));
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
                            self.selected_entity_id = self.raycast_placed_entity(orig, dir);
                            self.gizmo_drag = None;
                            self.hovered_gizmo_axis = None;
                        }
                    }
                }

                if let (Some(id), Some(drag), Some((orig, dir))) = (
                    input
                        .left_mouse_down
                        .then_some(self.selected_entity_id)
                        .flatten(),
                    self.gizmo_drag,
                    ray_opt,
                ) {
                    if let Some(s_now) = project_ray_on_axis(
                        orig,
                        dir,
                        drag.origin,
                        drag.axis.unit_dir(),
                        drag.plane_normal,
                    ) {
                        let delta = (s_now - drag.start_s).clamp(-200.0, 200.0);
                        let mut new_pos = drag.origin + drag.axis.unit_dir() * delta;
                        new_pos.y = new_pos.y.clamp(-64.0, 256.0);
                        if self.move_placed_entity_to(id, new_pos) {
                            self.mark_unsaved(now_ms);
                        }
                    }
                }

                if let Some(drag) = self.gizmo_drag {
                    self.hovered_gizmo_axis = Some(drag.axis);
                } else {
                    let selected_pos = self
                        .find_selected_placed_entity()
                        .map(|e| Vec3::from(e.position));
                    self.hovered_gizmo_axis = match (selected_pos, ray_opt) {
                        (Some(pos), Some((orig, dir))) => pick_gizmo_axis(camera, pos, orig, dir),
                        _ => None,
                    };
                }
            }
            EntitySubMode::Rotate => {
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
                            .find_selected_placed_entity()
                            .map(|e| (Vec3::from(e.position), [0.0, e.rotation_y, 0.0]));
                        let hit_axis = selected
                            .and_then(|(pos, _)| pick_rotate_gizmo_axis(camera, pos, orig, dir))
                            .filter(|axis| *axis == GizmoAxis::Y);
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
                            self.selected_entity_id = self.raycast_placed_entity(orig, dir);
                            self.rotate_drag = None;
                            self.hovered_rotate_axis = None;
                        }
                    }
                }

                if input.left_mouse_down {
                    if let (Some(id), Some(mut drag), Some((orig, dir))) =
                        (self.selected_entity_id, self.rotate_drag, ray_opt)
                    {
                        if let Some(next_rot) = update_rotate_drag(&mut drag, orig, dir, mouse_px) {
                            if self.modify_placed_entity(id, |e| e.rotation_y = next_rot[1]) {
                                self.mark_unsaved(now_ms);
                            }
                        }
                        self.rotate_drag = Some(drag);
                    }
                }

                if let Some(drag) = self.rotate_drag {
                    self.hovered_rotate_axis = Some(drag.axis);
                } else {
                    let selected_pos = self
                        .find_selected_placed_entity()
                        .map(|e| Vec3::from(e.position));
                    self.hovered_rotate_axis = match (selected_pos, ray_opt) {
                        (Some(pos), Some((orig, dir))) => {
                            pick_rotate_gizmo_axis(camera, pos, orig, dir)
                                .filter(|axis| *axis == GizmoAxis::Y)
                        }
                        _ => None,
                    };
                }
            }
        }
    }

    pub(super) fn handle_entity_hotkeys(&mut self, input: &InputState, dt: f32, now_ms: f64) {
        let rot_speed = 2.0 * dt;
        let mut delta_rot = 0.0;
        if input.is_key_down("KeyQ") {
            delta_rot -= rot_speed;
        }
        if input.is_key_down("KeyE") {
            delta_rot += rot_speed;
        }

        match self.entity_submode {
            EntitySubMode::Place => {
                if delta_rot.abs() > 0.0 {
                    self.entity_preview_yaw =
                        (self.entity_preview_yaw + delta_rot).rem_euclid(std::f32::consts::TAU);
                }
            }
            EntitySubMode::Move | EntitySubMode::FreeMove | EntitySubMode::Rotate => {
                if delta_rot.abs() > 0.0 {
                    if let Some(id) = self.selected_entity_id {
                        if self.modify_placed_entity(id, |e| {
                            e.rotation_y =
                                (e.rotation_y + delta_rot).rem_euclid(std::f32::consts::TAU);
                        }) {
                            self.mark_unsaved(now_ms);
                        }
                    }
                }
                if input.is_key_down("Delete") || input.is_key_down("Backspace") {
                    self.delete_selected_entity(now_ms);
                }
            }
        }
    }

    pub fn insert_placed_entity(&mut self, placed: PlacedEntity) -> bool {
        let inserted = if self.active_area.kind == AreaKind::Outside {
            let tx = (placed.position[0] / TILE_SIZE as f32).floor() as i32;
            let ty = (placed.position[2] / TILE_SIZE as f32).floor() as i32;
            if let Some(tile) = self.outside_tiles.get_mut(&(tx, ty)) {
                tile.entities.push(placed);
                tile.entities_dirty = true;
                true
            } else {
                false
            }
        } else if let Some(inside) = &mut self.inside_area {
            inside.entities.push(placed);
            inside.entities_dirty = true;
            true
        } else {
            false
        };
        if inserted {
            self.ensure_entity_assets_queued();
        }
        inserted
    }

    fn move_placed_entity_to(&mut self, entity_id: u64, pos: Vec3) -> bool {
        if self.active_area.kind == AreaKind::Outside {
            let new_tx = (pos.x / TILE_SIZE as f32).floor() as i32;
            let new_ty = (pos.z / TILE_SIZE as f32).floor() as i32;
            if !self.outside_tiles.contains_key(&(new_tx, new_ty)) {
                return false;
            }
            let mut found_old: Option<(i32, i32, usize)> = None;
            for (&key, tile) in &self.outside_tiles {
                if let Some(idx) = tile.entities.iter().position(|e| e.id == entity_id) {
                    found_old = Some((key.0, key.1, idx));
                    break;
                }
            }
            let Some((old_tx, old_ty, idx)) = found_old else {
                return false;
            };
            if old_tx == new_tx && old_ty == new_ty {
                if let Some(tile) = self.outside_tiles.get_mut(&(old_tx, old_ty)) {
                    tile.entities[idx].position = pos.to_array();
                    tile.entities_dirty = true;
                }
            } else {
                let mut moved = self
                    .outside_tiles
                    .get_mut(&(old_tx, old_ty))
                    .unwrap()
                    .entities
                    .remove(idx);
                self.outside_tiles
                    .get_mut(&(old_tx, old_ty))
                    .unwrap()
                    .entities_dirty = true;
                moved.position = pos.to_array();
                let dst = self.outside_tiles.get_mut(&(new_tx, new_ty)).unwrap();
                dst.entities.push(moved);
                dst.entities_dirty = true;
            }
            true
        } else if let Some(inside) = &mut self.inside_area {
            if let Some(entity) = inside.entities.iter_mut().find(|e| e.id == entity_id) {
                entity.position = pos.to_array();
                inside.entities_dirty = true;
                return true;
            }
            false
        } else {
            false
        }
    }

    fn modify_placed_entity(
        &mut self,
        entity_id: u64,
        mut f: impl FnMut(&mut PlacedEntity),
    ) -> bool {
        if self.active_area.kind == AreaKind::Outside {
            for tile in self.outside_tiles.values_mut() {
                if let Some(entity) = tile.entities.iter_mut().find(|e| e.id == entity_id) {
                    f(entity);
                    tile.entities_dirty = true;
                    return true;
                }
            }
            false
        } else if let Some(inside) = &mut self.inside_area {
            if let Some(entity) = inside.entities.iter_mut().find(|e| e.id == entity_id) {
                f(entity);
                inside.entities_dirty = true;
                return true;
            }
            false
        } else {
            false
        }
    }

    pub(super) fn find_selected_placed_entity(&self) -> Option<&PlacedEntity> {
        let id = self.selected_entity_id?;
        if self.active_area.kind == AreaKind::Outside {
            for tile in self.outside_tiles.values() {
                if let Some(entity) = tile.entities.iter().find(|e| e.id == id) {
                    return Some(entity);
                }
            }
            None
        } else {
            self.inside_area
                .as_ref()
                .and_then(|inside| inside.entities.iter().find(|e| e.id == id))
        }
    }

    pub fn update_selected_placed_entity(&mut self, mut updated: PlacedEntity, now_ms: f64) {
        let Some(selected_id) = self.selected_entity_id else {
            return;
        };
        updated.id = selected_id;
        let replaced = self.modify_placed_entity(selected_id, |slot| {
            *slot = updated.clone();
        });
        if replaced {
            self.ensure_entity_assets_queued();
            self.mark_unsaved(now_ms);
        }
    }

    pub fn snap_selected_entity_to_terrain(&mut self, now_ms: f64) {
        let Some((id, pos)) = self
            .find_selected_placed_entity()
            .map(|e| (e.id, Vec3::from(e.position)))
        else {
            return;
        };
        let Some(ground_y) = self.terrain_height_at(pos.x, pos.z) else {
            return;
        };
        if self.move_placed_entity_to(id, Vec3::new(pos.x, ground_y, pos.z)) {
            self.mark_unsaved(now_ms);
        }
    }

    pub fn duplicate_selected_entity(&mut self, now_ms: f64) {
        let Some(src) = self.find_selected_placed_entity().cloned() else {
            return;
        };
        let mut nx = src.position[0] + 1.4;
        let mut nz = src.position[2] + 1.4;
        if self.active_area.kind == AreaKind::Outside {
            let cand_tx = (nx / TILE_SIZE as f32).floor() as i32;
            let cand_ty = (nz / TILE_SIZE as f32).floor() as i32;
            if !self.outside_tiles.contains_key(&(cand_tx, cand_ty)) {
                nx = src.position[0] - 1.4;
                nz = src.position[2] - 1.4;
            }
        } else if let Some(inside) = &self.inside_area {
            let max_x = (inside.vis.width as f32 - 0.5).max(0.5);
            let max_z = (inside.vis.length as f32 - 0.5).max(0.5);
            nx = nx.clamp(0.5, max_x);
            nz = nz.clamp(0.5, max_z);
        }
        let ny = self.terrain_height_at(nx, nz).unwrap_or(src.position[1]);

        let new_id = self.next_entity_placement_id;
        self.next_entity_placement_id += 1;
        let dup = PlacedEntity {
            id: new_id,
            template_id: src.template_id,
            position: [nx, ny, nz],
            rotation_y: src.rotation_y,
            overrides: src.overrides,
        };
        if self.insert_placed_entity(dup) {
            self.selected_entity_id = Some(new_id);
            self.mark_unsaved(now_ms);
        }
    }

    pub fn delete_selected_entity(&mut self, now_ms: f64) {
        let Some(entity_id) = self.selected_entity_id.take() else {
            return;
        };
        if self.active_area.kind == AreaKind::Outside {
            for tile in self.outside_tiles.values_mut() {
                let before = tile.entities.len();
                tile.entities.retain(|e| e.id != entity_id);
                if tile.entities.len() != before {
                    tile.entities_dirty = true;
                    self.mark_unsaved(now_ms);
                    break;
                }
            }
        } else if let Some(inside) = &mut self.inside_area {
            let before = inside.entities.len();
            inside.entities.retain(|e| e.id != entity_id);
            if inside.entities.len() != before {
                inside.entities_dirty = true;
                self.mark_unsaved(now_ms);
            }
        }
    }

    fn raycast_placed_entity(&self, ray_orig: Vec3, ray_dir: Vec3) -> Option<u64> {
        let mut best: Option<(f32, u64)> = None;
        let check = |entities: &[PlacedEntity], best: &mut Option<(f32, u64)>| {
            for e in entities {
                let resolved = e.resolve(self.entity_templates.get(&e.template_id));
                let mut closest_t: Option<f32> = None;

                let inv_entity = Mat4::from_rotation_translation(
                    glam::Quat::from_rotation_y(e.rotation_y),
                    Vec3::from(e.position),
                )
                .inverse();
                let lo = inv_entity.transform_point3(ray_orig);
                let ld = inv_entity.transform_vector3(ray_dir);

                match &resolved.visual {
                    Some(VisualConfig::Prop {
                        asset_id, scale, ..
                    }) => {
                        let (bmin, bmax) = self
                            .prop_meshes
                            .get(asset_id)
                            .map(|m| (m.aabb_min, m.aabb_max))
                            .unwrap_or((Vec3::new(-0.8, 0.0, -0.8), Vec3::new(0.8, 2.2, 0.8)));
                        let inv_prop = (Mat4::from_rotation_translation(
                            glam::Quat::from_rotation_y(e.rotation_y),
                            Vec3::from(e.position),
                        ) * Mat4::from_scale(Vec3::from(*scale)))
                        .inverse();
                        let plo = inv_prop.transform_point3(ray_orig);
                        let pld = inv_prop.transform_vector3(ray_dir);
                        if let Some(t) = intersect_local_aabb(plo, pld, bmin, bmax) {
                            closest_t = Some(t);
                        }
                    }
                    Some(VisualConfig::Character { pose_clip, .. }) => {
                        let is_posed = pose_clip
                            .as_deref()
                            .is_some_and(|s| !s.trim().is_empty());
                        let (bmin, bmax) = if is_posed {
                            (Vec3::new(-0.85, 0.0, -0.85), Vec3::new(0.85, 1.0, 0.85))
                        } else {
                            (Vec3::new(-0.45, 0.0, -0.45), Vec3::new(0.45, 1.85, 0.45))
                        };
                        if let Some(t) = intersect_local_aabb(lo, ld, bmin, bmax) {
                            closest_t = Some(t);
                        }
                    }
                    None => {}
                }

                if let Some(trig) = &resolved.trigger {
                    if let Some(t) = trig.raycast(e.position, e.rotation_y, ray_orig, ray_dir) {
                        closest_t = Some(closest_t.map_or(t, |cur| cur.min(t)));
                    }
                }

                if resolved.visual.is_none() && resolved.trigger.is_none() {
                    if let Some(t) = intersect_local_aabb(
                        lo,
                        ld,
                        Vec3::new(-0.5, 0.0, -0.5),
                        Vec3::new(0.5, 1.0, 0.5),
                    ) {
                        closest_t = Some(t);
                    }
                }

                if let Some(t) = closest_t {
                    if best.is_none_or(|(bt, _)| t < bt) {
                        *best = Some((t, e.id));
                    }
                }
            }
        };

        if self.active_area.kind == AreaKind::Outside {
            for tile in self.outside_tiles.values() {
                check(&tile.entities, &mut best);
            }
        } else if let Some(inside) = &self.inside_area {
            check(&inside.entities, &mut best);
        }
        best.map(|(_, id)| id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::types::EditorTool;
    use shared::entity::{ComponentOverride, EntityTemplate, PropertyBag, PropertyValue, TriggerConfig};

    fn make_test_template() -> EntityTemplate {
        let mut props = PropertyBag::new();
        props.set("quest_id", PropertyValue::String("wake".to_owned()));
        EntityTemplate {
            id: "guard".to_owned(),
            name: "Town Guard".to_owned(),
            properties: props,
            visual: None,
            movement: None,
            trigger: Some(TriggerConfig::default()),
            script: None,
            inventory: None,
            lootable: None,
        }
    }

    #[test]
    fn entity_place_mode_qe_rotates_preview_and_ignores_delete() {
        let mut ed = EditorState::new();
        ed.tool = EditorTool::Entities;
        ed.entity_submode = EntitySubMode::Place;
        ed.selected_entity_id = Some(42);

        let mut input = InputState::default();
        input.keys.insert("KeyE".to_owned());
        ed.handle_entity_hotkeys(&input, 0.25, 1000.0);
        assert!((ed.entity_preview_yaw - 0.5).abs() < 1e-5);
        assert_eq!(ed.save_state, "Saved");

        input.keys.clear();
        input.keys.insert("Delete".to_owned());
        ed.handle_entity_hotkeys(&input, 0.1, 1100.0);
        assert_eq!(ed.selected_entity_id, Some(42));
    }

    #[test]
    fn non_place_entity_submodes_rotate_or_delete_and_update_overrides() {
        let mut ed = EditorState::new();
        ed.tool = EditorTool::Entities;
        ed.entity_submode = EntitySubMode::Move;
        ed.set_entity_templates(vec![make_test_template()]);

        let placed = PlacedEntity {
            id: 1001,
            template_id: "guard".to_owned(),
            position: [5.0, 0.0, 5.0],
            rotation_y: 0.0,
            overrides: EntityOverrides::default(),
        };
        ed.inside_area = None;
        // Outside tile insertion works only when tile exists; validate direct update on entity_templates & non-place delete clearing.
        ed.selected_entity_id = Some(placed.id);
        let mut input = InputState::default();
        input.keys.insert("Delete".to_owned());
        ed.handle_entity_hotkeys(&input, 0.1, 1000.0);
        assert_eq!(ed.selected_entity_id, None);

        // Verify template override resolution via EditorState's stored template map
        let mut custom = placed;
        custom.overrides.name = Some("Captain".to_owned());
        custom.overrides.trigger = ComponentOverride::Remove;
        let resolved = custom.resolve(ed.entity_templates.get("guard"));
        assert_eq!(resolved.name, "Captain");
        assert!(resolved.trigger.is_none());
        assert_eq!(resolved.properties.get_str("quest_id"), Some("wake"));
    }
}
