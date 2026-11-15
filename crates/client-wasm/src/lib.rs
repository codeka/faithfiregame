mod camera;
#[cfg(feature = "editor")]
mod editor;
mod glb_loader;
mod net;
mod render;

use std::cell::RefCell;
use std::rc::Rc;

#[cfg(feature = "editor")]
use shared::world::{AreaMetadata, AssetEntry, InsideVisData, TileVisData};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
#[cfg(feature = "editor")]
use wasm_bindgen_futures::{JsFuture, spawn_local};
#[cfg(feature = "editor")]
use web_sys::{Request, RequestInit, RequestMode, Response, console};
#[cfg(target_arch = "wasm32")]
use web_sys::WebGl2RenderingContext;
use web_sys::{Element, HtmlCanvasElement, KeyboardEvent, MouseEvent, WheelEvent};

#[cfg(not(feature = "editor"))]
use crate::camera::GameplayCameraController;
use crate::camera::{Camera, CameraController, InputState};
#[cfg(feature = "editor")]
use crate::camera::EditorCameraController;
#[cfg(feature = "editor")]
use crate::editor::{DecorationSubMode, EditorState, EditorTool, HeightMode, PendingSave};
use crate::net::NetClient;
use crate::render::Renderer;

struct EngineInner {
    canvas: HtmlCanvasElement,
    renderer: Renderer,
    camera: Camera,
    input: InputState,
    #[cfg(not(feature = "editor"))]
    gameplay_cam: GameplayCameraController,
    #[cfg(feature = "editor")]
    editor_cam: EditorCameraController,
    #[cfg(feature = "editor")]
    editor: EditorState,
    _net: Option<NetClient>,
    last_time_ms: f64,
}

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
            "rotate" => DecorationSubMode::Rotate,
            _ => DecorationSubMode::Place,
        };
    }

    #[cfg(feature = "editor")]
    pub fn set_decoration_asset(&self, asset_id: &str) {
        self.inner.borrow_mut().editor.active_asset_id = asset_id.to_owned();
    }

    #[cfg(feature = "editor")]
    pub fn set_selected_prop_rotation_deg(&self, degrees: f32) {
        let now_ms = web_sys::window()
            .and_then(|w| w.performance())
            .map(|p| p.now())
            .unwrap_or(0.0);
        self.inner
            .borrow_mut()
            .editor
            .set_selected_or_preview_rotation_deg(degrees, now_ms);
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
    pub fn delete_selected_prop(&self) {
        let now_ms = web_sys::window()
            .and_then(|w| w.performance())
            .map(|p| p.now())
            .unwrap_or(0.0);
        self.inner.borrow_mut().editor.delete_selected_prop(now_ms);
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

fn step_frame(inner: &Rc<RefCell<EngineInner>>, time_ms: f64) {
    #[cfg(feature = "editor")]
    let (tile_fetches, inside_fetch, asset_fetches, saves);

    {
        let mut st = inner.borrow_mut();
        let dt = ((time_ms - st.last_time_ms) * 0.001).clamp(0.001, 0.1) as f32;
        st.last_time_ms = time_ms;

        let cw = st.canvas.client_width().max(1);
        let ch = st.canvas.client_height().max(1);
        if st.canvas.width() != cw as u32 || st.canvas.height() != ch as u32 {
            st.canvas.set_width(cw as u32);
            st.canvas.set_height(ch as u32);
        }
        st.camera.aspect = cw as f32 / ch as f32;
        st.input.canvas_width = cw as f32;
        st.input.canvas_height = ch as f32;

        #[cfg(feature = "editor")]
        {
            let EngineInner {
                renderer,
                camera,
                input,
                editor_cam,
                editor,
                ..
            } = &mut *st;
            editor_cam.update(camera, input, dt);
            editor.update(&renderer.gl, camera, editor_cam, input, dt, time_ms);
            renderer.begin_frame(cw, ch);
            editor.render(renderer, camera);

            tile_fetches = std::mem::take(&mut editor.pending_tile_fetches);
            inside_fetch = editor.pending_inside_fetch.take();
            asset_fetches = std::mem::take(&mut editor.pending_asset_fetches);
            saves = std::mem::take(&mut editor.pending_saves);
        }

        #[cfg(not(feature = "editor"))]
        {
            let EngineInner {
                renderer,
                camera,
                input,
                gameplay_cam,
                ..
            } = &mut *st;
            gameplay_cam.update(camera, input, dt);
            renderer.begin_frame(cw, ch);
        }

        st.input.end_frame();
    }

    #[cfg(feature = "editor")]
    {
        for (area_id, tx, ty) in tile_fetches {
            let weak = Rc::clone(inner);
            spawn_local(async move {
                let url = format!("/world/areas/{area_id}/{tx},{ty}/vis.bin");
                match http_get_bytes(&url).await {
                    Ok((200, bytes)) => {
                        let decoded = postcard::from_bytes::<TileVisData>(&bytes).ok();
                        let mut st = weak.borrow_mut();
                        let EngineInner {
                            renderer, editor, ..
                        } = &mut *st;
                        editor.on_outside_tile_loaded(&renderer.gl, area_id, tx, ty, decoded);
                    }
                    Ok((404, _)) => {
                        let mut st = weak.borrow_mut();
                        let EngineInner {
                            renderer, editor, ..
                        } = &mut *st;
                        editor.on_outside_tile_loaded(&renderer.gl, area_id, tx, ty, None);
                    }
                    Ok((code, _)) => {
                        console::warn_1(&format!("Failed loading {url}: HTTP {code}").into());
                    }
                    Err(e) => {
                        console::warn_1(&format!("Error loading {url}: {e}").into());
                    }
                }
            });
        }

        if let Some(area_id) = inside_fetch {
            let weak = Rc::clone(inner);
            spawn_local(async move {
                let url = format!("/world/areas/{area_id}/vis.bin");
                if let Ok((200, bytes)) = http_get_bytes(&url).await {
                    if let Ok(vis) = postcard::from_bytes::<InsideVisData>(&bytes) {
                        let mut st = weak.borrow_mut();
                        let EngineInner {
                            renderer, editor, ..
                        } = &mut *st;
                        editor.on_inside_loaded(&renderer.gl, area_id, vis);
                    }
                }
            });
        }

        for entry in asset_fetches {
            let weak = Rc::clone(inner);
            spawn_local(async move {
                if let Ok((200, bytes)) = http_get_bytes(&entry.path).await {
                    let mut st = weak.borrow_mut();
                    let EngineInner {
                        renderer, editor, ..
                    } = &mut *st;
                    editor.on_glb_loaded(&renderer.gl, entry.id, &bytes);
                }
            });
        }

        dispatch_pending_saves(inner, saves);
    }
}

#[cfg(feature = "editor")]
fn dispatch_pending_saves(inner: &Rc<RefCell<EngineInner>>, saves: Vec<PendingSave>) {
    for save in saves {
        let weak = Rc::clone(inner);
        spawn_local(async move {
            let (url, bytes) = match save {
                PendingSave::Outside {
                    area_id,
                    tx,
                    ty,
                    bytes,
                } => (format!("/api/editor/areas/{area_id}/tiles/{tx},{ty}"), bytes),
                PendingSave::Inside { area_id, bytes } => {
                    (format!("/api/editor/areas/{area_id}/inside"), bytes)
                }
            };
            let ok = matches!(http_put_bytes(&url, &bytes).await, Ok(200..=299));
            weak.borrow_mut().editor.on_save_finished(ok);
        });
    }
}

#[cfg(feature = "editor")]
fn fetch_asset_catalog(inner: &Rc<RefCell<EngineInner>>) {
    let weak = Rc::clone(inner);
    spawn_local(async move {
        if let Ok((200, bytes)) = http_get_bytes("/world/assets/catalog.json").await {
            if let Ok(catalog) = serde_json::from_slice::<Vec<AssetEntry>>(&bytes) {
                weak.borrow_mut().editor.set_catalog(catalog);
            }
        }
    });
}

#[cfg(feature = "editor")]
async fn http_get_bytes(url: &str) -> Result<(u16, Vec<u8>), String> {
    let window = web_sys::window().ok_or_else(|| "no window".to_owned())?;
    let opts = RequestInit::new();
    opts.set_method("GET");
    opts.set_mode(RequestMode::SameOrigin);
    let req = Request::new_with_str_and_init(url, &opts).map_err(|e| format!("{e:?}"))?;
    let resp_val = JsFuture::from(window.fetch_with_request(&req))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let resp: Response = resp_val.dyn_into().map_err(|e| format!("{e:?}"))?;
    let status = resp.status();
    if status != 200 {
        return Ok((status, Vec::new()));
    }
    let buf_val = JsFuture::from(resp.array_buffer().map_err(|e| format!("{e:?}"))?)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let arr = js_sys::Uint8Array::new(&buf_val);
    Ok((status, arr.to_vec()))
}

#[cfg(feature = "editor")]
async fn http_put_bytes(url: &str, bytes: &[u8]) -> Result<u16, String> {
    let window = web_sys::window().ok_or_else(|| "no window".to_owned())?;
    let opts = RequestInit::new();
    opts.set_method("PUT");
    opts.set_mode(RequestMode::SameOrigin);
    let uint8 = js_sys::Uint8Array::from(bytes);
    opts.set_body(&uint8.into());
    let req = Request::new_with_str_and_init(url, &opts).map_err(|e| format!("{e:?}"))?;
    let resp_val = JsFuture::from(window.fetch_with_request(&req))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let resp: Response = resp_val.dyn_into().map_err(|e| format!("{e:?}"))?;
    Ok(resp.status())
}

fn is_typing_in_form(target: Option<web_sys::EventTarget>) -> bool {
    let Some(t) = target else {
        return false;
    };
    let Ok(el) = t.dyn_into::<Element>() else {
        return false;
    };
    let tag = el.tag_name();
    matches!(tag.as_str(), "INPUT" | "TEXTAREA" | "SELECT")
}

fn attach_input_listeners(
    window: &web_sys::Window,
    canvas: &HtmlCanvasElement,
    inner: &Rc<RefCell<EngineInner>>,
) -> Result<Vec<Box<dyn std::any::Any>>, JsValue> {
    let mut kept: Vec<Box<dyn std::any::Any>> = Vec::new();

    // Prevent browser auto-scroll on middle-click and context menu on right-click
    let on_context = Closure::<dyn FnMut(MouseEvent)>::new(|ev: MouseEvent| {
        ev.prevent_default();
    });
    canvas.add_event_listener_with_callback("contextmenu", on_context.as_ref().unchecked_ref())?;
    kept.push(Box::new(on_context));

    // Mouse down on canvas
    let st_down = Rc::clone(inner);
    let c_down = canvas.clone();
    let on_mousedown = Closure::<dyn FnMut(MouseEvent)>::new(move |ev: MouseEvent| {
        if ev.button() == 1 {
            ev.prevent_default();
        }
        let rect = c_down.get_bounding_client_rect();
        let mut st = st_down.borrow_mut();
        st.input.mouse_x = (ev.client_x() as f64 - rect.left()) as f32;
        st.input.mouse_y = (ev.client_y() as f64 - rect.top()) as f32;
        match ev.button() {
            0 => {
                st.input.left_mouse_down = true;
                st.input.left_just_pressed = true;
            }
            1 => st.input.middle_mouse_down = true,
            _ => {}
        }
    });
    canvas.add_event_listener_with_callback("mousedown", on_mousedown.as_ref().unchecked_ref())?;
    kept.push(Box::new(on_mousedown));

    // Mouse up on window so releasing outside canvas doesn't stick
    let st_up = Rc::clone(inner);
    let on_mouseup = Closure::<dyn FnMut(MouseEvent)>::new(move |ev: MouseEvent| {
        let mut st = st_up.borrow_mut();
        match ev.button() {
            0 => {
                if st.input.left_mouse_down {
                    st.input.left_just_released = true;
                }
                st.input.left_mouse_down = false;
            }
            1 => st.input.middle_mouse_down = false,
            _ => {}
        }
    });
    window.add_event_listener_with_callback("mouseup", on_mouseup.as_ref().unchecked_ref())?;
    kept.push(Box::new(on_mouseup));

    // Mouse move on window
    let st_move = Rc::clone(inner);
    let c_move = canvas.clone();
    let on_mousemove = Closure::<dyn FnMut(MouseEvent)>::new(move |ev: MouseEvent| {
        let rect = c_move.get_bounding_client_rect();
        let nx = (ev.client_x() as f64 - rect.left()) as f32;
        let ny = (ev.client_y() as f64 - rect.top()) as f32;
        let mut st = st_move.borrow_mut();
        let dx = nx - st.input.mouse_x;
        let dy = ny - st.input.mouse_y;
        st.input.mouse_x = nx;
        st.input.mouse_y = ny;
        st.input.mouse_delta.0 += dx;
        st.input.mouse_delta.1 += dy;
    });
    window.add_event_listener_with_callback("mousemove", on_mousemove.as_ref().unchecked_ref())?;
    kept.push(Box::new(on_mousemove));

    // Wheel on canvas
    let st_wheel = Rc::clone(inner);
    let on_wheel = Closure::<dyn FnMut(WheelEvent)>::new(move |ev: WheelEvent| {
        ev.prevent_default();
        let delta = (ev.delta_y() as f32 / 100.0).clamp(-3.0, 3.0);
        st_wheel.borrow_mut().input.scroll_delta += delta;
    });
    canvas.add_event_listener_with_callback("wheel", on_wheel.as_ref().unchecked_ref())?;
    kept.push(Box::new(on_wheel));

    // Key down on window
    let st_kd = Rc::clone(inner);
    let on_keydown = Closure::<dyn FnMut(KeyboardEvent)>::new(move |ev: KeyboardEvent| {
        if is_typing_in_form(ev.target()) {
            return;
        }
        let code = ev.code();
        if matches!(
            code.as_str(),
            "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight"
        ) {
            ev.prevent_default();
        }
        st_kd.borrow_mut().input.keys.insert(code);
    });
    window.add_event_listener_with_callback("keydown", on_keydown.as_ref().unchecked_ref())?;
    kept.push(Box::new(on_keydown));

    // Key up on window
    let st_ku = Rc::clone(inner);
    let on_keyup = Closure::<dyn FnMut(KeyboardEvent)>::new(move |ev: KeyboardEvent| {
        st_ku.borrow_mut().input.keys.remove(&ev.code());
    });
    window.add_event_listener_with_callback("keyup", on_keyup.as_ref().unchecked_ref())?;
    kept.push(Box::new(on_keyup));

    // Blur clears stuck keys
    let st_blur = Rc::clone(inner);
    let on_blur = Closure::<dyn FnMut()>::new(move || {
        let mut st = st_blur.borrow_mut();
        st.input.keys.clear();
        st.input.left_mouse_down = false;
        st.input.middle_mouse_down = false;
    });
    window.add_event_listener_with_callback("blur", on_blur.as_ref().unchecked_ref())?;
    kept.push(Box::new(on_blur));

    Ok(kept)
}
