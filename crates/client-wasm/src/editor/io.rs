use std::cell::RefCell;
use std::rc::Rc;

use shared::world::{AssetEntry, InsideVisData, TileVisData};
use wasm_bindgen_futures::spawn_local;
use web_sys::console;

use crate::engine::EngineInner;
use crate::net::{http_get_bytes, http_put_bytes};

use super::types::PendingSave;

pub(crate) struct FrameIoBatch {
    pub tile_fetches: Vec<(String, i32, i32)>,
    pub inside_fetch: Option<String>,
    pub asset_fetches: Vec<AssetEntry>,
    pub saves: Vec<PendingSave>,
}

pub(crate) fn fetch_asset_catalog(inner: &Rc<RefCell<EngineInner>>) {
    let weak = Rc::clone(inner);
    spawn_local(async move {
        if let Ok((200, bytes)) = http_get_bytes("/world/assets/catalog.json").await {
            if let Ok(catalog) = serde_json::from_slice::<Vec<AssetEntry>>(&bytes) {
                weak.borrow_mut().editor.set_catalog(catalog);
            }
        }
    });
}

pub(crate) fn spawn_frame_io(inner: &Rc<RefCell<EngineInner>>, batch: FrameIoBatch) {
    for (area_id, tx, ty) in batch.tile_fetches {
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

    if let Some(area_id) = batch.inside_fetch {
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

    for entry in batch.asset_fetches {
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

    dispatch_pending_saves(inner, batch.saves);
}

pub(crate) fn dispatch_pending_saves(inner: &Rc<RefCell<EngineInner>>, saves: Vec<PendingSave>) {
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
