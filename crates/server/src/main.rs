mod db;
#[cfg(feature = "editor")]
mod editor_api;
mod net;
mod sim;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::http::{HeaderValue, header};
use axum::routing::{get, post};
use tokio::sync::{RwLock, broadcast};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::db::SqliteStorage;
use crate::net::AppState;
use crate::sim::SimWorld;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,server=debug")),
        )
        .init();

    let world_dir = PathBuf::from("world");

    let db_path = PathBuf::from("faithfire.db");
    let storage = Arc::new(SqliteStorage::open(&db_path).await?);
    let sim = Arc::new(RwLock::new(SimWorld::new(world_dir.clone())));
    let (tx_broadcast, _) = broadcast::channel(128);

    // Spawn 10 Hz authoritative simulation tick loop.
    let sim_loop = Arc::clone(&sim);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_millis(100));
        loop {
            ticker.tick().await;
            sim_loop.write().await.tick();
        }
    });

    let state = AppState {
        world_dir: world_dir.clone(),
        storage,
        sim,
        tx_broadcast,
    };

    let app = Router::new()
        .route("/api/auth/play", post(net::play_guest_handler))
        .route("/api/auth/login", post(net::login_handler))
        .route("/api/auth/logout", post(net::logout_handler))
        .route("/ws", get(net::ws_handler))
        .route("/world/areas/{area_id}/area.bin", get(net::get_area_bin))
        .route(
            "/world/areas/{area_id}/assets/{file_name}",
            get(net::get_area_asset_glb),
        )
        .route(
            "/world/areas/{area_id}/{tile_coord}/vis.bin",
            get(net::get_outside_vis_bin),
        )
        .route("/world/areas/{area_id}/vis.bin", get(net::get_inside_vis_bin))
        .nest_service(
            "/world/assets",
            Router::new()
                .fallback_service(ServeDir::new(world_dir.join("assets")))
                .layer(SetResponseHeaderLayer::if_not_present(
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("no-cache"),
                )),
        )
        .nest_service(
            "/world/characters",
            Router::new()
                .fallback_service(ServeDir::new(world_dir.join("characters")))
                .layer(SetResponseHeaderLayer::if_not_present(
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("no-cache"),
                )),
        )
        .nest_service(
            "/world/items",
            Router::new()
                .fallback_service(ServeDir::new(world_dir.join("items")))
                .layer(SetResponseHeaderLayer::if_not_present(
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("no-cache"),
                )),
        );

    #[cfg(feature = "editor")]
    let app = {
        use axum::extract::DefaultBodyLimit;
        use axum::routing::put;
        info!("Editor API endpoints ENABLED");
        app.route(
            "/api/editor/areas",
            get(editor_api::list_areas).post(editor_api::create_area),
        )
        .route(
            "/api/editor/areas/{area_id}/assets",
            get(editor_api::list_area_assets),
        )
        .route(
            "/api/editor/areas/{area_id}/assets/{slug}",
            put(editor_api::upload_area_asset).layer(DefaultBodyLimit::max(64 * 1024 * 1024)),
        )
        .route(
            "/api/editor/areas/{area_id}/tiles/{tile_coord}",
            put(editor_api::save_outside_tile),
        )
        .route(
            "/api/editor/areas/{area_id}/inside",
            put(editor_api::save_inside_area),
        )
        .route(
            "/api/editor/characters/catalog",
            get(editor_api::get_character_catalog).put(editor_api::save_character_catalog),
        )
        .route(
            "/api/editor/characters/presets",
            get(editor_api::list_character_presets),
        )
        .route(
            "/api/editor/characters/presets/{preset_id}",
            put(editor_api::save_character_preset).delete(editor_api::delete_character_preset),
        )
        .route(
            "/api/editor/characters/assets/{slug}",
            put(editor_api::upload_character_asset).layer(DefaultBodyLimit::max(64 * 1024 * 1024)),
        )
        .route(
            "/api/editor/entities/templates",
            get(editor_api::list_entity_templates),
        )
        .route(
            "/api/editor/entities/templates/{template_id}",
            put(editor_api::save_entity_template).delete(editor_api::delete_entity_template),
        )
        .route(
            "/api/editor/items/catalog",
            get(editor_api::get_item_catalog),
        )
        .route(
            "/api/editor/areas/{area_id}/tiles/{tile_coord}/entities",
            get(editor_api::get_outside_tile_entities).put(editor_api::save_outside_tile_entities),
        )
        .route(
            "/api/editor/areas/{area_id}/inside/entities",
            get(editor_api::get_inside_area_entities).put(editor_api::save_inside_area_entities),
        )
    };

    let app = app.layer(CorsLayer::permissive()).with_state(state);

    let addr = "0.0.0.0:3000";
    info!("Faith & Fire server listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
