use std::cell::RefCell;
use std::rc::Rc;

use shared::entity::PlacedEntity;
use shared::world::{AssetEntry, InsideVisData, TileVisData};
use wasm_bindgen_futures::spawn_local;
use web_sys::console;

use crate::engine::EngineInner;
use crate::net::{http_get_bytes, http_put_bytes, http_put_json_bytes};

use super::types::PendingSave;

pub(crate) struct FrameIoBatch {
    pub tile_fetches: Vec<(String, i32, i32)>,
    pub inside_fetch: Option<String>,
    pub asset_fetches: Vec<AssetEntry>,
    pub character_glb_fetches: Vec<String>,
    pub saves: Vec<PendingSave>,
}

pub(crate) fn spawn_frame_io(inner: &Rc<RefCell<EngineInner>>, batch: FrameIoBatch) {
    for (area_id, tx, ty) in batch.tile_fetches {
        let weak = Rc::clone(inner);
        spawn_local(async move {
            let url = format!("/world/areas/{area_id}/{tx},{ty}/vis.bin");
            match http_get_bytes(&url).await {
                Ok((200, bytes)) => {
                    let decoded = TileVisData::from_postcard_bytes(&bytes).ok();
                    {
                        let mut st = weak.borrow_mut();
                        let EngineInner {
                            renderer, editor, ..
                        } = &mut *st;
                        editor.on_outside_tile_loaded(&renderer.gl, area_id.clone(), tx, ty, decoded);
                    }
                    let ent_url = format!("/api/editor/areas/{area_id}/tiles/{tx},{ty}/entities");
                    if let Ok((200, ent_bytes)) = http_get_bytes(&ent_url).await {
                        if let Ok(entities) = serde_json::from_slice::<Vec<PlacedEntity>>(&ent_bytes)
                        {
                            weak.borrow_mut()
                                .editor
                                .on_outside_tile_entities_loaded(&area_id, tx, ty, entities);
                        }
                    }
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
                if let Ok(vis) = InsideVisData::from_postcard_bytes(&bytes) {
                    {
                        let mut st = weak.borrow_mut();
                        let EngineInner {
                            renderer, editor, ..
                        } = &mut *st;
                        editor.on_inside_loaded(&renderer.gl, area_id.clone(), vis);
                    }
                    let ent_url = format!("/api/editor/areas/{area_id}/inside/entities");
                    if let Ok((200, ent_bytes)) = http_get_bytes(&ent_url).await {
                        if let Ok(entities) = serde_json::from_slice::<Vec<PlacedEntity>>(&ent_bytes)
                        {
                            weak.borrow_mut()
                                .editor
                                .on_inside_entities_loaded(&area_id, entities);
                        }
                    }
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
                if let Err(e) = editor.on_glb_loaded(&renderer.gl, entry.id, &bytes) {
                    console::error_1(&e.into());
                }
            }
        });
    }

    for url in batch.character_glb_fetches {
        let weak = Rc::clone(inner);
        spawn_local(async move {
            match http_get_bytes(&url).await {
                Ok((200, bytes)) => {
                    let mut st = weak.borrow_mut();
                    let EngineInner {
                        renderer, editor, ..
                    } = &mut *st;
                    if let Err(e) = editor
                        .character_studio
                        .on_glb_loaded(&renderer.gl, url.clone(), &bytes)
                    {
                        console::error_1(&format!("Character GLB {url}: {e}").into());
                    }
                }
                Ok((status, _)) => {
                    console::warn_1(&format!("Failed loading character GLB {url}: HTTP {status}").into());
                }
                Err(e) => {
                    console::warn_1(&format!("Error loading character GLB {url}: {e}").into());
                }
            }
        });
    }

    dispatch_pending_saves(inner, batch.saves);
}

pub(crate) fn dispatch_pending_saves(inner: &Rc<RefCell<EngineInner>>, saves: Vec<PendingSave>) {
    for save in saves {
        let weak = Rc::clone(inner);
        spawn_local(async move {
            let status = match save {
                PendingSave::Outside {
                    area_id,
                    tx,
                    ty,
                    bytes,
                } => http_put_bytes(
                    &format!("/api/editor/areas/{area_id}/tiles/{tx},{ty}"),
                    &bytes,
                )
                .await,
                PendingSave::Inside { area_id, bytes } => {
                    http_put_bytes(&format!("/api/editor/areas/{area_id}/inside"), &bytes).await
                }
                PendingSave::OutsideEntities {
                    area_id,
                    tx,
                    ty,
                    bytes,
                } => http_put_json_bytes(
                    &format!("/api/editor/areas/{area_id}/tiles/{tx},{ty}/entities"),
                    &bytes,
                )
                .await,
                PendingSave::InsideEntities { area_id, bytes } => http_put_json_bytes(
                    &format!("/api/editor/areas/{area_id}/inside/entities"),
                    &bytes,
                )
                .await,
            };
            let ok = matches!(status, Ok(200..=299));
            weak.borrow_mut().editor.on_save_finished(ok);
        });
    }
}
