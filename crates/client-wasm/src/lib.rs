mod camera;
#[cfg(feature = "editor")]
mod editor;
mod engine;
mod glb_loader;
mod input;
mod net;
mod render;

use std::cell::RefCell;
use std::rc::Rc;

#[cfg(feature = "editor")]
use shared::world::AreaMetadata;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use web_sys::WebGl2RenderingContext;
use web_sys::HtmlCanvasElement;

#[cfg(feature = "editor")]
use crate::camera::EditorCameraController;
#[cfg(not(feature = "editor"))]
use crate::camera::GameplayCameraController;
use crate::camera::Camera;
#[cfg(feature = "editor")]
use crate::editor::{
    DecorationSubMode, EditorState, EditorTool, HeightMode,
    io::{dispatch_pending_saves, fetch_asset_catalog},
};
use crate::engine::{EngineInner, step_frame};
use crate::input::{InputState, attach_input_listeners};
use crate::net::NetClient;
use crate::render::Renderer;

#[wasm_bindgen]
pub struct EngineHandle {
    inner: Rc<RefCell<EngineInner>>,
    raf_cell: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>>,
    _listeners: Vec<Box<dyn std::any::Any>>,
}

impl Drop for EngineHandle {
    fn drop(&mut self) {
        *self.raf_cell.borrow_mut() = None;
    }
}

impl EngineHandle {
    #[allow(dead_code)]
    fn init_with_gl(canvas: HtmlCanvasElement, gl: glow::Context) -> Result<EngineHandle, JsValue> {
        let renderer = Renderer::new(gl).map_err(|e| JsValue::from_str(&e))?;

        let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
        let now_ms = window.performance().map(|p| p.now()).unwrap_or(0.0);

        let loc = window.location();
        let proto = if loc.protocol().unwrap_or_default() == "https:" {
            "wss"
        } else {
            "ws"
        };
        let host = loc.host().unwrap_or_else(|_| "127.0.0.1:4200".to_owned());
        let ws_url = format!("{proto}://{host}/ws");
        let net = NetClient::connect(&ws_url, "editor".to_owned()).ok();

        let mut camera = Camera::default();
        let rect = canvas.get_bounding_client_rect();
        let cw = rect.width().max(1.0) as f32;
        let ch = rect.height().max(1.0) as f32;
        camera.aspect = cw / ch;

        let inner = Rc::new(RefCell::new(EngineInner {
            canvas: canvas.clone(),
            renderer,
            camera,
            input: InputState {
                canvas_width: cw,
                canvas_height: ch,
                ..Default::default()
            },
            #[cfg(not(feature = "editor"))]
            gameplay_cam: GameplayCameraController::default(),
            #[cfg(feature = "editor")]
            editor_cam: EditorCameraController::default(),
            #[cfg(feature = "editor")]
            editor: EditorState::new(),
            _net: net,
            last_time_ms: now_ms,
        }));

        let listeners = attach_input_listeners(&window, &canvas, &inner)?;

        #[cfg(feature = "editor")]
        fetch_asset_catalog(&inner);

        let raf_inner = Rc::clone(&inner);
        let raf_window = window.clone();
        let raf_cell: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
        let raf_cell_clone = Rc::clone(&raf_cell);

        let closure = Closure::<dyn FnMut(f64)>::new(move |time_ms: f64| {
            step_frame(&raf_inner, time_ms);
            if let Some(cb) = raf_cell_clone.borrow().as_ref() {
                let _ = raf_window.request_animation_frame(cb.as_ref().unchecked_ref());
            }
        });

        let _ = window.request_animation_frame(closure.as_ref().unchecked_ref());
        *raf_cell.borrow_mut() = Some(closure);

        Ok(EngineHandle {
            inner,
            raf_cell,
            _listeners: listeners,
        })
    }
}

#[wasm_bindgen]
impl EngineHandle {
    #[wasm_bindgen(constructor)]
    pub fn new(canvas: HtmlCanvasElement) -> Result<EngineHandle, JsValue> {
        console_error_panic_hook::set_once();

        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = canvas;
            return Err(JsValue::from_str("WebGL2 EngineHandle requires wasm32 target"));
        }

        #[cfg(target_arch = "wasm32")]
        {
            let webgl2: WebGl2RenderingContext = canvas
                .get_context("webgl2")?
                .ok_or_else(|| JsValue::from_str("WebGL2 context unavailable"))?
                .dyn_into()?;
            let gl = glow::Context::from_webgl2_context(webgl2);
            Self::init_with_gl(canvas, gl)
        }
    }

    pub fn is_editor_enabled() -> bool {
        cfg!(feature = "editor")
    }

    pub fn resize(&self, width: u32, height: u32) {
        let mut st = self.inner.borrow_mut();
        let w = width.max(1);
        let h = height.max(1);
        st.canvas.set_width(w);
        st.canvas.set_height(h);
        st.camera.aspect = w as f32 / h as f32;
        st.input.canvas_width = st.canvas.client_width().max(1) as f32;
        st.input.canvas_height = st.canvas.client_height().max(1) as f32;
    }

    #[cfg(feature = "editor")]
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

    #[cfg(not(feature = "editor"))]
    pub fn set_area(&self, _area_json: &str) -> Result<(), JsValue> {
        Ok(())
    }

    #[cfg(feature = "editor")]
    pub fn set_editor_tool(&self, tool: &str) {
        let mut st = self.inner.borrow_mut();
        st.editor.tool = match tool {
            "texture" => EditorTool::Texture,
            "decoration" => EditorTool::Decoration,
            _ => EditorTool::Heightmap,
        };
    }

    #[cfg(feature = "editor")]
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

    #[cfg(feature = "editor")]
    pub fn set_brush_radius(&self, radius: f32) {
        self.inner.borrow_mut().editor.brush_radius = radius.clamp(0.5, 24.0);
    }

    #[cfg(feature = "editor")]
    pub fn set_brush_strength(&self, strength: f32) {
        self.inner.borrow_mut().editor.brush_strength = strength.clamp(0.2, 20.0);
    }

    #[cfg(feature = "editor")]
    pub fn set_texture_layer(&self, layer: u32) {
        self.inner.borrow_mut().editor.texture_layer = (layer as usize).min(3);
    }

    #[cfg(feature = "editor")]
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

    #[cfg(feature = "editor")]
    pub fn set_decoration_asset(&self, asset_id: &str) {
        self.inner.borrow_mut().editor.active_asset_id = asset_id.to_owned();
    }

    #[cfg(feature = "editor")]
    pub fn set_selected_prop_rotation_axis_deg(&self, axis: u8, degrees: f32) {
        let now_ms = browser_now_ms();
        self.inner
            .borrow_mut()
            .editor
            .set_selected_or_preview_rotation_axis_deg(axis as usize, degrees, now_ms);
    }

    #[cfg(feature = "editor")]
    pub fn reset_selected_prop_rotation(&self) {
        let now_ms = browser_now_ms();
        self.inner
            .borrow_mut()
            .editor
            .reset_selected_or_preview_rotation(now_ms);
    }

    #[cfg(feature = "editor")]
    pub fn set_selected_prop_scale(&self, scale: f32) {
        let now_ms = web_sys::window()
            .and_then(|w| w.performance())
            .map(|p| p.now())
            .unwrap_or(0.0);
        self.inner
            .borrow_mut()
            .editor
            .set_selected_or_preview_scale(scale, now_ms);
    }

    #[cfg(feature = "editor")]
    pub fn set_selected_prop_cast_shadow(&self, cast_shadow: bool) {
        let now_ms = browser_now_ms();
        self.inner
            .borrow_mut()
            .editor
            .set_selected_or_preview_cast_shadow(cast_shadow, now_ms);
    }

    #[cfg(feature = "editor")]
    pub fn delete_selected_prop(&self) {
        let now_ms = web_sys::window()
            .and_then(|w| w.performance())
            .map(|p| p.now())
            .unwrap_or(0.0);
        self.inner.borrow_mut().editor.delete_selected_prop(now_ms);
    }

    #[cfg(feature = "editor")]
    pub fn snap_selected_prop_to_terrain(&self) {
        let now_ms = web_sys::window()
            .and_then(|w| w.performance())
            .map(|p| p.now())
            .unwrap_or(0.0);
        self.inner
            .borrow_mut()
            .editor
            .snap_selected_prop_to_terrain(now_ms);
    }

    #[cfg(feature = "editor")]
    pub fn save_editor_changes(&self) {
        let saves = {
            let mut st = self.inner.borrow_mut();
            st.editor.autosave_deadline_ms = None;
            st.editor.queue_save_all();
            std::mem::take(&mut st.editor.pending_saves)
        };
        dispatch_pending_saves(&self.inner, saves);
    }

    #[cfg(feature = "editor")]
    pub fn get_editor_status_json(&self) -> String {
        let st = self.inner.borrow();
        let snap = st.editor.status_snapshot(&st.editor_cam);
        serde_json::to_string(&snap).unwrap_or_else(|_| "{}".to_owned())
    }

    #[cfg(not(feature = "editor"))]
    pub fn get_editor_status_json(&self) -> String {
        "{}".to_owned()
    }
}

#[cfg(feature = "editor")]
fn browser_now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}
