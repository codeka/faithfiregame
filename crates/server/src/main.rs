mod db;
#[cfg(feature = "editor")]
mod editor_api;
mod net;
mod seed;
mod sim;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::routing::{get, post};
use tokio::sync::{RwLock, broadcast};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
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
    seed::ensure_world_seeded(&world_dir)?;

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
        .route("/api/auth/login", post(net::login_handler))
        .route("/ws", get(net::ws_handler))
        .route("/world/areas/{area_id}/area.bin", get(net::get_area_bin))
        .route(
            "/world/areas/{area_id}/{tile_coord}/vis.bin",
            get(net::get_outside_vis_bin),
        )
        .route("/world/areas/{area_id}/vis.bin", get(net::get_inside_vis_bin))
        .nest_service("/world/assets", ServeDir::new(world_dir.join("assets")));

    #[cfg(feature = "editor")]
    let app = {
        use axum::routing::put;
        info!("Editor API endpoints ENABLED");
        app.route(
            "/api/editor/areas",
            get(editor_api::list_areas).post(editor_api::create_area),
        )
        .route(
            "/api/editor/areas/{area_id}/tiles/{tile_coord}",
            put(editor_api::save_outside_tile),
        )
        .route(
            "/api/editor/areas/{area_id}/inside",
            put(editor_api::save_inside_area),
        )
    };

    let app = app.layer(CorsLayer::permissive()).with_state(state);

    let addr = "0.0.0.0:3000";
    info!("Faith & Fire server listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
