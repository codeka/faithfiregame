use std::cell::RefCell;
use std::rc::Rc;

use shared::world::AreaMetadata;
use wasm_bindgen::prelude::*;

use crate::EngineHandle;
use crate::engine::EngineInner;

use super::{DecorationSubMode, EditorTool, HeightMode, io::dispatch_pending_saves};

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
        let mut st = self.inner.borrow_mut();
        st.editor.tool = match tool {
            "texture" => EditorTool::Texture,
            "decoration" => EditorTool::Decoration,
            _ => EditorTool::Heightmap,
        };
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
        self.inner.borrow_mut().editor.texture_layer =
            (layer as usize).min(shared::world::TERRAIN_MATERIAL_COUNT - 1);
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
        self.inner.borrow_mut().editor.active_asset_id = asset_id.to_owned();
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

    pub fn save_editor_changes(&self) {
        let saves = {
            let mut st = self.inner.borrow_mut();
            st.editor.autosave_deadline_ms = None;
            st.editor.queue_save_all();
            std::mem::take(&mut st.editor.pending_saves)
        };
        dispatch_pending_saves(&self.inner, saves);
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
