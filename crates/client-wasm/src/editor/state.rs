use std::collections::{HashMap, HashSet};

use glam::Vec3;
use shared::world::{
    AreaKind, AreaMetadata, AssetCatalog, AssetEntry, InsideVisData, MAX_ROAD_MATERIALS,
    MAX_TERRAIN_MATERIALS, PropPlacement, ROAD_MAT_COBBLESTONE, ROAD_MAT_DIRT, ROAD_MATERIAL_COUNT,
    RoadStyle, TERRAIN_MATERIAL_COUNT, TILE_SIZE, TileVisData, format_local_asset_id,
    is_local_asset_id, local_asset_http_path, local_asset_slug, prettify_asset_slug,
    sanitize_asset_slug,
};

use crate::camera::{Camera, EditorCameraController};
use crate::glb_loader::parse_glb;
use crate::input::InputState;
use crate::render::{PropMeshGpu, TerrainMeshGpu};

use super::character_studio::{CharacterStudioState, StudioCameraController};
use super::gizmo::{GizmoAxis, GizmoDragState};
use super::types::{
    DecorationSubMode, EditorInsideState, EditorMode, EditorStatusSnapshot, EditorTileState,
    EditorTool, HeightMode, PendingSave, RoadDraft, RoadSubMode, SelectedPropInfo,
    SelectedRoadInfo, WalkabilityMode,
};

pub struct EditorState {
    pub mode: EditorMode,
    pub studio_cam: StudioCameraController,
    pub character_studio: CharacterStudioState,

    pub active_area: AreaMetadata,
    pub outside_tiles: HashMap<(i32, i32), EditorTileState>,
    pub requested_tiles: HashSet<(String, i32, i32)>,
    pub inside_area: Option<EditorInsideState>,
    pub requested_inside: Option<String>,

    pub catalog: Vec<AssetEntry>,
    #[allow(dead_code)]
    pub catalog_requested: bool,
    pub prop_meshes: HashMap<String, PropMeshGpu>,
    pub requested_assets: HashSet<String>,

    pub tool: EditorTool,
    pub height_mode: HeightMode,
    pub walkability_mode: WalkabilityMode,
    pub brush_radius: f32,
    pub brush_strength: f32,
    pub texture_layer: usize,
    pub max_texture_layer: usize,

    pub road_submode: RoadSubMode,
    pub road_style: RoadStyle,
    pub road_material: u8,
    pub max_road_material: u8,
    pub road_soft_edge_mask: u16,
    pub road_width: f32,
    pub road_shoulder: f32,
    pub road_draft: Option<RoadDraft>,
    pub hover_road_point: Option<([f32; 3], bool)>,
    pub selected_road_id: Option<u64>,
    pub selected_road_node: Option<usize>,
    pub dragging_road_node: bool,
    pub road_drag_moved: bool,
    pub welded_drag_peers: Vec<(Option<(i32, i32)>, u64, usize)>,
    pub next_road_id: u64,

    pub decoration_submode: DecorationSubMode,
    pub active_asset_id: String,
    pub preview_rotation: [f32; 3],
    pub preview_scale: f32,
    pub preview_cast_shadow: bool,
    pub selected_prop_id: Option<u64>,
    pub dragging_prop: bool,
    pub hovered_rotate_axis: Option<GizmoAxis>,
    pub rotate_drag: Option<super::gizmo::RotateDragState>,
    pub hovered_gizmo_axis: Option<GizmoAxis>,
    pub gizmo_drag: Option<GizmoDragState>,
    pub next_prop_id: u64,

    pub hover_world: Option<Vec3>,
    pub flatten_target_y: Option<f32>,

    pub pending_tile_fetches: Vec<(String, i32, i32)>,
    pub pending_inside_fetch: Option<String>,
    pub pending_asset_fetches: Vec<AssetEntry>,
    pub pending_saves: Vec<PendingSave>,

    pub autosave_deadline_ms: Option<f64>,
    pub in_flight_saves: usize,
    pub save_state: String,
}

impl EditorState {
    pub fn new() -> Self {
        Self {
            mode: EditorMode::World,
            studio_cam: StudioCameraController::default(),
            character_studio: CharacterStudioState::new(),
            active_area: AreaMetadata {
                id: String::new(),
                name: "No Area".to_owned(),
                kind: AreaKind::Outside,
                dimensions: None,
            },
            outside_tiles: HashMap::new(),
            requested_tiles: HashSet::new(),
            inside_area: None,
            requested_inside: None,
            catalog: Vec::new(),
            catalog_requested: false,
            prop_meshes: HashMap::new(),
            requested_assets: HashSet::new(),
            tool: EditorTool::Heightmap,
            height_mode: HeightMode::Raise,
            walkability_mode: WalkabilityMode::Block,
            brush_radius: 4.5,
            brush_strength: 3.5,
            texture_layer: 1, // Default brush paints Dirt so clicking immediately shows contrast on Grass
            max_texture_layer: TERRAIN_MATERIAL_COUNT - 1,
            road_submode: RoadSubMode::Place,
            road_style: RoadStyle::Curve,
            road_material: ROAD_MAT_COBBLESTONE,
            max_road_material: ROAD_MATERIAL_COUNT as u8 - 1,
            road_soft_edge_mask: 1u16 << ROAD_MAT_DIRT,
            road_width: 4.0,
            road_shoulder: 3.0,
            road_draft: None,
            hover_road_point: None,
            selected_road_id: None,
            selected_road_node: None,
            dragging_road_node: false,
            road_drag_moved: false,
            welded_drag_peers: Vec::new(),
            next_road_id: 1000,
            decoration_submode: DecorationSubMode::Place,
            active_asset_id: "pine_tree".to_owned(),
            preview_rotation: [0.0; 3],
            preview_scale: 1.0,
            preview_cast_shadow: true,
            selected_prop_id: None,
            dragging_prop: false,
            hovered_rotate_axis: None,
            rotate_drag: None,
            hovered_gizmo_axis: None,
            gizmo_drag: None,
            next_prop_id: 1000,
            hover_world: None,
            flatten_target_y: None,
            pending_tile_fetches: Vec::new(),
            pending_inside_fetch: None,
            pending_asset_fetches: Vec::new(),
            pending_saves: Vec::new(),
            autosave_deadline_ms: None,
            in_flight_saves: 0,
            save_state: "Saved".to_owned(),
        }
    }

    pub fn switch_area(
        &mut self,
        gl: &glow::Context,
        meta: AreaMetadata,
        cam_ctrl: &mut EditorCameraController,
    ) {
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
        if let Some(mut draft) = self.road_draft.take() {
            if let Some(mesh) = draft.preview_gpu.take() {
                mesh.destroy(gl);
            }
        }
        let stale_local_ids: Vec<String> = self
            .prop_meshes
            .keys()
            .filter(|id| is_local_asset_id(id))
            .cloned()
            .collect();
        for id in stale_local_ids {
            if let Some(mesh) = self.prop_meshes.remove(&id) {
                mesh.destroy(gl);
            }
        }
        self.requested_assets.retain(|id| !is_local_asset_id(id));
        self.pending_asset_fetches
            .retain(|entry| !is_local_asset_id(&entry.id));
        if is_local_asset_id(&self.active_asset_id) {
            self.active_asset_id = self
                .catalog
                .first()
                .map(|e| e.id.clone())
                .unwrap_or_else(|| "pine_tree".to_owned());
        }

        self.requested_tiles.clear();
        self.requested_inside = None;
        self.pending_tile_fetches.clear();
        self.pending_inside_fetch = None;
        self.hover_road_point = None;
        self.selected_road_id = None;
        self.selected_road_node = None;
        self.dragging_road_node = false;
        self.road_drag_moved = false;
        self.welded_drag_peers.clear();
        self.selected_prop_id = None;
        self.hovered_gizmo_axis = None;
        self.gizmo_drag = None;
        self.hovered_rotate_axis = None;
        self.rotate_drag = None;
        self.hover_world = None;
        self.autosave_deadline_ms = None;
        self.save_state = "Saved".to_owned();

        match meta.kind {
            AreaKind::Outside => {
                cam_ctrl.focus_on(32.0, 32.0);
                cam_ctrl.distance = 52.0;
            }
            AreaKind::Inside(_) => {
                let (w, l) = meta.dimensions.unwrap_or((24, 24));
                cam_ctrl.focus_on(w as f32 * 0.5, l as f32 * 0.5);
                cam_ctrl.distance = (w.max(l) as f32 * 1.35).clamp(18.0, 120.0);
                self.requested_inside = Some(meta.id.clone());
                self.pending_inside_fetch = Some(meta.id.clone());
            }
        }
        self.active_area = meta;
    }

    #[inline]
    pub(super) fn is_road_material_soft(&self, mat: u8) -> bool {
        (mat as usize) < 16 && (self.road_soft_edge_mask & (1u16 << mat)) != 0
    }

    pub fn set_catalog(&mut self, gl: &glow::Context, catalog: &AssetCatalog) {
        let entries = &catalog.decorations;
        if let Some(first) = entries.first() {
            if !entries.iter().any(|e| e.id == self.active_asset_id) {
                self.active_asset_id = first.id.clone();
            }
        }
        for entry in entries {
            if !self.prop_meshes.contains_key(&entry.id) && self.requested_assets.insert(entry.id.clone())
            {
                self.pending_asset_fetches.push(entry.clone());
            }
        }
        self.catalog = entries.clone();

        self.max_texture_layer = catalog
            .terrain_textures
            .iter()
            .map(|t| t.layer as usize)
            .filter(|&l| l < MAX_TERRAIN_MATERIALS)
            .max()
            .unwrap_or(TERRAIN_MATERIAL_COUNT - 1);
        self.texture_layer = self.texture_layer.min(self.max_texture_layer);

        self.max_road_material = catalog
            .road_textures
            .iter()
            .map(|r| r.layer)
            .filter(|&l| (l as usize) < MAX_ROAD_MATERIALS)
            .max()
            .unwrap_or(ROAD_MATERIAL_COUNT as u8 - 1);
        self.road_material = self.road_material.min(self.max_road_material);

        let mut next_soft_mask = 1u16 << ROAD_MAT_DIRT;
        for r in &catalog.road_textures {
            if (r.layer as usize) < 16 {
                if r.is_soft_edge() {
                    next_soft_mask |= 1u16 << r.layer;
                } else {
                    next_soft_mask &= !(1u16 << r.layer);
                }
            }
        }
        if next_soft_mask != self.road_soft_edge_mask {
            self.road_soft_edge_mask = next_soft_mask;
            self.rebuild_all_active_road_meshes(gl);
        }
    }

    pub fn on_glb_loaded(
        &mut self,
        gl: &glow::Context,
        asset_id: String,
        bytes: &[u8],
    ) -> Result<(), String> {
        let parsed = parse_glb(bytes).map_err(|e| format!("GLB parse failed: {e}"))?;
        let mesh = PropMeshGpu::from_parsed(gl, &parsed)
            .map_err(|e| format!("Prop GPU upload failed: {e}"))?;
        if let Some(old) = self.prop_meshes.insert(asset_id, mesh) {
            old.destroy(gl);
        }
        Ok(())
    }

    pub fn import_local_glb(
        &mut self,
        gl: &glow::Context,
        cam_ctrl: &EditorCameraController,
        raw_slug: &str,
        bytes: &[u8],
        spawn_in_scene: bool,
        now_ms: f64,
    ) -> Result<String, String> {
        let slug = sanitize_asset_slug(raw_slug);
        let asset_id = format_local_asset_id(&slug);
        self.on_glb_loaded(gl, asset_id.clone(), bytes)?;
        self.requested_assets.insert(asset_id.clone());

        if spawn_in_scene {
            let (mut sx, mut sz) = (cam_ctrl.target.x, cam_ctrl.target.z);
            if self.active_area.kind != AreaKind::Outside {
                if let Some((w, l)) = self.active_area.dimensions {
                    let max_x = (w as f32 - 1.0).max(1.0);
                    let max_z = (l as f32 - 1.0).max(1.0);
                    sx = sx.clamp(1.0, max_x);
                    sz = sz.clamp(1.0, max_z);
                }
            }
            let sy = self.terrain_height_at(sx, sz).unwrap_or(0.0);
            let id = self.next_prop_id;
            self.next_prop_id += 1;
            let placement = PropPlacement {
                id,
                asset_id: asset_id.clone(),
                position: [sx, sy, sz],
                rotation: [0.0; 3],
                scale: [1.0; 3],
                cast_shadow: true,
            };
            if self.insert_prop_placement(placement) {
                self.selected_prop_id = Some(id);
                self.tool = EditorTool::Decoration;
                self.decoration_submode = DecorationSubMode::FreeMove;
                self.mark_unsaved(now_ms);
            }
        }
        Ok(asset_id)
    }

    pub fn select_decoration_asset(&mut self, asset_id: &str) {
        self.active_asset_id = asset_id.to_owned();
        if let Some(slug) = local_asset_slug(asset_id) {
            if !self.prop_meshes.contains_key(asset_id)
                && self.requested_assets.insert(asset_id.to_owned())
            {
                self.pending_asset_fetches.push(AssetEntry {
                    id: asset_id.to_owned(),
                    name: prettify_asset_slug(slug),
                    path: local_asset_http_path(&self.active_area.id, slug),
                    category: "Area Unique".to_owned(),
                });
            }
        }
    }

    fn ensure_local_prop_assets_requested(&mut self, area_id: &str, props: &[PropPlacement]) {
        for prop in props {
            if let Some(slug) = local_asset_slug(&prop.asset_id) {
                if !self.prop_meshes.contains_key(&prop.asset_id)
                    && self.requested_assets.insert(prop.asset_id.clone())
                {
                    self.pending_asset_fetches.push(AssetEntry {
                        id: prop.asset_id.clone(),
                        name: prettify_asset_slug(slug),
                        path: local_asset_http_path(area_id, slug),
                        category: "Area Unique".to_owned(),
                    });
                }
            }
        }
    }

    pub fn on_outside_tile_loaded(
        &mut self,
        gl: &glow::Context,
        area_id: String,
        tx: i32,
        ty: i32,
        vis_opt: Option<TileVisData>,
    ) {
        if self.active_area.id != area_id || self.active_area.kind != AreaKind::Outside {
            return;
        }
        if self.outside_tiles.contains_key(&(tx, ty)) {
            return;
        }

        let is_new_blank = vis_opt.is_none();
        let mut vis = vis_opt.unwrap_or_else(|| TileVisData::seeded_grass(&area_id, tx, ty));

        if let Some(max_id) = vis.props.iter().map(|p| p.id).max() {
            self.next_prop_id = self.next_prop_id.max(max_id + 1);
        }
        if let Some(max_road_id) = vis.roads.iter().map(|r| r.id).max() {
            self.next_road_id = self.next_road_id.max(max_road_id + 1);
        }

        if is_new_blank {
            self.copy_loaded_borders_into(tx, ty, &mut vis);
        }
        self.ensure_local_prop_assets_requested(&area_id, &vis.props);

        if let Ok(gpu) = TerrainMeshGpu::new(
            gl,
            TILE_SIZE,
            TILE_SIZE,
            &vis.heights,
            &vis.splat_ids,
            &vis.splat_weights,
        ) {
            gpu.upload_walkability(gl, |x, z| vis.walkable.get(x, z));
            self.outside_tiles.insert(
                (tx, ty),
                EditorTileState {
                    vis,
                    gpu,
                    road_meshes: HashMap::new(),
                    dirty: false,
                },
            );
            // Re-upload vertices + cross-tile normals for this tile and neighbors
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
            self.rebuild_outside_road_meshes_near(gl, tx, ty);
        }
    }

    pub fn on_inside_loaded(&mut self, gl: &glow::Context, area_id: String, vis: InsideVisData) {
        if self.active_area.id != area_id {
            return;
        }
        if let Some(max_id) = vis.props.iter().map(|p| p.id).max() {
            self.next_prop_id = self.next_prop_id.max(max_id + 1);
        }
        if let Some(max_road_id) = vis.roads.iter().map(|r| r.id).max() {
            self.next_road_id = self.next_road_id.max(max_road_id + 1);
        }
        self.ensure_local_prop_assets_requested(&area_id, &vis.props);
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
            let state = EditorInsideState {
                vis,
                gpu,
                road_meshes: HashMap::new(),
                dirty: false,
            };
            self.inside_area = Some(state);
            self.refresh_inside_mesh(gl);
            self.rebuild_inside_road_meshes(gl);
        }
    }

    pub fn update(
        &mut self,
        gl: &glow::Context,
        camera: &Camera,
        cam_ctrl: &EditorCameraController,
        input: &InputState,
        dt: f32,
        now_ms: f64,
    ) {
        if self.mode == EditorMode::Characters {
            self.character_studio.update(input, dt);
            return;
        }
        if self.mode == EditorMode::Entities {
            return;
        }

        // 1. Stream visible tiles around editor camera target
        if !self.active_area.id.is_empty() && self.active_area.kind == AreaKind::Outside {
            let cx = (cam_ctrl.target.x / TILE_SIZE as f32).floor() as i32;
            let cz = (cam_ctrl.target.z / TILE_SIZE as f32).floor() as i32;
            let radius = if cam_ctrl.distance > 90.0 { 2 } else { 1 };
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let tx = cx + dx;
                    let ty = cz + dy;
                    if !self.outside_tiles.contains_key(&(tx, ty))
                        && self.requested_tiles.insert((self.active_area.id.clone(), tx, ty))
                    {
                        self.pending_tile_fetches
                            .push((self.active_area.id.clone(), tx, ty));
                    }
                }
            }
        }

        // 2. Raycast terrain under cursor
        self.hover_world = camera
            .screen_ray(
                input.mouse_x,
                input.mouse_y,
                input.canvas_width,
                input.canvas_height,
            )
            .and_then(|(orig, dir)| self.raycast_terrain(orig, dir));

        // 3. Hotkeys for active tool
        if self.tool == EditorTool::Decoration {
            self.handle_decoration_hotkeys(input, dt, now_ms);
        } else if self.tool == EditorTool::Road {
            self.handle_road_hotkeys(gl, input, now_ms);
        }

        // 4. Mouse tool interaction (ignore left-click while middle-mouse orbiting)
        if self.tool != EditorTool::Decoration
            || self.decoration_submode != DecorationSubMode::FreeMove
        {
            self.hovered_gizmo_axis = None;
            self.gizmo_drag = None;
        }
        if self.tool != EditorTool::Decoration
            || self.decoration_submode != DecorationSubMode::Rotate
        {
            self.hovered_rotate_axis = None;
            self.rotate_drag = None;
        }
        if !input.middle_mouse_down {
            match self.tool {
                EditorTool::Heightmap => self.handle_heightmap_tool(gl, input, dt, now_ms),
                EditorTool::Texture => self.handle_texture_tool(gl, input, dt, now_ms),
                EditorTool::Road => self.handle_road_tool(gl, camera, input, now_ms),
                EditorTool::Decoration => self.handle_decoration_tool(camera, input, now_ms),
                EditorTool::Walkability => self.handle_walkability_tool(gl, input, now_ms),
            }
        } else if self.tool == EditorTool::Road {
            self.update_road_hover_preview(gl);
        }

        if input.left_just_released {
            if self.dragging_road_node {
                self.finish_road_node_drag(gl, now_ms);
            }
            self.flatten_target_y = None;
            self.dragging_prop = false;
            self.gizmo_drag = None;
            self.rotate_drag = None;
        }

        // 5. Debounced auto-save after finishing an edit stroke
        if let Some(deadline) = self.autosave_deadline_ms {
            if now_ms >= deadline && !input.left_mouse_down {
                self.autosave_deadline_ms = None;
                self.queue_save_all();
            }
        }
    }

    pub(super) fn mark_unsaved(&mut self, now_ms: f64) {
        self.save_state = "Unsaved".to_owned();
        self.autosave_deadline_ms = Some(now_ms + 450.0);
    }

    pub fn queue_save_all(&mut self) {
        if self.active_area.id.is_empty() {
            return;
        }
        let area_id = self.active_area.id.clone();
        if self.active_area.kind == AreaKind::Outside {
            for (&(tx, ty), tile) in &mut self.outside_tiles {
                if tile.dirty {
                    tile.vis.enforce_slope_walkability();
                    if let Ok(bytes) = postcard::to_allocvec(&tile.vis) {
                        self.pending_saves.push(PendingSave::Outside {
                            area_id: area_id.clone(),
                            tx,
                            ty,
                            bytes,
                        });
                        tile.dirty = false;
                        self.in_flight_saves += 1;
                    }
                }
            }
        } else if let Some(inside) = &mut self.inside_area {
            if inside.dirty {
                inside.vis.enforce_slope_walkability();
                if let Ok(bytes) = postcard::to_allocvec(&inside.vis) {
                    self.pending_saves.push(PendingSave::Inside { area_id, bytes });
                    inside.dirty = false;
                    self.in_flight_saves += 1;
                }
            }
        }
        if self.in_flight_saves > 0 {
            self.save_state = "Saving...".to_owned();
        }
    }

    pub fn on_save_finished(&mut self, ok: bool) {
        self.in_flight_saves = self.in_flight_saves.saturating_sub(1);
        if !ok {
            self.save_state = "Save failed".to_owned();
        } else if self.in_flight_saves == 0 && self.dirty_count() == 0 {
            self.save_state = "Saved".to_owned();
        }
    }

    pub fn dirty_count(&self) -> usize {
        if self.active_area.kind == AreaKind::Outside {
            self.outside_tiles.values().filter(|t| t.dirty).count()
        } else {
            usize::from(self.inside_area.as_ref().is_some_and(|i| i.dirty))
        }
    }

    pub fn status_snapshot(&self, cam_ctrl: &EditorCameraController) -> EditorStatusSnapshot {
        let current_tile = [
            (cam_ctrl.target.x / TILE_SIZE as f32).floor() as i32,
            (cam_ctrl.target.z / TILE_SIZE as f32).floor() as i32,
        ];
        let selected_prop = self.find_selected_prop().map(|p| SelectedPropInfo {
            id: p.id,
            asset_id: p.asset_id.clone(),
            position: p.position,
            rotation_deg: p.rotation.map(|r| r.to_degrees().rem_euclid(360.0)),
            scale: p.scale[0],
            cast_shadow: p.cast_shadow,
        });
        let selected_road = self.find_selected_road().map(|r| SelectedRoadInfo {
            id: r.id,
            style: match r.style {
                RoadStyle::Curve => "curve",
                RoadStyle::Straight => "straight",
            },
            material: r.material,
            width: r.width,
            shoulder: r.shoulder,
            point_count: r.points.len(),
            selected_node: self.selected_road_node,
        });

        EditorStatusSnapshot {
            editor_mode: match self.mode {
                EditorMode::World => "world",
                EditorMode::Characters => "characters",
                EditorMode::Entities => "entities",
            },
            area_id: self.active_area.id.clone(),
            area_name: self.active_area.name.clone(),
            is_outside: self.active_area.kind == AreaKind::Outside,
            current_tile,
            dimensions: self.active_area.dimensions.map(|(w, l)| [w, l]),
            cursor_pos: self.hover_world.map(|v| [v.x, v.y, v.z]),
            dirty_count: self.dirty_count(),
            save_state: self.save_state.clone(),
            tool: match self.tool {
                EditorTool::Heightmap => "heightmap",
                EditorTool::Texture => "texture",
                EditorTool::Road => "road",
                EditorTool::Decoration => "decoration",
                EditorTool::Walkability => "walkability",
            },
            height_mode: match self.height_mode {
                HeightMode::Raise => "raise",
                HeightMode::Lower => "lower",
                HeightMode::Smooth => "smooth",
                HeightMode::Flatten => "flatten",
            },
            texture_layer: self.texture_layer,
            road_submode: match self.road_submode {
                RoadSubMode::Place => "place",
                RoadSubMode::Edit => "edit",
            },
            road_style: match self.road_style {
                RoadStyle::Curve => "curve",
                RoadStyle::Straight => "straight",
            },
            road_material: self.road_material,
            road_width: self.road_width,
            road_shoulder: self.road_shoulder,
            road_draft_points: self.road_draft.as_ref().map(|d| d.points.len()).unwrap_or(0),
            selected_road,
            decoration_submode: match self.decoration_submode {
                DecorationSubMode::Place => "place",
                DecorationSubMode::Move => "move",
                DecorationSubMode::FreeMove => "free_move",
                DecorationSubMode::Rotate => "rotate",
            },
            selected_prop,
            preview_rotation_deg: self
                .preview_rotation
                .map(|r| r.to_degrees().rem_euclid(360.0)),
            preview_scale: self.preview_scale,
            preview_cast_shadow: self.preview_cast_shadow,
            walkability_mode: match self.walkability_mode {
                WalkabilityMode::Block => "block",
                WalkabilityMode::Walkable => "walkable",
            },
            character_studio: self.character_studio.status_snapshot(),
        }
    }
}
