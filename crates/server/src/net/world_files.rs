use std::fs;

use axum::extract::{Path, State};
use axum::http::StatusCode;

use super::state::{AppState, is_valid_area_id, parse_tile_coord};

pub async fn get_area_bin(
    State(state): State<AppState>,
    Path(area_id): Path<String>,
) -> Result<Vec<u8>, StatusCode> {
    if !is_valid_area_id(&area_id) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let path = state.world_dir.join("areas").join(&area_id).join("area.bin");
    fs::read(path).map_err(|_| StatusCode::NOT_FOUND)
}

pub async fn get_outside_vis_bin(
    State(state): State<AppState>,
    Path((area_id, tile_coord)): Path<(String, String)>,
) -> Result<Vec<u8>, StatusCode> {
    if !is_valid_area_id(&area_id) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let (tx, ty) = parse_tile_coord(&tile_coord).ok_or(StatusCode::BAD_REQUEST)?;
    let path = state
        .world_dir
        .join("areas")
        .join(&area_id)
        .join(format!("{tx},{ty}"))
        .join("vis.bin");
    fs::read(path).map_err(|_| StatusCode::NOT_FOUND)
}

pub async fn get_inside_vis_bin(
    State(state): State<AppState>,
    Path(area_id): Path<String>,
) -> Result<Vec<u8>, StatusCode> {
    if !is_valid_area_id(&area_id) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let path = state.world_dir.join("areas").join(&area_id).join("vis.bin");
    fs::read(path).map_err(|_| StatusCode::NOT_FOUND)
}
