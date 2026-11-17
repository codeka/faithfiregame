use std::fs;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;
use shared::world::{
    AreaKind, AreaMetadata, InsideKind, InsideSimData, InsideVisData, TileSimData, TileVisData,
};
use tracing::info;

use crate::net::{AppState, is_valid_area_id};

pub async fn list_areas(State(state): State<AppState>) -> Result<Json<Vec<AreaMetadata>>, StatusCode> {
    let areas_dir = state.world_dir.join("areas");
    if !areas_dir.exists() {
        return Ok(Json(Vec::new()));
    }
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
            let vis = TileVisData::seeded_grass(&meta.id, 0, 0);
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
            let vis = InsideVisData::seeded_grass(&meta.id, w, l);
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use tokio::sync::{RwLock, broadcast};

    use super::*;
    use crate::db::SqliteStorage;
    use crate::sim::SimWorld;

    fn unique_temp_dir(label: &str) -> PathBuf {
        let unique = format!(
            "ff_{label}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(unique)
    }

    #[tokio::test]
    async fn empty_world_lists_no_areas_and_create_area_initializes_outside_tile() {
        let root = unique_temp_dir("empty_areas");
        let world_dir = root.join("world");
        fs::create_dir_all(&root).unwrap();

        let storage = Arc::new(SqliteStorage::open(&root.join("test.db")).await.unwrap());
        let sim = Arc::new(RwLock::new(SimWorld::new(world_dir.clone())));
        let (tx_broadcast, _) = broadcast::channel(8);
        let state = AppState {
            world_dir: world_dir.clone(),
            storage,
            sim,
            tx_broadcast,
        };

        let Json(initial) = list_areas(State(state.clone())).await.unwrap();
        assert!(initial.is_empty());

        let req = CreateAreaRequest {
            id: "frontier".to_owned(),
            name: "Frontier".to_owned(),
            kind: "outside".to_owned(),
            width: None,
            length: None,
        };
        let (status, Json(created)) = create_area(State(state.clone()), Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(created.id, "frontier");
        assert!(world_dir.join("areas/frontier/area.bin").exists());
        assert!(world_dir.join("areas/frontier/0,0/vis.bin").exists());
        assert!(world_dir.join("areas/frontier/0,0/sim.bin").exists());

        let Json(after) = list_areas(State(state)).await.unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].id, "frontier");

        let _ = fs::remove_dir_all(&root);
    }
}
