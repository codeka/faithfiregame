use std::collections::{HashMap, HashSet};

use glam::Vec3;
use shared::world::{AreaKind, AreaMetadata, AssetEntry, InsideVisData, TILE_SIZE, TileVisData};

use crate::camera::{Camera, EditorCameraController};
use crate::glb_loader::parse_glb;
use crate::input::InputState;
use crate::render::{PropMeshGpu, TerrainMeshGpu};

use super::gizmo::{GizmoAxis, GizmoDragState};
use super::types::{
    DecorationSubMode, EditorInsideState, EditorStatusSnapshot, EditorTileState, EditorTool,
    HeightMode, PendingSave, SelectedPropInfo,
};

pub struct EditorState {
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
    pub brush_radius: f32,
    pub brush_strength: f32,
    pub texture_layer: usize,

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
            active_area: AreaMetadata {
                id: "main".to_owned(),
                name: "Main World".to_owned(),
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
            brush_radius: 4.5,
            brush_strength: 3.5,
            texture_layer: 1, // Default brush paints Dirt so clicking immediately shows contrast on Grass
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
            tile.gpu.destroy(gl);
        }
        if let Some(inside) = self.inside_area.take() {
            inside.gpu.destroy(gl);
        }
        self.requested_tiles.clear();
        self.requested_inside = None;
        self.pending_tile_fetches.clear();
        self.pending_inside_fetch = None;
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

    pub fn set_catalog(&mut self, entries: Vec<AssetEntry>) {
        if let Some(first) = entries.first() {
            if !entries.iter().any(|e| e.id == self.active_asset_id) {
                self.active_asset_id = first.id.clone();
            }
        }
        for entry in &entries {
            if !self.prop_meshes.contains_key(&entry.id) && self.requested_assets.insert(entry.id.clone())
            {
                self.pending_asset_fetches.push(entry.clone());
            }
        }
        self.catalog = entries;
    }

    pub fn on_glb_loaded(&mut self, gl: &glow::Context, asset_id: String, bytes: &[u8]) {
        match parse_glb(bytes) {
            Ok(parsed) => match PropMeshGpu::from_parsed(gl, &parsed) {
                Ok(mesh) => {
                    self.prop_meshes.insert(asset_id, mesh);
                }
                Err(e) => web_sys::console::error_1(&format!("Prop GPU upload failed: {e}").into()),
            },
            Err(e) => web_sys::console::error_1(&format!("GLB parse failed: {e}").into()),
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
        let mut vis = vis_opt.unwrap_or_else(TileVisData::flat_default);

        if let Some(max_id) = vis.props.iter().map(|p| p.id).max() {
            self.next_prop_id = self.next_prop_id.max(max_id + 1);
        }

        if is_new_blank {
            self.copy_loaded_borders_into(tx, ty, &mut vis);
        }

        if let Ok(gpu) = TerrainMeshGpu::new(gl, TILE_SIZE, TILE_SIZE, &vis.heights, &vis.splat_map)
        {
            self.outside_tiles.insert(
                (tx, ty),
                EditorTileState {
                    vis,
                    gpu,
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
        }
    }

    pub fn on_inside_loaded(&mut self, gl: &glow::Context, area_id: String, vis: InsideVisData) {
        if self.active_area.id != area_id {
            return;
        }
        if let Some(max_id) = vis.props.iter().map(|p| p.id).max() {
            self.next_prop_id = self.next_prop_id.max(max_id + 1);
        }
        if let Some(old) = self.inside_area.take() {
            old.gpu.destroy(gl);
        }
        if let Ok(gpu) = TerrainMeshGpu::new(
            gl,
            vis.width as usize,
            vis.length as usize,
            &vis.heights,
            &vis.splat_map,
        ) {
            let state = EditorInsideState {
                vis,
                gpu,
                dirty: false,
            };
            self.inside_area = Some(state);
            self.refresh_inside_mesh(gl);
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
        // 1. Stream visible tiles around editor camera target
        if self.active_area.kind == AreaKind::Outside {
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

        // 3. Hotkeys for decoration mode (Q/E rotate, Delete removes selected prop)
        if self.tool == EditorTool::Decoration {
            let rot_speed = 2.0 * dt;
            let mut delta_rot = 0.0;
            if input.is_key_down("KeyQ") {
                delta_rot -= rot_speed;
            }
            if input.is_key_down("KeyE") {
                delta_rot += rot_speed;
            }
            if delta_rot.abs() > 0.0 {
                if let Some(id) = self.selected_prop_id {
                    self.rotate_prop_by(id, delta_rot, now_ms);
                } else {
                    self.preview_rotation[1] =
                        (self.preview_rotation[1] + delta_rot).rem_euclid(std::f32::consts::TAU);
                }
            }
            if input.is_key_down("Delete") || input.is_key_down("Backspace") {
                self.delete_selected_prop(now_ms);
            }
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
                EditorTool::Decoration => self.handle_decoration_tool(camera, input, now_ms),
            }
        }

        if input.left_just_released {
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
        let area_id = self.active_area.id.clone();
        if self.active_area.kind == AreaKind::Outside {
            for (&(tx, ty), tile) in &mut self.outside_tiles {
                if tile.dirty {
                    tile.vis.recompute_slope_walkability();
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
                inside.vis.recompute_slope_walkability();
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

        EditorStatusSnapshot {
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
                EditorTool::Decoration => "decoration",
            },
            height_mode: match self.height_mode {
                HeightMode::Raise => "raise",
                HeightMode::Lower => "lower",
                HeightMode::Smooth => "smooth",
                HeightMode::Flatten => "flatten",
            },
            texture_layer: self.texture_layer,
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
        }
    }
}
