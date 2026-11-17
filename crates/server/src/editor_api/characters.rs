use std::fs;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Serialize;
use shared::character::{CharacterCatalog, CharacterDefinition};
use shared::world::{is_valid_asset_slug, prettify_asset_slug, sanitize_asset_slug};
use tracing::info;

use crate::net::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct UploadedCharacterAsset {
    pub slug: String,
    pub name: String,
    pub path: String,
}

fn validate_glb_bytes(body: &[u8]) -> Result<(), (StatusCode, String)> {
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
    Ok(())
}

pub async fn get_character_catalog(
    State(state): State<AppState>,
) -> Result<Json<CharacterCatalog>, (StatusCode, String)> {
    let path = state.world_dir.join("characters").join("catalog.json");
    if !path.exists() {
        return Ok(Json(CharacterCatalog::default()));
    }
    let bytes = fs::read(&path).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let catalog: CharacterCatalog = serde_json::from_slice(&bytes)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(catalog))
}

pub async fn save_character_catalog(
    State(state): State<AppState>,
    Json(catalog): Json<CharacterCatalog>,
) -> Result<Json<CharacterCatalog>, (StatusCode, String)> {
    for skel in &catalog.skeletons {
        if !is_valid_asset_slug(&skel.id) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("invalid skeleton id '{}'", skel.id),
            ));
        }
        for slot in &skel.skinned_slots {
            if !is_valid_asset_slug(&slot.id) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("invalid skinned slot id '{}'", slot.id),
                ));
            }
        }
        for sock in &skel.sockets {
            if !is_valid_asset_slug(&sock.id) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("invalid socket id '{}'", sock.id),
                ));
            }
        }
    }
    for part in &catalog.parts {
        if !is_valid_asset_slug(&part.id) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("invalid part id '{}'", part.id),
            ));
        }
        if !is_valid_asset_slug(&part.skeleton_id) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("invalid part skeleton_id '{}'", part.skeleton_id),
            ));
        }
    }

    let chars_dir = state.world_dir.join("characters");
    fs::create_dir_all(&chars_dir)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let bytes = serde_json::to_vec_pretty(&catalog)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    fs::write(chars_dir.join("catalog.json"), bytes)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    info!(
        "saved character catalog ({} skeletons, {} parts)",
        catalog.skeletons.len(),
        catalog.parts.len()
    );
    Ok(Json(catalog))
}

pub async fn list_character_presets(
    State(state): State<AppState>,
) -> Result<Json<Vec<CharacterDefinition>>, (StatusCode, String)> {
    let presets_dir = state.world_dir.join("characters").join("presets");
    if !presets_dir.exists() {
        return Ok(Json(Vec::new()));
    }
    let entries =
        fs::read_dir(&presets_dir).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let mut list = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(def) = serde_json::from_slice::<CharacterDefinition>(&bytes) {
                list.push(def);
            }
        }
    }
    list.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Json(list))
}

pub async fn save_character_preset(
    State(state): State<AppState>,
    Path(preset_id): Path<String>,
    Json(mut def): Json<CharacterDefinition>,
) -> Result<Json<CharacterDefinition>, (StatusCode, String)> {
    if !is_valid_asset_slug(&preset_id) {
        return Err((
            StatusCode::BAD_REQUEST,
            "preset id must be 1-64 lowercase ASCII [a-z0-9_-] chars".to_owned(),
        ));
    }
    if def.name.trim().is_empty() {
        def.name = prettify_asset_slug(&preset_id);
    }
    def.id = preset_id.clone();

    let presets_dir = state.world_dir.join("characters").join("presets");
    fs::create_dir_all(&presets_dir)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let bytes = serde_json::to_vec_pretty(&def)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    fs::write(presets_dir.join(format!("{preset_id}.json")), bytes)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    info!("saved character preset '{}'", def.id);
    Ok(Json(def))
}

pub async fn delete_character_preset(
    State(state): State<AppState>,
    Path(preset_id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    if !is_valid_asset_slug(&preset_id) {
        return Err((
            StatusCode::BAD_REQUEST,
            "preset id must be 1-64 lowercase ASCII [a-z0-9_-] chars".to_owned(),
        ));
    }
    let path = state
        .world_dir
        .join("characters")
        .join("presets")
        .join(format!("{preset_id}.json"));
    if path.exists() {
        fs::remove_file(&path).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        info!("deleted character preset '{}'", preset_id);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn upload_character_asset(
    State(state): State<AppState>,
    Path(raw_slug): Path<String>,
    body: Bytes,
) -> Result<Json<UploadedCharacterAsset>, (StatusCode, String)> {
    let slug = sanitize_asset_slug(&raw_slug);
    if !is_valid_asset_slug(&slug) {
        return Err((StatusCode::BAD_REQUEST, "invalid character asset slug".to_owned()));
    }
    validate_glb_bytes(&body)?;

    let assets_dir = state.world_dir.join("assets").join("characters");
    fs::create_dir_all(&assets_dir)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let dest = assets_dir.join(format!("{slug}.glb"));
    fs::write(&dest, &body).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let uploaded = UploadedCharacterAsset {
        name: prettify_asset_slug(&slug),
        path: format!("/world/assets/characters/{slug}.glb"),
        slug,
    };
    info!(
        "uploaded character asset '{}' ({} bytes)",
        uploaded.path,
        body.len()
    );
    Ok(Json(uploaded))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::routing::{get, put};

    #[test]
    fn validates_glb_headers_and_rejects_corrupt_payloads() {
        assert!(validate_glb_bytes(b"not-a-glb").is_err());
        let mut wrong_len = Vec::new();
        wrong_len.extend_from_slice(b"glTF");
        wrong_len.extend_from_slice(&2u32.to_le_bytes());
        wrong_len.extend_from_slice(&99u32.to_le_bytes());
        assert!(validate_glb_bytes(&wrong_len).is_err());

        let mut valid = Vec::new();
        valid.extend_from_slice(b"glTF");
        valid.extend_from_slice(&2u32.to_le_bytes());
        valid.extend_from_slice(&12u32.to_le_bytes());
        assert!(validate_glb_bytes(&valid).is_ok());
    }

    #[test]
    fn axum_routes_for_character_editor_compile_without_conflict() {
        let _router: Router<AppState> = Router::new()
            .route(
                "/api/editor/characters/catalog",
                get(get_character_catalog).put(save_character_catalog),
            )
            .route("/api/editor/characters/presets", get(list_character_presets))
            .route(
                "/api/editor/characters/presets/{preset_id}",
                put(save_character_preset).delete(delete_character_preset),
            )
            .route(
                "/api/editor/characters/assets/{slug}",
                put(upload_character_asset),
            );
    }
}
