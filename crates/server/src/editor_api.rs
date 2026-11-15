use std::fs;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use shared::protocol::ServerMessage;
use shared::world::{
    AreaKind, AreaMetadata, InsideKind, InsideSimData, InsideVisData, TILE_SIZE, TILE_VERTICES,
    TileSimData, TileVisData, compute_inside_sim_from_vis, compute_sim_from_vis,
};
use tracing::info;

use crate::net::{AppState, is_valid_area_id, parse_tile_coord};

pub async fn list_areas(State(state): State<AppState>) -> Result<Json<Vec<AreaMetadata>>, StatusCode> {
    let areas_dir = state.world_dir.join("areas");
    let entries = fs::read_dir(&areas_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut areas = Vec::new();
    for entry in entries.flatten() {
        let bin_path = entry.path().join("area.bin");
        if let Ok(bytes) = fs::read(&bin_path) {
            if let Ok(meta) = postcard::from_bytes::<AreaMetadata>(&bytes) {
                areas.push(meta);
            }
        }
    }

    areas.sort_by(|a, b| match (a.id.as_str(), b.id.as_str()) {
        ("main", _) => std::cmp::Ordering::Less,
        (_, "main") => std::cmp::Ordering::Greater,
        _ => a.id.cmp(&b.id),
    });

    Ok(Json(areas))
}

#[derive(Deserialize)]
pub struct CreateAreaRequest {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub width: Option<u32>,
    pub length: Option<u32>,
}

pub async fn create_area(
    State(state): State<AppState>,
    Json(req): Json<CreateAreaRequest>,
) -> Result<(StatusCode, Json<AreaMetadata>), (StatusCode, String)> {
    let id = req.id.trim().to_lowercase();
    if !is_valid_area_id(&id) {
        return Err((
            StatusCode::BAD_REQUEST,
            "area id must be 1-64 ASCII alphanumeric, '_' or '-' chars".to_owned(),
        ));
    }

    let name = req.name.trim();
    if name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "area name is required".to_owned()));
    }

    let area_dir = state.world_dir.join("areas").join(&id);
    if area_dir.exists() {
        return Err((StatusCode::CONFLICT, format!("area '{id}' already exists")));
    }

    let (kind, dimensions) = match req.kind.as_str() {
        "outside" => (AreaKind::Outside, None),
        "inside_ephemeral" => {
            let w = req.width.unwrap_or(24).clamp(8, 256);
            let l = req.length.unwrap_or(24).clamp(8, 256);
            (AreaKind::Inside(InsideKind::Ephemeral), Some((w, l)))
        }
        "inside_permanent" => {
            let w = req.width.unwrap_or(24).clamp(8, 256);
            let l = req.length.unwrap_or(24).clamp(8, 256);
            (AreaKind::Inside(InsideKind::Permanent), Some((w, l)))
        }
        "inside_generated" => (AreaKind::Inside(InsideKind::Generated), None),
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("unsupported area kind '{other}'"),
            ));
        }
    };

    let meta = AreaMetadata {
        id: id.clone(),
        name: name.to_owned(),
        kind,
        dimensions,
    };

    fs::create_dir_all(&area_dir).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    fs::write(
        area_dir.join("area.bin"),
        postcard::to_allocvec(&meta).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match meta.kind {
        AreaKind::Outside => {
            let tile_dir = area_dir.join("0,0");
            fs::create_dir_all(&tile_dir)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            let vis = TileVisData::flat_default();
            let sim = TileSimData::flat_default();
            fs::write(
                tile_dir.join("vis.bin"),
                postcard::to_allocvec(&vis)
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
            )
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            fs::write(
                tile_dir.join("sim.bin"),
                postcard::to_allocvec(&sim)
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
            )
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        }
        AreaKind::Inside(_) => {
            let (w, l) = meta.dimensions.unwrap_or((32, 32));
            let vis = InsideVisData::flat_default(w, l);
            let sim = InsideSimData::flat_default(w, l);
            fs::write(
                area_dir.join("vis.bin"),
                postcard::to_allocvec(&vis)
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
            )
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            fs::write(
                area_dir.join("sim.bin"),
                postcard::to_allocvec(&sim)
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
            )
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        }
    }

    info!("editor created new area '{}' ({:?})", meta.id, meta.kind);
    Ok((StatusCode::CREATED, Json(meta)))
}

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
