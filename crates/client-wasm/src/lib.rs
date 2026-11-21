mod asset_loader;
mod camera;
mod character;
#[cfg(feature = "editor")]
mod editor;
mod engine;
mod game;
mod glb_loader;
mod input;
mod net;
mod render;

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use web_sys::WebGl2RenderingContext;
use web_sys::HtmlCanvasElement;

#[cfg(feature = "editor")]
use crate::camera::EditorCameraController;
use crate::camera::{Camera, GameplayCameraController};
#[cfg(feature = "editor")]
use crate::editor::EditorState;
#[cfg(feature = "editor")]
pub use crate::editor::EditorEngineHandle;
#[cfg(feature = "editor")]
use crate::engine::ClientRunMode;
use crate::engine::{EngineInner, step_frame};
use crate::game::{GameClientState, GamePhase};
use crate::input::{InputState, attach_input_listeners};
use crate::net::NetClient;
use crate::render::Renderer;

#[wasm_bindgen]
pub struct EngineHandle {
    pub(crate) inner: Rc<RefCell<EngineInner>>,
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
            ws_url,
            gameplay_cam: GameplayCameraController::default(),
            game: GameClientState::new(),
            #[cfg(feature = "editor")]
            active_run_mode: ClientRunMode::Game,
            #[cfg(feature = "editor")]
            editor_cam: EditorCameraController::default(),
            #[cfg(feature = "editor")]
            editor: EditorState::new(),
            net: None,
            last_time_ms: now_ms,
        }));

        let listeners = attach_input_listeners(&window, &canvas, &inner)?;

        asset_loader::spawn_asset_catalog_load(&inner);

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

    pub fn enter_landing_mode(&self) {
        let mut st = self.inner.borrow_mut();
        #[cfg(feature = "editor")]
        {
            st.active_run_mode = ClientRunMode::Game;
        }
        st.net = None;
        let EngineInner {
            canvas,
            renderer,
            game,
            ..
        } = &mut *st;
        game.clear_cursor(canvas);
        game.reset_to_idle(&renderer.gl);
    }

    pub fn connect_game_session(&self, session_token: &str) {
        let mut st = self.inner.borrow_mut();
        #[cfg(feature = "editor")]
        {
            st.active_run_mode = ClientRunMode::Game;
        }
        let canvas = st.canvas.clone();
        st.game.clear_cursor(&canvas);

        let already_active = !matches!(st.game.phase, GamePhase::Unauthorized | GamePhase::Error(_))
            && st
                .net
                .as_ref()
                .is_some_and(|n| !n.is_closed() && n.session_token() == session_token);
        if already_active {
            return;
        }

        let ws_url = st.ws_url.clone();
        st.net = None;
        {
            let EngineInner { renderer, game, .. } = &mut *st;
            game.begin_connecting(&renderer.gl);
        }

        match NetClient::connect(&ws_url, session_token.to_owned()) {
            Ok(client) => st.net = Some(client),
            Err(err) => st.game.phase = GamePhase::Error(err),
        }
    }

    pub fn disconnect_game_session(&self) {
        self.enter_landing_mode();
    }

    pub fn dismiss_game_popup(&self) {
        self.inner.borrow_mut().game.active_popup = None;
    }

    pub fn get_game_status_json(&self) -> String {
        let st = self.inner.borrow();
        serde_json::to_string(&st.game.status_snapshot()).unwrap_or_else(|_| "{}".to_owned())
    }
}
