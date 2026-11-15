use std::fs;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use shared::protocol::ServerMessage;
use shared::world::{
    InsideSimData, InsideVisData, TILE_SIZE, TILE_VERTICES, TileSimData, TileVisData,
    compute_inside_sim_from_vis, compute_sim_from_vis,
};

use crate::net::{AppState, is_valid_area_id, parse_tile_coord};

pub async fn save_outside_tile(
    State(state): State<AppState>,
    Path((area_id, tile_coord)): Path<(String, String)>,
    body: Bytes,
) -> Result<StatusCode, (StatusCode, String)> {
    if !is_valid_area_id(&area_id) {
        return Err((StatusCode::BAD_REQUEST, "invalid area id".to_owned()));
    }
    let (tx, ty) =
        parse_tile_coord(&tile_coord).ok_or((StatusCode::BAD_REQUEST, "invalid tile coord".to_owned()))?;

    let mut vis: TileVisData = postcard::from_bytes(&body)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid TileVisData: {e}")))?;

    if vis.heights.len() != TILE_VERTICES * TILE_VERTICES || vis.splat_map.len() != TILE_SIZE * TILE_SIZE {
        return Err((
            StatusCode::BAD_REQUEST,
            "unexpected TileVisData height/splat dimensions".to_owned(),
        ));
    }

    vis.recompute_slope_walkability();

    let tile_dir = state
        .world_dir
        .join("areas")
        .join(&area_id)
        .join(format!("{tx},{ty}"));
    fs::create_dir_all(&tile_dir).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let existing_sim = fs::read(tile_dir.join("sim.bin"))
        .ok()
        .and_then(|bytes| postcard::from_bytes::<TileSimData>(&bytes).ok());
    let sim = compute_sim_from_vis(&vis, existing_sim.as_ref());

    fs::write(
        tile_dir.join("vis.bin"),
        postcard::to_allocvec(&vis).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    fs::write(
        tile_dir.join("sim.bin"),
        postcard::to_allocvec(&sim).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    state
        .sim
        .write()
        .await
        .hot_reload_outside_tile(&area_id, tx, ty, sim);

    let _ = state.tx_broadcast.send(ServerMessage::TileUpdated {
        area_id,
        tile_x: tx,
        tile_y: ty,
    });

    Ok(StatusCode::NO_CONTENT)
}

pub async fn save_inside_area(
    State(state): State<AppState>,
    Path(area_id): Path<String>,
    body: Bytes,
) -> Result<StatusCode, (StatusCode, String)> {
    if !is_valid_area_id(&area_id) {
        return Err((StatusCode::BAD_REQUEST, "invalid area id".to_owned()));
    }

    let mut vis: InsideVisData = postcard::from_bytes(&body)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid InsideVisData: {e}")))?;

    vis.recompute_slope_walkability();

    let area_dir = state.world_dir.join("areas").join(&area_id);
    if !area_dir.exists() {
        return Err((StatusCode::NOT_FOUND, "area does not exist".to_owned()));
    }

    let existing_sim = fs::read(area_dir.join("sim.bin"))
        .ok()
        .and_then(|bytes| postcard::from_bytes::<InsideSimData>(&bytes).ok());
    let sim = compute_inside_sim_from_vis(&vis, existing_sim.as_ref());

    fs::write(
        area_dir.join("vis.bin"),
        postcard::to_allocvec(&vis).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    fs::write(
        area_dir.join("sim.bin"),
        postcard::to_allocvec(&sim).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    state.sim.write().await.hot_reload_inside(&area_id, sim);
    let _ = state.tx_broadcast.send(ServerMessage::AreaUpdated { area_id });

    Ok(StatusCode::NO_CONTENT)
}
