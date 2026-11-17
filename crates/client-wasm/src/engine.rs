use std::cell::RefCell;
use std::rc::Rc;

use web_sys::HtmlCanvasElement;

#[cfg(feature = "editor")]
use crate::camera::EditorCameraController;
use crate::camera::{Camera, GameplayCameraController};
#[cfg(feature = "editor")]
use crate::editor::{EditorMode, EditorState, io::{FrameIoBatch, spawn_frame_io}};
use crate::game::{GameClientState, GameIoBatch, GamePhase, spawn_game_io};
use crate::input::InputState;
use crate::net::NetClient;
use crate::render::Renderer;

#[cfg(feature = "editor")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClientRunMode {
    Game,
    Editor,
}

enum PendingIo {
    #[cfg(feature = "editor")]
    Editor(FrameIoBatch),
    Game(GameIoBatch),
}

pub(crate) struct EngineInner {
    pub(crate) canvas: HtmlCanvasElement,
    pub(crate) renderer: Renderer,
    pub(crate) camera: Camera,
    pub(crate) input: InputState,
    pub(crate) ws_url: String,
    pub(crate) gameplay_cam: GameplayCameraController,
    pub(crate) game: GameClientState,
    #[cfg(feature = "editor")]
    pub(crate) active_run_mode: ClientRunMode,
    #[cfg(feature = "editor")]
    pub(crate) editor_cam: EditorCameraController,
    #[cfg(feature = "editor")]
    pub(crate) editor: EditorState,
    pub(crate) net: Option<NetClient>,
    pub(crate) last_time_ms: f64,
}

pub(crate) fn step_frame(inner: &Rc<RefCell<EngineInner>>, time_ms: f64) {
    let pending_io: PendingIo;

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
        let is_editor = st.active_run_mode == ClientRunMode::Editor;
        #[cfg(not(feature = "editor"))]
        let is_editor = false;

        if is_editor {
            #[cfg(feature = "editor")]
            {
                if let Some(net) = &st.net {
                    let mut ignored = Vec::new();
                    net.drain_messages(&mut ignored);
                }
                let EngineInner {
                    renderer,
                    camera,
                    input,
                    editor_cam,
                    editor,
                    ..
                } = &mut *st;
                if editor.mode == EditorMode::World {
                    use crate::camera::CameraController;
                    camera.znear = 0.25;
                    camera.zfar = 600.0;
                    renderer.fog_color = [0.56, 0.72, 0.86];
                    renderer.fog_start = 140.0;
                    renderer.fog_end = 240.0;
                    editor_cam.update(camera, input, dt);
                } else {
                    camera.znear = 0.08;
                    camera.zfar = 120.0;
                    renderer.fog_color = [0.11, 0.13, 0.18];
                    renderer.fog_start = 45.0;
                    renderer.fog_end = 110.0;
                    let turntable =
                        editor.mode == EditorMode::Characters && editor.character_studio.turntable;
                    editor.studio_cam.update(camera, input, dt, turntable);
                }
                editor.update(&renderer.gl, camera, editor_cam, input, dt, time_ms);
                editor.render_shadows(renderer, camera);
                renderer.begin_frame(cw, ch);
                editor.render(renderer, camera);

                pending_io = PendingIo::Editor(FrameIoBatch {
                    tile_fetches: std::mem::take(&mut editor.pending_tile_fetches),
                    inside_fetch: editor.pending_inside_fetch.take(),
                    asset_fetches: std::mem::take(&mut editor.pending_asset_fetches),
                    character_glb_fetches: std::mem::take(
                        &mut editor.character_studio.pending_glb_fetches,
                    ),
                    saves: std::mem::take(&mut editor.pending_saves),
                });
            }
            #[cfg(not(feature = "editor"))]
            unreachable!();
        } else {
            let mut msgs = Vec::new();
            let socket_closed = if let Some(net) = &st.net {
                net.drain_messages(&mut msgs);
                net.is_closed()
            } else {
                false
            };

            let EngineInner {
                canvas,
                renderer,
                camera,
                input,
                gameplay_cam,
                game,
                net,
                ..
            } = &mut *st;

            game.handle_server_messages(&renderer.gl, gameplay_cam, msgs);
            if socket_closed
                && matches!(
                    game.phase,
                    GamePhase::Connecting | GamePhase::LoadingArea | GamePhase::Playing
                )
            {
                game.phase = GamePhase::Error("Connection to server closed.".to_owned());
            }

            game.update(canvas, renderer, camera, gameplay_cam, input, net.as_ref(), dt);
            game.render_shadows(renderer, camera);
            renderer.begin_frame(cw, ch);
            game.render(renderer, camera);

            pending_io = PendingIo::Game(game.take_io_batch());
        }

        st.input.end_frame();
    }

    match pending_io {
        #[cfg(feature = "editor")]
        PendingIo::Editor(batch) => spawn_frame_io(inner, batch),
        PendingIo::Game(batch) => spawn_game_io(inner, batch),
    }
}
