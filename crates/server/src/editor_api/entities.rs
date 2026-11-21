use std::fs;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use shared::entity::{EntityTemplate, PlacedEntity};
use shared::item::ItemCatalog;
use shared::world::{
    AreaMetadata, InsideSimData, InsideVisData, TileSimData, TileVisData,
    compute_inside_sim_from_vis, compute_sim_from_vis, is_valid_asset_slug, prettify_asset_slug,
};
use tracing::info;

use crate::net::{AppState, is_valid_area_id, parse_tile_coord};

pub async fn get_item_catalog(
    State(state): State<AppState>,
) -> Result<Json<ItemCatalog>, (StatusCode, String)> {
    let path = state.world_dir.join("items").join("catalog.json");
    if !path.exists() {
        return Ok(Json(ItemCatalog::default()));
    }
    let bytes = fs::read(&path).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let catalog: ItemCatalog =
        serde_json::from_slice(&bytes).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(catalog))
}

pub async fn list_entity_templates(
    State(state): State<AppState>,
) -> Result<Json<Vec<EntityTemplate>>, (StatusCode, String)> {
    let dir = state.world_dir.join("entities").join("templates");
    if !dir.exists() {
        return Ok(Json(Vec::new()));
    }
    let entries = fs::read_dir(&dir).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let mut list = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(tpl) = serde_json::from_slice::<EntityTemplate>(&bytes) {
                list.push(tpl);
            }
        }
    }
    list.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Json(list))
}

pub async fn save_entity_template(
    State(state): State<AppState>,
    Path(template_id): Path<String>,
    Json(mut template): Json<EntityTemplate>,
) -> Result<Json<EntityTemplate>, (StatusCode, String)> {
    if !is_valid_asset_slug(&template_id) {
        return Err((
            StatusCode::BAD_REQUEST,
            "template id must be 1-64 lowercase ASCII [a-z0-9_-] chars".to_owned(),
        ));
    }
    if template.name.trim().is_empty() {
        template.name = prettify_asset_slug(&template_id);
    }
    template.id = template_id.clone();

    let dir = state.world_dir.join("entities").join("templates");
    fs::create_dir_all(&dir).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let bytes = serde_json::to_vec_pretty(&template)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    fs::write(dir.join(format!("{template_id}.json")), bytes)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    state
        .sim
        .write()
        .await
        .upsert_entity_template(template.clone());
    info!("saved entity template '{}'", template.id);
    Ok(Json(template))
}

pub async fn delete_entity_template(
    State(state): State<AppState>,
    Path(template_id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    if !is_valid_asset_slug(&template_id) {
        return Err((
            StatusCode::BAD_REQUEST,
            "template id must be 1-64 lowercase ASCII [a-z0-9_-] chars".to_owned(),
        ));
    }
    let path = state
        .world_dir
        .join("entities")
        .join("templates")
        .join(format!("{template_id}.json"));
    if path.exists() {
        fs::remove_file(&path).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    state.sim.write().await.remove_entity_template(&template_id);
    info!("deleted entity template '{}'", template_id);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_outside_tile_entities(
    State(state): State<AppState>,
    Path((area_id, tile_coord)): Path<(String, String)>,
) -> Result<Json<Vec<PlacedEntity>>, (StatusCode, String)> {
    if !is_valid_area_id(&area_id) {
        return Err((StatusCode::BAD_REQUEST, "invalid area id".to_owned()));
    }
    let (tx, ty) = parse_tile_coord(&tile_coord)
        .ok_or((StatusCode::BAD_REQUEST, "invalid tile coord".to_owned()))?;

    let sim_path = state
        .world_dir
        .join("areas")
        .join(&area_id)
        .join(format!("{tx},{ty}"))
        .join("sim.bin");
    if !sim_path.exists() {
        return Ok(Json(Vec::new()));
    }
    let bytes =
        fs::read(&sim_path).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let sim = TileSimData::from_postcard_bytes(&bytes)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("invalid TileSimData: {e}")))?;
    Ok(Json(sim.entities))
}

pub async fn save_outside_tile_entities(
    State(state): State<AppState>,
    Path((area_id, tile_coord)): Path<(String, String)>,
    Json(entities): Json<Vec<PlacedEntity>>,
) -> Result<StatusCode, (StatusCode, String)> {
    if !is_valid_area_id(&area_id) {
        return Err((StatusCode::BAD_REQUEST, "invalid area id".to_owned()));
    }
    let (tx, ty) = parse_tile_coord(&tile_coord)
        .ok_or((StatusCode::BAD_REQUEST, "invalid tile coord".to_owned()))?;

    let area_dir = state.world_dir.join("areas").join(&area_id);
    if !area_dir.join("area.bin").exists() {
        return Err((StatusCode::NOT_FOUND, "area does not exist".to_owned()));
    }
    let tile_dir = area_dir.join(format!("{tx},{ty}"));
    fs::create_dir_all(&tile_dir)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let sim_path = tile_dir.join("sim.bin");
    let vis_path = tile_dir.join("vis.bin");
    let mut sim = if let Ok(bytes) = fs::read(&sim_path) {
        TileSimData::from_postcard_bytes(&bytes)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("invalid TileSimData: {e}")))?
    } else if let Ok(bytes) = fs::read(&vis_path) {
        let vis = TileVisData::from_postcard_bytes(&bytes)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("invalid TileVisData: {e}")))?;
        compute_sim_from_vis(&vis, None)
    } else {
        TileSimData::flat_default()
    };

    sim.entities = entities;
    let sim_bytes = postcard::to_allocvec(&sim)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    fs::write(&sim_path, sim_bytes)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    state
        .sim
        .write()
        .await
        .hot_reload_outside_tile(&area_id, tx, ty, sim);

    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_inside_area_entities(
    State(state): State<AppState>,
    Path(area_id): Path<String>,
) -> Result<Json<Vec<PlacedEntity>>, (StatusCode, String)> {
    if !is_valid_area_id(&area_id) {
        return Err((StatusCode::BAD_REQUEST, "invalid area id".to_owned()));
    }
    let sim_path = state.world_dir.join("areas").join(&area_id).join("sim.bin");
    if !sim_path.exists() {
        return Ok(Json(Vec::new()));
    }
    let bytes =
        fs::read(&sim_path).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let sim = InsideSimData::from_postcard_bytes(&bytes)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("invalid InsideSimData: {e}")))?;
    Ok(Json(sim.entities))
}

pub async fn save_inside_area_entities(
    State(state): State<AppState>,
    Path(area_id): Path<String>,
    Json(entities): Json<Vec<PlacedEntity>>,
) -> Result<StatusCode, (StatusCode, String)> {
    if !is_valid_area_id(&area_id) {
        return Err((StatusCode::BAD_REQUEST, "invalid area id".to_owned()));
    }
    let area_dir = state.world_dir.join("areas").join(&area_id);
    if !area_dir.exists() {
        return Err((StatusCode::NOT_FOUND, "area does not exist".to_owned()));
    }

    let sim_path = area_dir.join("sim.bin");
    let vis_path = area_dir.join("vis.bin");
    let mut sim = if let Ok(bytes) = fs::read(&sim_path) {
        InsideSimData::from_postcard_bytes(&bytes).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("invalid InsideSimData: {e}"),
            )
        })?
    } else if let Ok(bytes) = fs::read(&vis_path) {
        let vis = InsideVisData::from_postcard_bytes(&bytes).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("invalid InsideVisData: {e}"),
            )
        })?;
        compute_inside_sim_from_vis(&vis, None)
    } else {
        let meta = fs::read(area_dir.join("area.bin"))
            .ok()
            .and_then(|b| postcard::from_bytes::<AreaMetadata>(&b).ok());
        let (w, l) = meta.and_then(|m| m.dimensions).unwrap_or((32, 32));
        InsideSimData::flat_default(w, l)
    };

    sim.entities = entities;
    let sim_bytes = postcard::to_allocvec(&sim)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    fs::write(&sim_path, sim_bytes)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    state.sim.write().await.hot_reload_inside(&area_id, sim);
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use axum::Router;
    use axum::routing::{get, put};
    use shared::entity::{EntityOverrides, MovementConfig, PropertyBag, PropertyValue, TriggerConfig};
    use tokio::sync::{RwLock, broadcast};

    use super::*;
    use crate::db::SqliteStorage;
    use crate::editor_api::areas::{CreateAreaRequest, create_area};
    use crate::sim::SimWorld;

    fn unique_temp_dir(label: &str) -> PathBuf {
        let unique = format!(
            "ff_ed_ent_{label}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(unique)
    }

    #[test]
    fn axum_routes_for_entity_editor_compile_without_conflict() {
        let _router: Router<AppState> = Router::new()
            .route("/api/editor/items/catalog", get(get_item_catalog))
            .route("/api/editor/entities/templates", get(list_entity_templates))
            .route(
                "/api/editor/entities/templates/{template_id}",
                put(save_entity_template).delete(delete_entity_template),
            )
            .route(
                "/api/editor/areas/{area_id}/tiles/{tile_coord}/entities",
                get(get_outside_tile_entities).put(save_outside_tile_entities),
            )
            .route(
                "/api/editor/areas/{area_id}/inside/entities",
                get(get_inside_area_entities).put(save_inside_area_entities),
            );
    }

    #[tokio::test]
    async fn entity_template_and_area_entity_crud_roundtrips() {
        let root = unique_temp_dir("crud");
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

        let mut props = PropertyBag::new();
        props.set("quest_id", PropertyValue::String("intro_wake".to_owned()));
        let tpl = EntityTemplate {
            id: "cryo_terminal".to_owned(),
            name: "Cryo Terminal".to_owned(),
            properties: props,
            visual: None,
            movement: Some(MovementConfig { speed: 2.0 }),
            trigger: Some(TriggerConfig::default()),
            script: None,
            inventory: None,
            lootable: None,
        };

        let Json(saved_tpl) = save_entity_template(
            State(state.clone()),
            Path("cryo_terminal".to_owned()),
            Json(tpl),
        )
        .await
        .unwrap();
        assert_eq!(saved_tpl.id, "cryo_terminal");

        let Json(listed) = list_entity_templates(State(state.clone())).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Cryo Terminal");

        // Create Outside area and Inside area, then save & fetch placed entities.
        let _ = create_area(
            State(state.clone()),
            Json(CreateAreaRequest {
                id: "outlands".to_owned(),
                name: "Outlands".to_owned(),
                kind: "outside".to_owned(),
                width: None,
                length: None,
            }),
        )
        .await
        .unwrap();
        let _ = create_area(
            State(state.clone()),
            Json(CreateAreaRequest {
                id: "bunker".to_owned(),
                name: "Bunker".to_owned(),
                kind: "inside_ephemeral".to_owned(),
                width: Some(16),
                length: Some(16),
            }),
        )
        .await
        .unwrap();

        let placed = vec![PlacedEntity {
            id: 1001,
            template_id: "cryo_terminal".to_owned(),
            position: [5.5, 0.0, 7.5],
            rotation_y: 0.75,
            overrides: EntityOverrides::default(),
        }];

        assert_eq!(
            save_outside_tile_entities(
                State(state.clone()),
                Path(("outlands".to_owned(), "0,0".to_owned())),
                Json(placed.clone()),
            )
            .await
            .unwrap(),
            StatusCode::NO_CONTENT
        );
        let Json(out_loaded) = get_outside_tile_entities(
            State(state.clone()),
            Path(("outlands".to_owned(), "0,0".to_owned())),
        )
        .await
        .unwrap();
        assert_eq!(out_loaded, placed);

        assert_eq!(
            save_inside_area_entities(
                State(state.clone()),
                Path("bunker".to_owned()),
                Json(placed.clone()),
            )
            .await
            .unwrap(),
            StatusCode::NO_CONTENT
        );
        let Json(in_loaded) =
            get_inside_area_entities(State(state.clone()), Path("bunker".to_owned()))
                .await
                .unwrap();
        assert_eq!(in_loaded, placed);

        assert_eq!(
            delete_entity_template(State(state.clone()), Path("cryo_terminal".to_owned()))
                .await
                .unwrap(),
            StatusCode::NO_CONTENT
        );
        let Json(after_del) = list_entity_templates(State(state)).await.unwrap();
        assert!(after_del.is_empty());

        let _ = fs::remove_dir_all(&root);
    }
}
