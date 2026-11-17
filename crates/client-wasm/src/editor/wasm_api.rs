use std::cell::RefCell;
use std::rc::Rc;

use shared::character::{CharacterCatalog, CharacterDefinition};
use shared::world::{AreaMetadata, RoadStyle};
use wasm_bindgen::prelude::*;

use crate::EngineHandle;
use crate::engine::EngineInner;

use super::{
    DecorationSubMode, EditorMode, EditorTool, HeightMode, RoadSubMode, WalkabilityMode,
    io::dispatch_pending_saves,
};

#[wasm_bindgen]
pub struct EditorEngineHandle {
    inner: Rc<RefCell<EngineInner>>,
}

#[wasm_bindgen]
impl EditorEngineHandle {
    #[wasm_bindgen(constructor)]
    pub fn new(engine: &EngineHandle) -> EditorEngineHandle {
        EditorEngineHandle {
            inner: Rc::clone(&engine.inner),
        }
    }

    pub fn set_area(&self, area_json: &str) -> Result<(), JsValue> {
        let meta: AreaMetadata = serde_json::from_str(area_json)
            .map_err(|e| JsValue::from_str(&format!("invalid AreaMetadata json: {e}")))?;
        let mut st = self.inner.borrow_mut();
        let EngineInner {
            renderer,
            editor,
            editor_cam,
            ..
        } = &mut *st;
        editor.switch_area(&renderer.gl, meta, editor_cam);
        Ok(())
    }

    pub fn set_editor_tool(&self, tool: &str) {
        let next = match tool {
            "texture" => EditorTool::Texture,
            "road" => EditorTool::Road,
            "decoration" => EditorTool::Decoration,
            "walkability" => EditorTool::Walkability,
            _ => EditorTool::Heightmap,
        };
        let mut st = self.inner.borrow_mut();
        let EngineInner {
            renderer, editor, ..
        } = &mut *st;
        if editor.tool == EditorTool::Road && next != EditorTool::Road {
            editor.cancel_road_draft(&renderer.gl);
        }
        editor.tool = next;
    }

    pub fn set_height_mode(&self, mode: &str) {
        let mut st = self.inner.borrow_mut();
        st.editor.height_mode = match mode {
            "lower" => HeightMode::Lower,
            "smooth" => HeightMode::Smooth,
            "flatten" => HeightMode::Flatten,
            _ => HeightMode::Raise,
        };
        st.editor.flatten_target_y = None;
    }

    pub fn set_brush_radius(&self, radius: f32) {
        self.inner.borrow_mut().editor.brush_radius = radius.clamp(0.5, 24.0);
    }

    pub fn set_brush_strength(&self, strength: f32) {
        self.inner.borrow_mut().editor.brush_strength = strength.clamp(0.2, 20.0);
    }

    pub fn set_texture_layer(&self, layer: u32) {
        let mut st = self.inner.borrow_mut();
        let max_layer = st
            .editor
            .max_texture_layer
            .min(shared::world::MAX_TERRAIN_MATERIALS - 1);
        st.editor.texture_layer = (layer as usize).min(max_layer);
    }

    pub fn set_road_submode(&self, submode: &str) {
        let mode = match submode {
            "edit" => RoadSubMode::Edit,
            _ => RoadSubMode::Place,
        };
        let mut st = self.inner.borrow_mut();
        let EngineInner {
            renderer, editor, ..
        } = &mut *st;
        editor.set_road_submode(&renderer.gl, mode);
    }

    pub fn set_road_style(&self, style: &str) {
        let now_ms = browser_now_ms();
        let road_style = match style {
            "straight" => RoadStyle::Straight,
            _ => RoadStyle::Curve,
        };
        let mut st = self.inner.borrow_mut();
        let EngineInner {
            renderer, editor, ..
        } = &mut *st;
        editor.set_road_style(&renderer.gl, road_style, now_ms);
    }

    pub fn set_road_material(&self, material: u8) {
        let now_ms = browser_now_ms();
        let mut st = self.inner.borrow_mut();
        let EngineInner {
            renderer, editor, ..
        } = &mut *st;
        editor.set_road_material(&renderer.gl, material, now_ms);
    }

    pub fn set_road_width(&self, width: f32) {
        let now_ms = browser_now_ms();
        let mut st = self.inner.borrow_mut();
        let EngineInner {
            renderer, editor, ..
        } = &mut *st;
        editor.set_road_width(&renderer.gl, width, now_ms);
    }

    pub fn set_road_shoulder(&self, shoulder: f32) {
        let now_ms = browser_now_ms();
        let mut st = self.inner.borrow_mut();
        let EngineInner {
            renderer, editor, ..
        } = &mut *st;
        editor.set_road_shoulder(&renderer.gl, shoulder, now_ms);
    }

    pub fn finish_active_road(&self) {
        let now_ms = browser_now_ms();
        let mut st = self.inner.borrow_mut();
        let EngineInner {
            renderer, editor, ..
        } = &mut *st;
        editor.finish_road_draft(&renderer.gl, now_ms);
    }

    pub fn cancel_active_road(&self) {
        let mut st = self.inner.borrow_mut();
        let EngineInner {
            renderer, editor, ..
        } = &mut *st;
        editor.cancel_road_draft(&renderer.gl);
    }

    pub fn relevel_selected_road(&self) {
        let now_ms = browser_now_ms();
        let mut st = self.inner.borrow_mut();
        let EngineInner {
            renderer, editor, ..
        } = &mut *st;
        editor.relevel_selected_road(&renderer.gl, now_ms);
    }

    pub fn delete_selected_road_node(&self) {
        let now_ms = browser_now_ms();
        let mut st = self.inner.borrow_mut();
        let EngineInner {
            renderer, editor, ..
        } = &mut *st;
        editor.delete_selected_road_node(&renderer.gl, now_ms);
    }

    pub fn delete_selected_road(&self) {
        let now_ms = browser_now_ms();
        let mut st = self.inner.borrow_mut();
        let EngineInner {
            renderer, editor, ..
        } = &mut *st;
        editor.delete_selected_road(&renderer.gl, now_ms);
    }

    pub fn set_decoration_submode(&self, submode: &str) {
        let mut st = self.inner.borrow_mut();
        st.editor.decoration_submode = match submode {
            "move" => DecorationSubMode::Move,
            "free_move" => DecorationSubMode::FreeMove,
            "rotate" => DecorationSubMode::Rotate,
            _ => DecorationSubMode::Place,
        };
        st.editor.dragging_prop = false;
        st.editor.gizmo_drag = None;
        st.editor.hovered_gizmo_axis = None;
        st.editor.rotate_drag = None;
        st.editor.hovered_rotate_axis = None;
    }

    pub fn set_decoration_asset(&self, asset_id: &str) {
        self.inner
            .borrow_mut()
            .editor
            .select_decoration_asset(asset_id);
    }

    pub fn set_selected_prop_rotation_axis_deg(&self, axis: u8, degrees: f32) {
        let now_ms = browser_now_ms();
        self.inner
            .borrow_mut()
            .editor
            .set_selected_or_preview_rotation_axis_deg(axis as usize, degrees, now_ms);
    }

    pub fn reset_selected_prop_rotation(&self) {
        let now_ms = browser_now_ms();
        self.inner
            .borrow_mut()
            .editor
            .reset_selected_or_preview_rotation(now_ms);
    }

    pub fn set_selected_prop_scale(&self, scale: f32) {
        let now_ms = browser_now_ms();
        self.inner
            .borrow_mut()
            .editor
            .set_selected_or_preview_scale(scale, now_ms);
    }

    pub fn set_selected_prop_cast_shadow(&self, cast_shadow: bool) {
        let now_ms = browser_now_ms();
        self.inner
            .borrow_mut()
            .editor
            .set_selected_or_preview_cast_shadow(cast_shadow, now_ms);
    }

    pub fn duplicate_selected_prop(&self) {
        let now_ms = browser_now_ms();
        self.inner
            .borrow_mut()
            .editor
            .duplicate_selected_prop(now_ms);
    }

    pub fn delete_selected_prop(&self) {
        let now_ms = browser_now_ms();
        self.inner.borrow_mut().editor.delete_selected_prop(now_ms);
    }

    pub fn snap_selected_prop_to_terrain(&self) {
        let now_ms = browser_now_ms();
        self.inner
            .borrow_mut()
            .editor
            .snap_selected_prop_to_terrain(now_ms);
    }

    pub fn set_walkability_mode(&self, mode: &str) {
        let mut st = self.inner.borrow_mut();
        st.editor.walkability_mode = match mode {
            "walkable" => WalkabilityMode::Walkable,
            _ => WalkabilityMode::Block,
        };
    }

    pub fn bake_props_to_walkability(&self) {
        let now_ms = browser_now_ms();
        let mut st = self.inner.borrow_mut();
        let EngineInner {
            renderer, editor, ..
        } = &mut *st;
        editor.bake_props_into_walkability(&renderer.gl, now_ms);
    }

    pub fn reset_walkability_to_slope(&self) {
        let now_ms = browser_now_ms();
        let mut st = self.inner.borrow_mut();
        let EngineInner {
            renderer, editor, ..
        } = &mut *st;
        editor.reset_walkability_to_slope(&renderer.gl, now_ms);
    }

    pub fn import_local_glb(
        &self,
        slug: &str,
        bytes: &[u8],
        spawn_in_scene: bool,
    ) -> Result<String, JsValue> {
        let now_ms = browser_now_ms();
        let mut st = self.inner.borrow_mut();
        let EngineInner {
            renderer,
            editor,
            editor_cam,
            ..
        } = &mut *st;
        editor
            .import_local_glb(&renderer.gl, editor_cam, slug, bytes, spawn_in_scene, now_ms)
            .map_err(|e| JsValue::from_str(&e))
    }

    pub fn save_editor_changes(&self) {
        let saves = {
            let mut st = self.inner.borrow_mut();
            st.editor.autosave_deadline_ms = None;
            st.editor.queue_save_all();
            std::mem::take(&mut st.editor.pending_saves)
        };
        dispatch_pending_saves(&self.inner, saves);
    }

    pub fn set_editor_mode(&self, mode: &str) {
        let next = match mode {
            "characters" => EditorMode::Characters,
            "entities" => EditorMode::Entities,
            _ => EditorMode::World,
        };
        let mut st = self.inner.borrow_mut();
        st.editor.mode = next;
    }

    pub fn set_character_catalog_json(&self, catalog_json: &str) -> Result<(), JsValue> {
        let catalog: CharacterCatalog = serde_json::from_str(catalog_json)
            .map_err(|e| JsValue::from_str(&format!("invalid CharacterCatalog json: {e}")))?;
        self.inner
            .borrow_mut()
            .editor
            .character_studio
            .set_catalog(catalog);
        Ok(())
    }

    pub fn set_active_character_json(&self, char_json: &str) -> Result<(), JsValue> {
        let def: CharacterDefinition = serde_json::from_str(char_json)
            .map_err(|e| JsValue::from_str(&format!("invalid CharacterDefinition json: {e}")))?;
        self.inner
            .borrow_mut()
            .editor
            .character_studio
            .set_active_character(def);
        Ok(())
    }

    pub fn load_character_glb_bytes(&self, path: &str, bytes: &[u8]) -> Result<(), JsValue> {
        let mut st = self.inner.borrow_mut();
        let EngineInner {
            renderer, editor, ..
        } = &mut *st;
        editor
            .character_studio
            .on_glb_loaded(&renderer.gl, path.to_owned(), bytes)
            .map_err(|e| JsValue::from_str(&e))
    }

    pub fn set_character_anim_clip(&self, clip: &str) {
        self.inner
            .borrow_mut()
            .editor
            .character_studio
            .select_clip(clip);
    }

    pub fn set_character_anim_playing(&self, playing: bool) {
        self.inner
            .borrow_mut()
            .editor
            .character_studio
            .playing = playing;
    }

    pub fn set_character_anim_time(&self, time_sec: f32) {
        self.inner
            .borrow_mut()
            .editor
            .character_studio
            .anim_time = time_sec.max(0.0);
    }

    pub fn set_character_anim_speed(&self, speed: f32) {
        self.inner
            .borrow_mut()
            .editor
            .character_studio
            .playback_speed = speed.clamp(0.1, 3.0);
    }

    pub fn set_character_debug_skeleton(&self, show: bool) {
        self.inner
            .borrow_mut()
            .editor
            .character_studio
            .show_skeleton_debug = show;
    }

    pub fn set_character_turntable(&self, enabled: bool) {
        self.inner
            .borrow_mut()
            .editor
            .character_studio
            .turntable = enabled;
    }

    pub fn set_selected_socket_id(&self, socket_id: &str) {
        let mut st = self.inner.borrow_mut();
        st.editor.character_studio.selected_socket_id = if socket_id.is_empty() {
            None
        } else {
            Some(socket_id.to_owned())
        };
    }

    pub fn reset_character_studio_camera(&self) {
        self.inner.borrow_mut().editor.studio_cam.reset();
    }

    pub fn get_editor_status_json(&self) -> String {
        let st = self.inner.borrow();
        let snap = st.editor.status_snapshot(&st.editor_cam);
        serde_json::to_string(&snap).unwrap_or_else(|_| "{}".to_owned())
    }
}

fn browser_now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}
