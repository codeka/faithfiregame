use std::collections::HashMap;
use std::f32::consts::TAU;

use glam::{EulerRot, Mat4, Quat, Vec3, Vec4};
use shared::character::{
    CharacterCatalog, CharacterDefinition, CharacterPartDef, CompactCharacterAppearance,
    PartAttachment, SkeletonDef,
};
use shared::entity::{EntityCapabilities, EntityId, EntityVisual};
use shared::protocol::ServerMessage;
use shared::world::{
    AreaKind, AssetEntry, InsideVisData, PropPlacement, RoadSpline, TILE_SIZE, TILE_VERTICES,
    TileVisData, is_local_asset_id, local_asset_http_path, local_asset_slug, prettify_asset_slug,
};
use web_sys::HtmlCanvasElement;

use crate::camera::{Camera, CameraController, GameplayCameraController};
use crate::character::{
    AnimationClip, ParsedSkeletonRig, SkeletonPose, evaluate_pose, parse_character_glb,
    parse_glb_animations, parse_skeleton_glb,
};
use crate::glb_loader::parse_glb;
use crate::input::InputState;
use crate::net::NetClient;
use crate::render::{
    BrushOverlay, CharacterMeshGpu, PropMeshGpu, Renderer, RoadMeshGpu, TerrainMeshGpu,
};

use super::state::{
    ClientEntityRenderState, ClientEntityVisualState, GameClientState, GameInsideState, GamePhase,
    GameTileState,
};

impl GameClientState {
    pub fn clear_world_meshes(&mut self, gl: &glow::Context) {
        for (_, tile) in self.outside_tiles.drain() {
            for mesh in tile.road_meshes.into_values() {
                mesh.destroy(gl);
            }
            tile.gpu.destroy(gl);
        }
        if let Some(inside) = self.inside_area.take() {
            for mesh in inside.road_meshes.into_values() {
                mesh.destroy(gl);
            }
            inside.gpu.destroy(gl);
        }
        let stale_local: Vec<String> = self
            .prop_meshes
            .keys()
            .filter(|id| is_local_asset_id(id))
            .cloned()
            .collect();
        for id in stale_local {
            if let Some(mesh) = self.prop_meshes.remove(&id) {
                mesh.destroy(gl);
            }
        }
        self.requested_assets.retain(|id| !is_local_asset_id(id));
        self.pending_asset_fetches
            .retain(|entry| !is_local_asset_id(&entry.id));
        self.requested_tiles.clear();
        self.requested_inside = None;
    }

    pub fn handle_server_messages(
        &mut self,
        gl: &glow::Context,
        cam_ctrl: &mut GameplayCameraController,
        msgs: Vec<ServerMessage>,
    ) {
        for msg in msgs {
            match msg {
                ServerMessage::HandshakeAccepted { player_id } => {
                    self.local_player_id = Some(player_id);
                }
                ServerMessage::Unauthorized => {
                    self.clear_world_meshes(gl);
                    self.entities.clear();
                    self.local_player_id = None;
                    self.local_entity_id = None;
                    self.phase = GamePhase::Unauthorized;
                }
                ServerMessage::VersionMismatch { server_version } => {
                    self.clear_world_meshes(gl);
                    self.entities.clear();
                    self.local_entity_id = None;
                    self.phase = GamePhase::Error(format!(
                        "Client protocol outdated (server expects v{server_version}). Please reload."
                    ));
                }
                ServerMessage::EnterArea {
                    entity_id,
                    area_id,
                    area_name,
                    area_kind,
                    dimensions,
                    pos,
                    yaw,
                    visual,
                    fresh_cryo_wake,
                } => {
                    self.clear_world_meshes(gl);
                    self.entities.clear();
                    self.local_entity_id = Some(entity_id);
                    self.pending_tile_fetches.clear();
                    self.pending_inside_fetch = None;

                    self.area_id = area_id.clone();
                    self.area_name = area_name;
                    self.area_kind = area_kind;
                    self.dimensions = dimensions;
                    self.fresh_cryo_wake = fresh_cryo_wake;
                    self.wake_ready = false;
                    self.area_enter_elapsed_s = 0.0;
                    self.move_target_marker = None;
                    self.phase = GamePhase::LoadingArea;

                    self.ensure_entity_visual_requested(&area_id, &visual);

                    let spawn_pos = Vec3::from(pos);
                    self.entities.insert(
                        entity_id,
                        ClientEntityRenderState {
                            entity_id,
                            name: None,
                            capabilities: EntityCapabilities::PLAYER | EntityCapabilities::MOVABLE,
                            render_pos: spawn_pos,
                            target_pos: spawn_pos,
                            render_yaw: yaw,
                            target_yaw: yaw,
                            moving: false,
                            visual: client_visual_from_wire(visual),
                        },
                    );

                    if area_kind == AreaKind::Outside {
                        cam_ctrl.min_distance = 8.0;
                        cam_ctrl.max_distance = 58.0;
                        cam_ctrl.snap_to(spawn_pos, 45.0, 42.0, 22.0);
                        self.wake_cam_timer = None;
                        self.queue_outside_tiles_around(spawn_pos);
                    } else {
                        cam_ctrl.min_distance = 5.5;
                        cam_ctrl.max_distance = 32.0;
                        if fresh_cryo_wake {
                            cam_ctrl.snap_to(spawn_pos, 105.0, 30.0, 9.8);
                            self.wake_cam_timer = Some(0.0);
                        } else {
                            cam_ctrl.snap_to(spawn_pos, 105.0, 40.0, 13.5);
                            self.wake_cam_timer = None;
                        }
                        self.requested_inside = Some(area_id.clone());
                        self.pending_inside_fetch = Some(area_id);
                    }
                }
                ServerMessage::WorldSnapshot { entities } => {
                    let keep_ids: HashMap<EntityId, _> = entities
                        .into_iter()
                        .map(|snap| (snap.entity_id, snap))
                        .collect();
                    self.entities.retain(|id, _| keep_ids.contains_key(id));
                    let area_id = self.area_id.clone();
                    for (id, snap) in keep_ids {
                        self.ensure_entity_visual_requested(&area_id, &snap.visual);
                        let target_pos = Vec3::from(snap.pos);
                        if let Some(existing) = self.entities.get_mut(&id) {
                            existing.name = snap.name;
                            existing.capabilities = snap.capabilities;
                            existing.target_pos = target_pos;
                            existing.target_yaw = snap.yaw;
                            existing.moving = snap.moving;
                            match (&mut existing.visual, snap.visual) {
                                (
                                    ClientEntityVisualState::Character { appearance, .. },
                                    EntityVisual::Character(new_app),
                                ) => {
                                    *appearance = new_app;
                                }
                                (_, new_visual) => {
                                    existing.visual = client_visual_from_wire(new_visual);
                                }
                            }
                        } else {
                            self.entities.insert(
                                id,
                                ClientEntityRenderState {
                                    entity_id: id,
                                    name: snap.name,
                                    capabilities: snap.capabilities,
                                    render_pos: target_pos,
                                    target_pos,
                                    render_yaw: snap.yaw,
                                    target_yaw: snap.yaw,
                                    moving: snap.moving,
                                    visual: client_visual_from_wire(snap.visual),
                                },
                            );
                        }
                    }
                }
                ServerMessage::TileUpdated {
                    area_id,
                    tile_x,
                    tile_y,
                } => {
                    if self.area_id == area_id && self.area_kind == AreaKind::Outside {
                        self.pending_tile_fetches.push((area_id, tile_x, tile_y));
                    }
                }
                ServerMessage::AreaUpdated { area_id } => {
                    if self.area_id == area_id && self.area_kind != AreaKind::Outside {
                        self.pending_inside_fetch = Some(area_id);
                    }
                }
                ServerMessage::Pong(_) => {}
            }
        }
    }

    pub fn on_character_catalog_loaded(&mut self, catalog: CharacterCatalog) {
        self.character_catalog = Some(catalog);
        let appearances: Vec<CompactCharacterAppearance> = self
            .entities
            .values()
            .filter_map(|entity| match &entity.visual {
                ClientEntityVisualState::Character { appearance, .. } => Some(appearance.clone()),
                ClientEntityVisualState::Prop { .. } => None,
            })
            .collect();
        for app in &appearances {
            self.ensure_appearance_glbs_requested(app);
        }
    }

    pub fn on_character_glb_loaded(
        &mut self,
        gl: &glow::Context,
        path: String,
        bytes: &[u8],
    ) -> Result<(), String> {
        let Some(catalog) = &self.character_catalog else {
            return Ok(());
        };
        let is_rig = catalog.skeletons.iter().any(|s| s.rig_path == path);
        let is_anim = catalog
            .skeletons
            .iter()
            .any(|s| s.animation_paths.iter().any(|a| a == &path));
        let is_part = catalog.parts.iter().any(|p| p.path == path);

        if is_rig {
            let rig = parse_skeleton_glb(bytes)?;
            if let Some(ref parsed_base) = rig.base_mesh {
                let gpu = CharacterMeshGpu::from_parsed(gl, parsed_base)?;
                if let Some(old) = self.base_meshes.insert(path.clone(), gpu) {
                    old.destroy(gl);
                }
            }
            self.rigs.insert(path.clone(), rig);
        }

        if is_anim {
            let clips = parse_glb_animations(bytes)?;
            self.extra_anims.insert(path.clone(), clips);
        }

        if is_part || (!is_rig && !is_anim) {
            if let Ok(parsed) = parse_character_glb(bytes) {
                if !parsed.vertices.is_empty() && !parsed.indices.is_empty() {
                    let gpu = CharacterMeshGpu::from_parsed(gl, &parsed)?;
                    if let Some(old) = self.part_meshes.insert(path.clone(), gpu) {
                        old.destroy(gl);
                    }
                }
            }
        }

        Ok(())
    }

    pub fn on_prop_glb_loaded(
        &mut self,
        gl: &glow::Context,
        asset_id: String,
        bytes: &[u8],
    ) -> Result<(), String> {
        let parsed = parse_glb(bytes)?;
        let gpu = PropMeshGpu::from_parsed(gl, &parsed)?;
        if let Some(old) = self.prop_meshes.insert(asset_id, gpu) {
            old.destroy(gl);
        }
        Ok(())
    }

    pub fn on_outside_tile_loaded(
        &mut self,
        gl: &glow::Context,
        area_id: String,
        tx: i32,
        ty: i32,
        vis_opt: Option<TileVisData>,
    ) {
        if self.area_id != area_id || self.area_kind != AreaKind::Outside {
            return;
        }
        let Some(vis) = vis_opt else {
            return;
        };
        self.ensure_prop_assets_requested(&area_id, &vis.props);
        if let Ok(gpu) = TerrainMeshGpu::new(
            gl,
            TILE_SIZE,
            TILE_SIZE,
            &vis.heights,
            &vis.splat_ids,
            &vis.splat_weights,
        ) {
            gpu.upload_walkability(gl, |x, z| vis.walkable.get(x, z));
            let mut road_meshes = HashMap::new();
            for road in &vis.roads {
                if let Ok(mesh) = RoadMeshGpu::from_road(
                    gl,
                    road,
                    None,
                    None,
                    0,
                    self.is_road_material_soft(road.material),
                ) {
                    road_meshes.insert(road.id, mesh);
                }
            }
            if let Some(old) = self.outside_tiles.insert(
                (tx, ty),
                GameTileState {
                    vis,
                    gpu,
                    road_meshes,
                },
            ) {
                for m in old.road_meshes.into_values() {
                    m.destroy(gl);
                }
                old.gpu.destroy(gl);
            }
            let mut refresh = Vec::new();
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let key = (tx + dx, ty + dy);
                    if self.outside_tiles.contains_key(&key) {
                        refresh.push(key);
                    }
                }
            }
            self.refresh_outside_tile_meshes(gl, &refresh);
        }
    }

    pub fn on_inside_loaded(&mut self, gl: &glow::Context, area_id: String, vis: InsideVisData) {
        if self.area_id != area_id || self.area_kind == AreaKind::Outside {
            return;
        }
        self.ensure_prop_assets_requested(&area_id, &vis.props);
        if let Some(old) = self.inside_area.take() {
            for mesh in old.road_meshes.into_values() {
                mesh.destroy(gl);
            }
            old.gpu.destroy(gl);
        }
        if let Ok(gpu) = TerrainMeshGpu::new(
            gl,
            vis.width as usize,
            vis.length as usize,
            &vis.heights,
            &vis.splat_ids,
            &vis.splat_weights,
        ) {
            gpu.upload_walkability(gl, |x, z| vis.get_walkable(x, z));
            let mut road_meshes = HashMap::new();
            for road in &vis.roads {
                if let Ok(mesh) = RoadMeshGpu::from_road(
                    gl,
                    road,
                    None,
                    None,
                    0,
                    self.is_road_material_soft(road.material),
                ) {
                    road_meshes.insert(road.id, mesh);
                }
            }
            self.inside_area = Some(GameInsideState {
                vis,
                gpu,
                road_meshes,
            });
            self.refresh_inside_mesh(gl);
        }
    }

    pub fn update(
        &mut self,
        canvas: &HtmlCanvasElement,
        renderer: &mut Renderer,
        camera: &mut Camera,
        cam_ctrl: &mut GameplayCameraController,
        input: &InputState,
        net: Option<&NetClient>,
        dt: f32,
    ) {
        if self.area_kind == AreaKind::Outside {
            camera.znear = 0.25;
            camera.zfar = 600.0;
            renderer.fog_color = [0.56, 0.72, 0.86];
            renderer.fog_start = 120.0;
            renderer.fog_end = 220.0;
        } else {
            camera.znear = 0.20;
            camera.zfar = 250.0;
            renderer.fog_color = [0.04, 0.05, 0.08];
            renderer.fog_start = 42.0;
            renderer.fog_end = 95.0;
        }

        if matches!(self.phase, GamePhase::LoadingArea | GamePhase::Playing) {
            self.area_enter_elapsed_s += dt;
        }

        if self.phase == GamePhase::LoadingArea && self.is_area_ready_for_wake() {
            self.phase = GamePhase::Playing;
            self.wake_ready = true;
        }

        if self.phase == GamePhase::Playing {
            if let Some(t) = self.wake_cam_timer.as_mut() {
                if input.middle_mouse_down || input.scroll_delta.abs() > 0.0 {
                    self.wake_cam_timer = None;
                } else {
                    *t += dt;
                    let u = (*t / 3.2).clamp(0.0, 1.0);
                    let ease = u * u * (3.0 - 2.0 * u);
                    cam_ctrl.pitch_deg = 30.0 + (40.0 - 30.0) * ease;
                    cam_ctrl.distance = 9.8 + (13.5 - 9.8) * ease;
                    if u >= 1.0 {
                        self.wake_cam_timer = None;
                    }
                }
            }
        }

        if self.area_kind == AreaKind::Outside {
            if let Some(local) = self.local_entity() {
                let pos = local.render_pos;
                self.queue_outside_tiles_around(pos);
            }
        }

        self.update_entities(dt);

        if let Some(local) = self.local_entity() {
            cam_ctrl.player_pos = local.render_pos;
            if let Some(marker) = self.move_target_marker {
                let dist_xz = Vec3::new(
                    marker.x - local.render_pos.x,
                    0.0,
                    marker.z - local.render_pos.z,
                )
                .length();
                if dist_xz < 0.35 && !local.moving {
                    self.move_target_marker = None;
                }
            }
        }

        cam_ctrl.update(camera, input, dt);

        if self.phase == GamePhase::Playing && !input.middle_mouse_down {
            let hit = camera
                .screen_ray(
                    input.mouse_x,
                    input.mouse_y,
                    input.canvas_width,
                    input.canvas_height,
                )
                .and_then(|(orig, dir)| self.raycast_terrain(orig, dir));

            self.hover_walkable = hit.and_then(|p| self.is_point_walkable(p.x, p.z));
            let cursor = match self.hover_walkable {
                Some(true) => "pointer",
                Some(false) => "not-allowed",
                None => "",
            };
            let _ = canvas.style().set_property("cursor", cursor);

            if input.left_just_pressed {
                if let Some(hit_pos) = hit {
                    if let Some(net) = net {
                        net.send(&shared::protocol::ClientMessage::MoveTo {
                            x: hit_pos.x,
                            z: hit_pos.z,
                        });
                    }
                    if self.hover_walkable == Some(true) {
                        self.move_target_marker = Some(hit_pos);
                    }
                }
            }
        } else {
            self.hover_walkable = None;
            let _ = canvas.style().set_property("cursor", "");
        }
    }

    pub fn render_shadows(&self, renderer: &mut Renderer, camera: &Camera) {
        if !matches!(self.phase, GamePhase::LoadingArea | GamePhase::Playing) {
            return;
        }
        let min_extent = if self.area_kind == AreaKind::Outside {
            28.0
        } else {
            16.0
        };
        renderer.begin_shadow_pass_with_min_extent(camera, min_extent);

        if self.area_kind == AreaKind::Outside {
            for tile in self.outside_tiles.values() {
                renderer.draw_terrain_shadow(&tile.gpu);
            }
            for tile in self.outside_tiles.values() {
                self.draw_prop_shadows(renderer, &tile.vis.props);
            }
        } else if let Some(inside) = &self.inside_area {
            renderer.draw_terrain_shadow(&inside.gpu);
            self.draw_prop_shadows(renderer, &inside.vis.props);
        }

        for entity in self.entities.values() {
            self.draw_entity_shadow(renderer, entity);
        }
    }

    pub fn render(&self, renderer: &Renderer, camera: &Camera) {
        if !matches!(self.phase, GamePhase::LoadingArea | GamePhase::Playing) {
            return;
        }

        let no_brush = BrushOverlay::default();
        if self.area_kind == AreaKind::Outside {
            for (&(tx, ty), tile) in &self.outside_tiles {
                let ox = (tx * TILE_SIZE as i32) as f32;
                let oz = (ty * TILE_SIZE as i32) as f32;
                renderer.draw_terrain_chunk(camera, &tile.gpu, ox, oz, false, false, &no_brush);
            }
            for tile in self.outside_tiles.values() {
                self.draw_prop_list(renderer, camera, &tile.vis.props);
            }
            for tile in self.outside_tiles.values() {
                self.draw_road_list(
                    renderer,
                    camera,
                    &tile.vis.roads,
                    &tile.road_meshes,
                    &no_brush,
                );
            }
        } else if let Some(inside) = &self.inside_area {
            renderer.draw_terrain_chunk(camera, &inside.gpu, 0.0, 0.0, false, false, &no_brush);
            self.draw_prop_list(renderer, camera, &inside.vis.props);
            self.draw_road_list(
                renderer,
                camera,
                &inside.vis.roads,
                &inside.road_meshes,
                &no_brush,
            );
        }

        if let Some(marker) = self.move_target_marker {
            renderer.draw_node_ring(
                camera,
                marker + Vec3::Y * 0.05,
                0.42,
                [0.35, 0.88, 1.0, 0.85],
                false,
            );
        }

        for entity in self.entities.values() {
            self.draw_entity(renderer, camera, entity);
        }
    }

    fn is_area_ready_for_wake(&self) -> bool {
        let terrain_ready = if self.area_kind == AreaKind::Outside {
            if let Some(local) = self.local_entity() {
                let tx = (local.render_pos.x / TILE_SIZE as f32).floor() as i32;
                let ty = (local.render_pos.z / TILE_SIZE as f32).floor() as i32;
                self.outside_tiles.contains_key(&(tx, ty))
            } else {
                !self.outside_tiles.is_empty()
            }
        } else {
            self.inside_area.is_some()
        };

        if !terrain_ready {
            return false;
        }
        if self.area_enter_elapsed_s >= 2.0 {
            return true;
        }

        let props_ready = if let Some(inside) = &self.inside_area {
            inside
                .vis
                .props
                .iter()
                .all(|p| self.prop_meshes.contains_key(&p.asset_id))
        } else {
            true
        };

        let char_ready = self.local_entity().is_some_and(|e| match &e.visual {
            ClientEntityVisualState::Character { appearance, .. } => {
                let Some(catalog) = &self.character_catalog else {
                    return false;
                };
                let Some(skel) = catalog.skeletons.get(appearance.skeleton_idx as usize) else {
                    return false;
                };
                self.rigs.contains_key(&skel.rig_path)
                    && (!skel.render_base_mesh
                        || !appearance.show_base_mesh
                        || self.base_meshes.contains_key(&skel.rig_path))
            }
            ClientEntityVisualState::Prop { asset_id, .. } => {
                self.prop_meshes.contains_key(asset_id)
            }
        });

        props_ready && char_ready
    }

    fn update_entities(&mut self, dt: f32) {
        let pos_alpha = (12.0 * dt).min(1.0);
        let yaw_alpha = (14.0 * dt).min(1.0);

        for entity in self.entities.values_mut() {
            entity.render_pos = entity.render_pos.lerp(entity.target_pos, pos_alpha);
            entity.render_yaw = lerp_angle(entity.render_yaw, entity.target_yaw, yaw_alpha);

            let Some(catalog) = &self.character_catalog else {
                continue;
            };
            let is_moving =
                entity.moving || (entity.target_pos - entity.render_pos).length_squared() > 0.002;
            let ClientEntityVisualState::Character {
                appearance,
                pose,
                anim_time,
            } = &mut entity.visual
            else {
                continue;
            };

            let Some(skel) = catalog.skeletons.get(appearance.skeleton_idx as usize) else {
                continue;
            };
            let Some(rig) = self.rigs.get(&skel.rig_path) else {
                continue;
            };

            let preferred = if is_moving { "Walk" } else { "Idle" };
            let clip = find_clip(rig, skel, &self.extra_anims, preferred)
                .or_else(|| find_clip(rig, skel, &self.extra_anims, "Idle"))
                .or_else(|| rig.animations.first());

            if let Some(c) = clip {
                if c.duration > 1e-6 {
                    *anim_time = (*anim_time + dt).rem_euclid(c.duration);
                }
            }
            evaluate_pose(rig, clip, *anim_time, true, pose);
        }
    }

    fn resolve_character_parts<'a>(
        &self,
        catalog: &'a CharacterCatalog,
        app: &CompactCharacterAppearance,
    ) -> Option<(&'a SkeletonDef, CharacterDefinition)> {
        let skel = catalog.skeletons.get(app.skeleton_idx as usize)?;
        let def = app.to_definition("entity", "Entity", catalog)?;
        Some((skel, def))
    }

    fn draw_entity_shadow(&self, renderer: &Renderer, entity: &ClientEntityRenderState) {
        match &entity.visual {
            ClientEntityVisualState::Character {
                appearance, pose, ..
            } => {
                let Some(catalog) = &self.character_catalog else {
                    return;
                };
                let Some((skel, def)) = self.resolve_character_parts(catalog, appearance) else {
                    return;
                };
                let Some(rig) = self.rigs.get(&skel.rig_path) else {
                    return;
                };
                if pose.world_matrices.len() != rig.bones.len() {
                    return;
                }
                let char_model = Mat4::from_rotation_translation(
                    Quat::from_rotation_y(entity.render_yaw),
                    entity.render_pos,
                );

                if skel.render_base_mesh && def.show_base_mesh {
                    if let Some(base_gpu) = self.base_meshes.get(&skel.rig_path) {
                        let palette = base_gpu.remapped_skin_palette(rig, pose);
                        renderer.draw_character_shadow(base_gpu, &char_model, Some(&palette));
                    }
                }
                for part_id in def.skinned_parts.values() {
                    if let Some(part) = catalog.parts.iter().find(|p| p.id == *part_id) {
                        if let Some(mesh) = self.part_meshes.get(&part.path) {
                            let palette = mesh
                                .is_skinned
                                .then(|| mesh.remapped_skin_palette(rig, pose));
                            renderer.draw_character_shadow(mesh, &char_model, palette.as_ref());
                        }
                    }
                }
                for (socket_id, part_id) in &def.socket_parts {
                    let Some(sock) = skel.sockets.iter().find(|s| s.id == *socket_id) else {
                        continue;
                    };
                    let Some(part) = catalog.parts.iter().find(|p| p.id == *part_id) else {
                        continue;
                    };
                    let Some(mesh) = self.part_meshes.get(&part.path) else {
                        continue;
                    };
                    let world =
                        char_model * pose.socket_world_matrix(rig, sock) * part_local_matrix(part);
                    renderer.draw_character_shadow(mesh, &world, None);
                }
            }
            ClientEntityVisualState::Prop {
                asset_id,
                scale,
                cast_shadow,
            } => {
                if !*cast_shadow {
                    return;
                }
                if let Some(mesh) = self.prop_meshes.get(asset_id) {
                    let model = Mat4::from_translation(entity.render_pos)
                        * Mat4::from_rotation_y(entity.render_yaw)
                        * Mat4::from_scale(Vec3::from(*scale));
                    renderer.draw_mesh_shadow(mesh, &model);
                }
            }
        }
    }

    fn draw_entity(&self, renderer: &Renderer, camera: &Camera, entity: &ClientEntityRenderState) {
        match &entity.visual {
            ClientEntityVisualState::Character {
                appearance, pose, ..
            } => {
                let Some(catalog) = &self.character_catalog else {
                    return;
                };
                let Some((skel, def)) = self.resolve_character_parts(catalog, appearance) else {
                    return;
                };
                let Some(rig) = self.rigs.get(&skel.rig_path) else {
                    return;
                };
                if pose.world_matrices.len() != rig.bones.len() {
                    return;
                }

                let char_model = Mat4::from_rotation_translation(
                    Quat::from_rotation_y(entity.render_yaw),
                    entity.render_pos,
                );
                let char_palette = &def.palette;

                if skel.render_base_mesh && def.show_base_mesh {
                    if let Some(base_gpu) = self.base_meshes.get(&skel.rig_path) {
                        let skin_mats = base_gpu.remapped_skin_palette(rig, pose);
                        renderer.draw_character_mesh(
                            camera,
                            base_gpu,
                            &char_model,
                            Some(&skin_mats),
                            Some(char_palette),
                            Vec4::ONE,
                        );
                    }
                }

                for part_id in def.skinned_parts.values() {
                    if let Some(part) = catalog.parts.iter().find(|p| p.id == *part_id) {
                        if let Some(mesh) = self.part_meshes.get(&part.path) {
                            let skin_mats = mesh
                                .is_skinned
                                .then(|| mesh.remapped_skin_palette(rig, pose));
                            renderer.draw_character_mesh(
                                camera,
                                mesh,
                                &char_model,
                                skin_mats.as_ref(),
                                Some(char_palette),
                                Vec4::ONE,
                            );
                        }
                    }
                }

                for (socket_id, part_id) in &def.socket_parts {
                    let Some(sock) = skel.sockets.iter().find(|s| s.id == *socket_id) else {
                        continue;
                    };
                    let Some(part) = catalog.parts.iter().find(|p| p.id == *part_id) else {
                        continue;
                    };
                    let Some(mesh) = self.part_meshes.get(&part.path) else {
                        continue;
                    };
                    let world =
                        char_model * pose.socket_world_matrix(rig, sock) * part_local_matrix(part);
                    renderer.draw_character_mesh(
                        camera,
                        mesh,
                        &world,
                        None,
                        Some(char_palette),
                        Vec4::ONE,
                    );
                }
            }
            ClientEntityVisualState::Prop {
                asset_id, scale, ..
            } => {
                if let Some(mesh) = self.prop_meshes.get(asset_id) {
                    let model = Mat4::from_translation(entity.render_pos)
                        * Mat4::from_rotation_y(entity.render_yaw)
                        * Mat4::from_scale(Vec3::from(*scale));
                    renderer.draw_mesh(camera, mesh, &model, Vec4::ONE);
                }
            }
        }
    }

    fn draw_prop_shadows(&self, renderer: &Renderer, props: &[PropPlacement]) {
        for prop in props {
            if !prop.cast_shadow {
                continue;
            }
            if let Some(mesh) = self.prop_meshes.get(&prop.asset_id) {
                renderer.draw_prop_shadow(mesh, prop);
            }
        }
    }

    fn draw_prop_list(&self, renderer: &Renderer, camera: &Camera, props: &[PropPlacement]) {
        for prop in props {
            if let Some(mesh) = self.prop_meshes.get(&prop.asset_id) {
                renderer.draw_prop(camera, mesh, prop, Vec4::ONE);
            }
        }
    }

    fn draw_road_list(
        &self,
        renderer: &Renderer,
        camera: &Camera,
        roads: &[RoadSpline],
        meshes: &HashMap<u64, RoadMeshGpu>,
        brush: &BrushOverlay,
    ) {
        for road in roads {
            if let Some(mesh) = meshes.get(&road.id) {
                renderer.draw_road(camera, mesh, road.material, 1.0, Vec3::ONE, brush);
            }
        }
    }

    fn ensure_entity_visual_requested(&mut self, area_id: &str, visual: &EntityVisual) {
        match visual {
            EntityVisual::Character(app) => {
                if self.character_catalog.is_none() && !self.character_catalog_requested {
                    self.character_catalog_requested = true;
                    self.pending_char_catalog_fetch = true;
                }
                self.ensure_appearance_glbs_requested(app);
            }
            EntityVisual::Prop { asset_id, .. } => {
                self.ensure_asset_id_requested(area_id, asset_id);
            }
        }
    }

    fn ensure_appearance_glbs_requested(&mut self, app: &CompactCharacterAppearance) {
        let Some(catalog) = &self.character_catalog else {
            return;
        };
        let Some(skel) = catalog.skeletons.get(app.skeleton_idx as usize) else {
            return;
        };
        if !self.rigs.contains_key(&skel.rig_path)
            && self.requested_char_glbs.insert(skel.rig_path.clone())
        {
            self.pending_char_glb_fetches.push(skel.rig_path.clone());
        }
        for anim_url in &skel.animation_paths {
            if !self.extra_anims.contains_key(anim_url)
                && self.requested_char_glbs.insert(anim_url.clone())
            {
                self.pending_char_glb_fetches.push(anim_url.clone());
            }
        }
        for &(_, part_idx) in app
            .skinned_part_indices
            .iter()
            .chain(app.socket_part_indices.iter())
        {
            if let Some(part) = catalog.parts.get(part_idx as usize) {
                if !self.part_meshes.contains_key(&part.path)
                    && self.requested_char_glbs.insert(part.path.clone())
                {
                    self.pending_char_glb_fetches.push(part.path.clone());
                }
            }
        }
    }

    pub(super) fn ensure_asset_id_requested(&mut self, area_id: &str, asset_id: &str) {
        if self.prop_meshes.contains_key(asset_id) || self.requested_assets.contains(asset_id) {
            return;
        }
        let resolved = if let Some(slug) = local_asset_slug(asset_id) {
            if area_id.is_empty() {
                None
            } else {
                Some(AssetEntry {
                    id: asset_id.to_owned(),
                    name: prettify_asset_slug(slug),
                    path: local_asset_http_path(area_id, slug),
                    category: "Area".to_owned(),
                })
            }
        } else {
            self.asset_catalog
                .iter()
                .find(|e| e.id == asset_id)
                .cloned()
        };
        if let Some(entry) = resolved {
            self.requested_assets.insert(asset_id.to_owned());
            self.pending_asset_fetches.push(entry);
        }
    }

    pub(super) fn ensure_prop_assets_requested(&mut self, area_id: &str, props: &[PropPlacement]) {
        for prop in props {
            self.ensure_asset_id_requested(area_id, &prop.asset_id);
        }
    }

    fn queue_outside_tiles_around(&mut self, pos: Vec3) {
        if self.area_id.is_empty() {
            return;
        }
        let cx = (pos.x / TILE_SIZE as f32).floor() as i32;
        let cz = (pos.z / TILE_SIZE as f32).floor() as i32;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let tx = cx + dx;
                let ty = cz + dy;
                if !self.outside_tiles.contains_key(&(tx, ty))
                    && self
                        .requested_tiles
                        .insert((self.area_id.clone(), tx, ty))
                {
                    self.pending_tile_fetches
                        .push((self.area_id.clone(), tx, ty));
                }
            }
        }
    }

    fn is_point_walkable(&self, wx: f32, wz: f32) -> Option<bool> {
        if self.area_kind == AreaKind::Outside {
            let tx = (wx / TILE_SIZE as f32).floor() as i32;
            let ty = (wz / TILE_SIZE as f32).floor() as i32;
            let tile = self.outside_tiles.get(&(tx, ty))?;
            let lx = (wx.floor() as i32 - tx * TILE_SIZE as i32) as usize;
            let lz = (wz.floor() as i32 - ty * TILE_SIZE as i32) as usize;
            Some(tile.vis.walkable.get(lx, lz))
        } else {
            let inside = self.inside_area.as_ref()?;
            let w = inside.vis.width as f32;
            let l = inside.vis.length as f32;
            if wx < 0.0 || wz < 0.0 || wx >= w || wz >= l {
                return None;
            }
            Some(inside.vis.get_walkable(wx.floor() as usize, wz.floor() as usize))
        }
    }

    fn raycast_terrain(&self, orig: Vec3, dir: Vec3) -> Option<Vec3> {
        let mut t = 0.4f32;
        let step = 0.65f32;
        let mut prev_t: Option<f32> = None;
        let mut prev_above = false;

        while t <= 220.0 {
            let p = orig + dir * t;
            if let Some(h) = self.terrain_height_at(p.x, p.z) {
                let above = p.y >= h;
                if let Some(pt) = prev_t {
                    if prev_above && !above {
                        let mut lo = pt;
                        let mut hi = t;
                        for _ in 0..8 {
                            let mid = 0.5 * (lo + hi);
                            let mp = orig + dir * mid;
                            if let Some(mh) = self.terrain_height_at(mp.x, mp.z) {
                                if mp.y >= mh {
                                    lo = mid;
                                } else {
                                    hi = mid;
                                }
                            }
                        }
                        let hit = orig + dir * (0.5 * (lo + hi));
                        let hy = self.terrain_height_at(hit.x, hit.z).unwrap_or(hit.y);
                        return Some(Vec3::new(hit.x, hy, hit.z));
                    }
                }
                prev_t = Some(t);
                prev_above = above;
            }
            t += step;
        }
        None
    }

    fn terrain_height_at(&self, wx: f32, wz: f32) -> Option<f32> {
        if self.area_kind == AreaKind::Outside {
            let tx = (wx / TILE_SIZE as f32).floor() as i32;
            let ty = (wz / TILE_SIZE as f32).floor() as i32;
            let tile = self.outside_tiles.get(&(tx, ty))?;
            let lx = (wx - (tx * TILE_SIZE as i32) as f32).clamp(0.0, TILE_SIZE as f32 - 0.0001);
            let lz = (wz - (ty * TILE_SIZE as i32) as f32).clamp(0.0, TILE_SIZE as f32 - 0.0001);
            Some(bilerp_height(&tile.vis.heights, TILE_VERTICES, lx, lz))
        } else if let Some(inside) = &self.inside_area {
            let w = inside.vis.width as f32;
            let l = inside.vis.length as f32;
            if wx < 0.0 || wz < 0.0 || wx >= w || wz >= l {
                return None;
            }
            Some(bilerp_height(
                &inside.vis.heights,
                inside.vis.width as usize + 1,
                wx,
                wz,
            ))
        } else {
            None
        }
    }

    fn sample_outside_vertex_height(&self, wx: i32, wz: i32) -> Option<f32> {
        let tx = wx.div_euclid(TILE_SIZE as i32);
        let ty = wz.div_euclid(TILE_SIZE as i32);
        let vx = wx.rem_euclid(TILE_SIZE as i32) as usize;
        let vz = wz.rem_euclid(TILE_SIZE as i32) as usize;
        if let Some(tile) = self.outside_tiles.get(&(tx, ty)) {
            return Some(tile.vis.heights[vz * TILE_VERTICES + vx]);
        }
        let alt_tx = if vx == 0 { tx - 1 } else { tx };
        let alt_ty = if vz == 0 { ty - 1 } else { ty };
        for (cand_tx, cand_ty) in [(alt_tx, ty), (tx, alt_ty), (alt_tx, alt_ty)] {
            if let Some(tile) = self.outside_tiles.get(&(cand_tx, cand_ty)) {
                let avx = (wx - cand_tx * TILE_SIZE as i32) as usize;
                let avz = (wz - cand_ty * TILE_SIZE as i32) as usize;
                if avx < TILE_VERTICES && avz < TILE_VERTICES {
                    return Some(tile.vis.heights[avz * TILE_VERTICES + avx]);
                }
            }
        }
        None
    }

    fn refresh_outside_tile_meshes(&self, gl: &glow::Context, tiles: &[(i32, i32)]) {
        for &(tx, ty) in tiles {
            let Some(tile) = self.outside_tiles.get(&(tx, ty)) else {
                continue;
            };
            let ox = tx * TILE_SIZE as i32;
            let oz = ty * TILE_SIZE as i32;
            let mut normals = Vec::with_capacity(TILE_VERTICES * TILE_VERTICES);
            for vz in 0..TILE_VERTICES {
                for vx in 0..TILE_VERTICES {
                    let wx = ox + vx as i32;
                    let wz = oz + vz as i32;
                    let hc = tile.vis.heights[vz * TILE_VERTICES + vx];
                    let hl = self.sample_outside_vertex_height(wx - 1, wz).unwrap_or(hc);
                    let hr = self.sample_outside_vertex_height(wx + 1, wz).unwrap_or(hc);
                    let hd = self.sample_outside_vertex_height(wx, wz - 1).unwrap_or(hc);
                    let hu = self.sample_outside_vertex_height(wx, wz + 1).unwrap_or(hc);
                    let n = Vec3::new(hl - hr, 2.0, hd - hu).normalize_or_zero();
                    normals.push(n.to_array());
                }
            }
            tile.gpu
                .upload_vertices(gl, ox as f32, oz as f32, &tile.vis.heights, &normals);
        }
    }

    fn refresh_inside_mesh(&self, gl: &glow::Context) {
        let Some(inside) = &self.inside_area else {
            return;
        };
        let w = inside.vis.width as usize;
        let l = inside.vis.length as usize;
        let vw = w + 1;
        let vh = l + 1;
        let mut normals = Vec::with_capacity(vw * vh);
        for vz in 0..vh {
            for vx in 0..vw {
                let hc = inside.vis.heights[vz * vw + vx];
                let hl = if vx > 0 { inside.vis.heights[vz * vw + (vx - 1)] } else { hc };
                let hr = if vx < w { inside.vis.heights[vz * vw + (vx + 1)] } else { hc };
                let hd = if vz > 0 { inside.vis.heights[(vz - 1) * vw + vx] } else { hc };
                let hu = if vz < l { inside.vis.heights[(vz + 1) * vw + vx] } else { hc };
                let n = Vec3::new(hl - hr, 2.0, hd - hu).normalize_or_zero();
                normals.push(n.to_array());
            }
        }
        inside
            .gpu
            .upload_vertices(gl, 0.0, 0.0, &inside.vis.heights, &normals);
    }
}

fn client_visual_from_wire(visual: EntityVisual) -> ClientEntityVisualState {
    match visual {
        EntityVisual::Character(appearance) => ClientEntityVisualState::Character {
            appearance,
            pose: SkeletonPose::default(),
            anim_time: 0.0,
        },
        EntityVisual::Prop {
            asset_id,
            scale,
            cast_shadow,
        } => ClientEntityVisualState::Prop {
            asset_id,
            scale,
            cast_shadow,
        },
    }
}

fn find_clip<'a>(
    rig: &'a ParsedSkeletonRig,
    skel: &SkeletonDef,
    extra_anims: &'a HashMap<String, Vec<AnimationClip>>,
    name: &str,
) -> Option<&'a AnimationClip> {
    for anim_url in &skel.animation_paths {
        if let Some(clips) = extra_anims.get(anim_url) {
            if let Some(found) = clips.iter().find(|c| c.name.eq_ignore_ascii_case(name)) {
                return Some(found);
            }
        }
    }
    rig.animations.iter().find(|c| c.name.eq_ignore_ascii_case(name))
}

fn part_local_matrix(part: &CharacterPartDef) -> Mat4 {
    match &part.attachment {
        PartAttachment::Skinned { .. } => Mat4::IDENTITY,
        PartAttachment::Socket {
            offset,
            rotation_deg,
            scale,
            ..
        } => {
            let rot = Quat::from_euler(
                EulerRot::YXZ,
                rotation_deg[1].to_radians(),
                rotation_deg[0].to_radians(),
                rotation_deg[2].to_radians(),
            );
            let s = if scale.abs() < 1e-4 { 1.0 } else { *scale };
            Mat4::from_scale_rotation_translation(Vec3::splat(s), rot, Vec3::from(*offset))
        }
    }
}

fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut diff = (b - a).rem_euclid(TAU);
    if diff > std::f32::consts::PI {
        diff -= TAU;
    }
    a + diff * t
}

fn bilerp_height(heights: &[f32], stride: usize, lx: f32, lz: f32) -> f32 {
    let ix = lx.floor() as usize;
    let iz = lz.floor() as usize;
    let fx = lx - ix as f32;
    let fz = lz - iz as f32;
    let i00 = iz * stride + ix;
    let h00 = heights.get(i00).copied().unwrap_or(0.0);
    let h10 = heights.get(i00 + 1).copied().unwrap_or(h00);
    let h01 = heights.get(i00 + stride).copied().unwrap_or(h00);
    let h11 = heights.get(i00 + stride + 1).copied().unwrap_or(h00);
    let h0 = h00 + (h10 - h00) * fx;
    let h1 = h01 + (h11 - h01) * fx;
    h0 + (h1 - h0) * fz
}
