use std::fs;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use shared::world::{
    AssetEntry, format_local_asset_id, is_valid_asset_slug, local_asset_http_path,
    prettify_asset_slug, sanitize_asset_slug,
};
use tracing::info;

use crate::net::{AppState, is_valid_area_id};

pub async fn list_area_assets(
    State(state): State<AppState>,
    Path(area_id): Path<String>,
) -> Result<Json<Vec<AssetEntry>>, StatusCode> {
    if !is_valid_area_id(&area_id) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let assets_dir = state.world_dir.join("areas").join(&area_id).join("assets");
    if !assets_dir.exists() {
        return Ok(Json(Vec::new()));
    }

    let entries = fs::read_dir(&assets_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut list = Vec::new();
    for entry in entries.flatten() {
        let fname = entry.file_name();
        let Some(name_str) = fname.to_str() else {
            continue;
        };
        let Some(slug) = name_str.strip_suffix(".glb") else {
            continue;
        };
        if !is_valid_asset_slug(slug) {
            continue;
        }
        list.push(AssetEntry {
            id: format_local_asset_id(slug),
            name: prettify_asset_slug(slug),
            path: local_asset_http_path(&area_id, slug),
            category: "Area Unique".to_owned(),
        });
    }
    list.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(list))
}

pub async fn upload_area_asset(
    State(state): State<AppState>,
    Path((area_id, raw_slug)): Path<(String, String)>,
    body: Bytes,
) -> Result<Json<AssetEntry>, (StatusCode, String)> {
    if !is_valid_area_id(&area_id) {
        return Err((StatusCode::BAD_REQUEST, "invalid area id".to_owned()));
    }
    let area_dir = state.world_dir.join("areas").join(&area_id);
    if !area_dir.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("area '{area_id}' does not exist"),
        ));
    }

    let slug = sanitize_asset_slug(&raw_slug);
    if !is_valid_asset_slug(&slug) {
        return Err((StatusCode::BAD_REQUEST, "invalid asset slug".to_owned()));
    }

    if body.len() < 12 || &body[0..4] != b"glTF" {
        return Err((
            StatusCode::BAD_REQUEST,
            "uploaded file is not a valid binary .glb (missing glTF header)".to_owned(),
        ));
    }
    let declared_len = u32::from_le_bytes([body[8], body[9], body[10], body[11]]) as usize;
    if declared_len != body.len() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "corrupt .glb header length ({declared_len} bytes declared vs {} bytes uploaded)",
                body.len()
            ),
        ));
    }

    let assets_dir = area_dir.join("assets");
    fs::create_dir_all(&assets_dir)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let dest = assets_dir.join(format!("{slug}.glb"));
    fs::write(&dest, &body).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let entry = AssetEntry {
        id: format_local_asset_id(&slug),
        name: prettify_asset_slug(&slug),
        path: local_asset_http_path(&area_id, &slug),
        category: "Area Unique".to_owned(),
    };
    info!(
        "uploaded area-local asset '{}' ({} bytes) into area '{}'",
        entry.id,
        body.len(),
        area_id
    );
    Ok(Json(entry))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::routing::{get, put};

    #[test]
    fn axum_routes_for_area_assets_and_tiles_do_not_conflict() {
        let _router: Router<AppState> = Router::new()
            .route("/world/areas/{area_id}/area.bin", get(crate::net::get_area_bin))
            .route(
                "/world/areas/{area_id}/assets/{file_name}",
                get(crate::net::get_area_asset_glb),
            )
            .route(
                "/world/areas/{area_id}/{tile_coord}/vis.bin",
                get(crate::net::get_outside_vis_bin),
            )
            .route(
                "/world/areas/{area_id}/vis.bin",
                get(crate::net::get_inside_vis_bin),
            )
            .route("/api/editor/areas/{area_id}/assets", get(list_area_assets))
            .route(
                "/api/editor/areas/{area_id}/assets/{slug}",
                put(upload_area_asset),
            );
    }
}
