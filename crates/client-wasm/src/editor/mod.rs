use std::collections::{HashMap, HashSet};

use glam::{Vec3, Vec4};
use serde::Serialize;
use shared::world::{
    AreaKind, AreaMetadata, AssetEntry, InsideVisData, PropPlacement, TILE_SIZE, TILE_VERTICES,
    TileVisData, pack_splat, unpack_splat,
};

use crate::camera::{Camera, EditorCameraController, InputState};
use crate::glb_loader::parse_glb;
use crate::render::{BrushOverlay, PropMeshGpu, Renderer, TerrainMeshGpu, prop_model_matrix};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorTool {
    Heightmap,
    Texture,
    Decoration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeightMode {
    Raise,
    Lower,
    Smooth,
    Flatten,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorationSubMode {
    Place,
    Move,
    Rotate,
}

pub enum PendingSave {
    Outside {
        area_id: String,
        tx: i32,
        ty: i32,
        bytes: Vec<u8>,
    },
    Inside {
        area_id: String,
        bytes: Vec<u8>,
    },
}

pub struct EditorTileState {
    pub vis: TileVisData,
    pub gpu: TerrainMeshGpu,
    pub dirty: bool,
}

pub struct EditorInsideState {
    pub vis: InsideVisData,
    pub gpu: TerrainMeshGpu,
    pub dirty: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedPropInfo {
    pub id: u64,
    pub asset_id: String,
    pub rotation_deg: f32,
    pub scale: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorStatusSnapshot {
    pub area_id: String,
    pub area_name: String,
    pub is_outside: bool,
    pub current_tile: [i32; 2],
    pub dimensions: Option<[u32; 2]>,
    pub cursor_pos: Option<[f32; 3]>,
    pub dirty_count: usize,
    pub save_state: String,
    pub tool: &'static str,
    pub height_mode: &'static str,
    pub texture_layer: usize,
    pub decoration_submode: &'static str,
    pub selected_prop: Option<SelectedPropInfo>,
    pub preview_rotation_deg: f32,
    pub preview_scale: f32,
}

pub struct EditorState {
    pub active_area: AreaMetadata,
    pub outside_tiles: HashMap<(i32, i32), EditorTileState>,
    pub requested_tiles: HashSet<(String, i32, i32)>,
    pub inside_area: Option<EditorInsideState>,
    pub requested_inside: Option<String>,

    pub catalog: Vec<AssetEntry>,
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
    pub preview_rotation_y: f32,
    pub preview_scale: f32,
    pub selected_prop_id: Option<u64>,
    pub dragging_prop: bool,
    pub rotating_prop: bool,
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
            preview_rotation_y: 0.0,
            preview_scale: 1.0,
            selected_prop_id: None,
            dragging_prop: false,
            rotating_prop: false,
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
                    self.preview_rotation_y =
                        (self.preview_rotation_y + delta_rot).rem_euclid(std::f32::consts::TAU);
                }
            }
            if input.is_key_down("Delete") || input.is_key_down("Backspace") {
                self.delete_selected_prop(now_ms);
            }
        }

        // 4. Mouse tool interaction (ignore left-click while middle-mouse orbiting)
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
            self.rotating_prop = false;
        }

        // 5. Debounced auto-save after finishing an edit stroke
        if let Some(deadline) = self.autosave_deadline_ms {
            if now_ms >= deadline && !input.left_mouse_down {
                self.autosave_deadline_ms = None;
                self.queue_save_all();
            }
        }
    }

    pub fn render(&self, renderer: &Renderer, camera: &Camera) {
        let show_brush = self.hover_world.is_some()
            && matches!(self.tool, EditorTool::Heightmap | EditorTool::Texture);

        let brush = BrushOverlay {
            active: show_brush,
            center: self.hover_world.unwrap_or(Vec3::ZERO),
            radius: self.brush_radius,
            color: match self.tool {
                EditorTool::Heightmap => [0.25, 0.85, 1.0],
                EditorTool::Texture => match self.texture_layer {
                    0 => [0.35, 0.90, 0.35],
                    1 => [0.85, 0.58, 0.30],
                    2 => [0.75, 0.78, 0.85],
                    _ => [0.95, 0.85, 0.45],
                },
                EditorTool::Decoration => [1.0, 1.0, 1.0],
            },
        };

        if self.active_area.kind == AreaKind::Outside {
            for (&(tx, ty), tile) in &self.outside_tiles {
                let ox = (tx * TILE_SIZE as i32) as f32;
                let oz = (ty * TILE_SIZE as i32) as f32;
                renderer.draw_terrain_chunk(camera, &tile.gpu, ox, oz, true, &brush);
            }
            for tile in self.outside_tiles.values() {
                self.draw_prop_list(renderer, camera, &tile.vis.props);
            }
        } else if let Some(inside) = &self.inside_area {
            renderer.draw_terrain_chunk(camera, &inside.gpu, 0.0, 0.0, true, &brush);
            self.draw_prop_list(renderer, camera, &inside.vis.props);
        }

        // Ghost preview when placing a decoration
        if self.tool == EditorTool::Decoration && self.decoration_submode == DecorationSubMode::Place
        {
            if let (Some(hit), Some(mesh)) = (self.hover_world, self.prop_meshes.get(&self.active_asset_id))
            {
                let ghost = PropPlacement {
                    id: 0,
                    asset_id: self.active_asset_id.clone(),
                    position: hit.to_array(),
                    rotation_y: self.preview_rotation_y,
                    scale: [self.preview_scale; 3],
                };
                renderer.draw_prop(camera, mesh, &ghost, Vec4::new(0.65, 1.0, 0.78, 0.60));
                renderer.draw_selection_ring(camera, &ghost, 1.1 * self.preview_scale);
            }
        }
    }

    fn draw_prop_list(&self, renderer: &Renderer, camera: &Camera, props: &[PropPlacement]) {
        for prop in props {
            if let Some(mesh) = self.prop_meshes.get(&prop.asset_id) {
                let selected = self.tool == EditorTool::Decoration && self.selected_prop_id == Some(prop.id);
                let tint = if selected {
                    Vec4::new(1.30, 1.18, 0.72, 1.0)
                } else {
                    Vec4::ONE
                };
                renderer.draw_prop(camera, mesh, prop, tint);
                if selected {
                    let extent = (mesh.aabb_max.x - mesh.aabb_min.x)
                        .max(mesh.aabb_max.z - mesh.aabb_min.z)
                        * 0.55
                        * prop.scale[0];
                    renderer.draw_selection_ring(camera, prop, extent.max(0.9));
                }
            }
        }
    }

    fn handle_heightmap_tool(&mut self, gl: &glow::Context, input: &InputState, dt: f32, now_ms: f64) {
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
                inside.dirty = true;
                self.refresh_inside_mesh(gl);
                self.mark_unsaved(now_ms);
            }
        }
    }

    fn handle_texture_tool(&mut self, gl: &glow::Context, input: &InputState, dt: f32, now_ms: f64) {
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

    fn handle_decoration_tool(&mut self, camera: &Camera, input: &InputState, now_ms: f64) {
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

    fn rotate_prop_by(&mut self, prop_id: u64, delta_rad: f32, now_ms: f64) {
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

    fn find_selected_prop(&self) -> Option<&PropPlacement> {
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

    fn raycast_terrain(&self, orig: Vec3, dir: Vec3) -> Option<Vec3> {
        let mut t = 0.5f32;
        let step = 0.75f32;
        let mut prev_t: Option<f32> = None;
        let mut prev_above = false;

        while t <= 320.0 {
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

    pub fn terrain_height_at(&self, wx: f32, wz: f32) -> Option<f32> {
        if self.active_area.kind == AreaKind::Outside {
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

    fn sample_vertex_height(&self, wx: i32, wz: i32) -> Option<f32> {
        if self.active_area.kind == AreaKind::Outside {
            // Check canonical tile owning cell/vertex first, then fall back to border neighbor
            let tx = wx.div_euclid(TILE_SIZE as i32);
            let ty = wz.div_euclid(TILE_SIZE as i32);
            let vx = wx.rem_euclid(TILE_SIZE as i32) as usize;
            let vz = wz.rem_euclid(TILE_SIZE as i32) as usize;
            if let Some(tile) = self.outside_tiles.get(&(tx, ty)) {
                return Some(tile.vis.heights[vz * TILE_VERTICES + vx]);
            }
            // Handle right/bottom boundary vertices (vx == 64 or vz == 64)
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
        } else if let Some(inside) = &self.inside_area {
            let w = inside.vis.width as i32;
            let l = inside.vis.length as i32;
            if wx < 0 || wz < 0 || wx > w || wz > l {
                return None;
            }
            Some(inside.vis.heights[wz as usize * (w as usize + 1) + wx as usize])
        } else {
            None
        }
    }

    fn copy_loaded_borders_into(&self, tx: i32, ty: i32, vis: &mut TileVisData) {
        let ox = tx * TILE_SIZE as i32;
        let oz = ty * TILE_SIZE as i32;
        for vz in 0..TILE_VERTICES {
            for vx in 0..TILE_VERTICES {
                if vx == 0 || vx == TILE_SIZE || vz == 0 || vz == TILE_SIZE {
                    if let Some(h) = self.sample_vertex_height(ox + vx as i32, oz + vz as i32) {
                        vis.heights[vz * TILE_VERTICES + vx] = h;
                    }
                }
            }
        }
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
                    let hl = self.sample_vertex_height(wx - 1, wz).unwrap_or(hc);
                    let hr = self.sample_vertex_height(wx + 1, wz).unwrap_or(hc);
                    let hd = self.sample_vertex_height(wx, wz - 1).unwrap_or(hc);
                    let hu = self.sample_vertex_height(wx, wz + 1).unwrap_or(hc);
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

    fn mark_unsaved(&mut self, now_ms: f64) {
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
            rotation_deg: p.rotation_y.to_degrees().rem_euclid(360.0),
            scale: p.scale[0],
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
                DecorationSubMode::Rotate => "rotate",
            },
            selected_prop,
            preview_rotation_deg: self.preview_rotation_y.to_degrees().rem_euclid(360.0),
            preview_scale: self.preview_scale,
        }
    }
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
