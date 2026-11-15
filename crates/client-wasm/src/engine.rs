use std::cell::RefCell;
use std::rc::Rc;

use web_sys::HtmlCanvasElement;

#[cfg(feature = "editor")]
use crate::camera::EditorCameraController;
#[cfg(not(feature = "editor"))]
use crate::camera::GameplayCameraController;
use crate::camera::{Camera, CameraController};
#[cfg(feature = "editor")]
use crate::editor::{EditorState, io::{FrameIoBatch, spawn_frame_io}};
use crate::input::InputState;
use crate::net::NetClient;
use crate::render::Renderer;

pub(crate) struct EngineInner {
    pub(crate) canvas: HtmlCanvasElement,
    pub(crate) renderer: Renderer,
    pub(crate) camera: Camera,
    pub(crate) input: InputState,
    #[cfg(not(feature = "editor"))]
    pub(crate) gameplay_cam: GameplayCameraController,
    #[cfg(feature = "editor")]
    pub(crate) editor_cam: EditorCameraController,
    #[cfg(feature = "editor")]
    pub(crate) editor: EditorState,
    pub(crate) _net: Option<NetClient>,
    pub(crate) last_time_ms: f64,
}

pub(crate) fn step_frame(inner: &Rc<RefCell<EngineInner>>, time_ms: f64) {
    #[cfg(feature = "editor")]
    let io_batch: FrameIoBatch;

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
            editor.render_shadows(renderer, camera);
            renderer.begin_frame(cw, ch);
            editor.render(renderer, camera);

            io_batch = FrameIoBatch {
                tile_fetches: std::mem::take(&mut editor.pending_tile_fetches),
                inside_fetch: editor.pending_inside_fetch.take(),
                asset_fetches: std::mem::take(&mut editor.pending_asset_fetches),
                saves: std::mem::take(&mut editor.pending_saves),
            };
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
            renderer.begin_shadow_pass(camera);
            renderer.begin_frame(cw, ch);
        }

        st.input.end_frame();
    }

    #[cfg(feature = "editor")]
    spawn_frame_io(inner, io_batch);
}
