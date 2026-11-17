use std::cell::RefCell;
use std::rc::Rc;

use shared::character::CharacterCatalog;
use shared::world::{AssetEntry, InsideVisData, TileVisData};
use wasm_bindgen_futures::spawn_local;
use web_sys::console;

use crate::engine::EngineInner;
use crate::net::http_get_bytes;

pub struct GameIoBatch {
    pub tile_fetches: Vec<(String, i32, i32)>,
    pub inside_fetch: Option<String>,
    pub asset_fetches: Vec<AssetEntry>,
    pub char_catalog_fetch: bool,
    pub char_glb_fetches: Vec<String>,
}

pub fn spawn_game_io(inner: &Rc<RefCell<EngineInner>>, batch: GameIoBatch) {
    for (area_id, tx, ty) in batch.tile_fetches {
        let weak = Rc::clone(inner);
        spawn_local(async move {
            let url = format!("/world/areas/{area_id}/{tx},{ty}/vis.bin");
            match http_get_bytes(&url).await {
                Ok((200, bytes)) => {
                    let decoded = TileVisData::from_postcard_bytes(&bytes).ok();
                    let mut st = weak.borrow_mut();
                    let EngineInner { renderer, game, .. } = &mut *st;
                    game.on_outside_tile_loaded(&renderer.gl, area_id, tx, ty, decoded);
                }
                Ok((404, _)) => {
                    let mut st = weak.borrow_mut();
                    let EngineInner { renderer, game, .. } = &mut *st;
                    game.on_outside_tile_loaded(&renderer.gl, area_id, tx, ty, None);
                }
                Ok((code, _)) => {
                    console::warn_1(&format!("Game tile {url}: HTTP {code}").into());
                }
                Err(e) => {
                    console::warn_1(&format!("Game tile {url}: {e}").into());
                }
            }
        });
    }

    if let Some(area_id) = batch.inside_fetch {
        let weak = Rc::clone(inner);
        spawn_local(async move {
            let url = format!("/world/areas/{area_id}/vis.bin");
            match http_get_bytes(&url).await {
                Ok((200, bytes)) => {
                    if let Ok(vis) = InsideVisData::from_postcard_bytes(&bytes) {
                        let mut st = weak.borrow_mut();
                        let EngineInner { renderer, game, .. } = &mut *st;
                        game.on_inside_loaded(&renderer.gl, area_id, vis);
                    }
                }
                Ok((code, _)) => {
                    console::warn_1(&format!("Game inside {url}: HTTP {code}").into());
                }
                Err(e) => {
                    console::warn_1(&format!("Game inside {url}: {e}").into());
                }
            }
        });
    }

    for entry in batch.asset_fetches {
        let weak = Rc::clone(inner);
        spawn_local(async move {
            match http_get_bytes(&entry.path).await {
                Ok((200, bytes)) => {
                    let mut st = weak.borrow_mut();
                    let EngineInner { renderer, game, .. } = &mut *st;
                    if let Err(e) = game.on_prop_glb_loaded(&renderer.gl, entry.id, &bytes) {
                        console::warn_1(&format!("Game prop {}: {e}", entry.path).into());
                    }
                }
                Ok((code, _)) => {
                    console::warn_1(&format!("Game prop {}: HTTP {code}", entry.path).into());
                }
                Err(e) => {
                    console::warn_1(&format!("Game prop {}: {e}", entry.path).into());
                }
            }
        });
    }

    if batch.char_catalog_fetch {
        let weak = Rc::clone(inner);
        spawn_local(async move {
            let url = "/world/characters/catalog.json";
            match http_get_bytes(url).await {
                Ok((200, bytes)) => {
                    if let Ok(catalog) = serde_json::from_slice::<CharacterCatalog>(&bytes) {
                        weak.borrow_mut().game.on_character_catalog_loaded(catalog);
                    }
                }
                Ok((code, _)) => {
                    console::warn_1(&format!("Character catalog {url}: HTTP {code}").into());
                }
                Err(e) => {
                    console::warn_1(&format!("Character catalog {url}: {e}").into());
                }
            }
        });
    }

    for url in batch.char_glb_fetches {
        let weak = Rc::clone(inner);
        spawn_local(async move {
            match http_get_bytes(&url).await {
                Ok((200, bytes)) => {
                    let mut st = weak.borrow_mut();
                    let EngineInner { renderer, game, .. } = &mut *st;
                    if let Err(e) = game.on_character_glb_loaded(&renderer.gl, url.clone(), &bytes) {
                        console::warn_1(&format!("Game character GLB {url}: {e}").into());
                    }
                }
                Ok((code, _)) => {
                    console::warn_1(&format!("Game character GLB {url}: HTTP {code}").into());
                }
                Err(e) => {
                    console::warn_1(&format!("Game character GLB {url}: {e}").into());
                }
            }
        });
    }
}
