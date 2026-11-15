use std::path::PathBuf;
use std::sync::Arc;

use shared::protocol::ServerMessage;
use tokio::sync::{RwLock, broadcast};

use crate::db::SqliteStorage;
use crate::sim::SimWorld;

#[derive(Clone)]
pub struct AppState {
    pub world_dir: PathBuf,
    pub storage: Arc<SqliteStorage>,
    #[allow(dead_code)]
    pub sim: Arc<RwLock<SimWorld>>,
    pub tx_broadcast: broadcast::Sender<ServerMessage>,
}

/// Validates that an area ID only contains safe path characters (`[a-zA-Z0-9_-]`).
pub fn is_valid_area_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub fn parse_tile_coord(coord: &str) -> Option<(i32, i32)> {
    let (x_str, y_str) = coord.split_once(',')?;
    let tx = x_str.parse::<i32>().ok()?;
    let ty = y_str.parse::<i32>().ok()?;
    Some((tx, ty))
}
