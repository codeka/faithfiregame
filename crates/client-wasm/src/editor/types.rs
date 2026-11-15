use serde::Serialize;
use shared::world::{InsideVisData, TileVisData};

use crate::render::TerrainMeshGpu;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorTool {
    Heightmap,
    Texture,
    Decoration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeightMode {
    Raise,
    Lower,
    Smooth,
    Flatten,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorationSubMode {
    Place,
    Move,
    FreeMove,
    Rotate,
}

pub enum PendingSave {
    Outside {
        area_id: String,
        tx: i32,
        ty: i32,
        bytes: Vec<u8>,
    },
    Inside {
        area_id: String,
        bytes: Vec<u8>,
    },
}

pub struct EditorTileState {
    pub vis: TileVisData,
    pub gpu: TerrainMeshGpu,
    pub dirty: bool,
}

pub struct EditorInsideState {
    pub vis: InsideVisData,
    pub gpu: TerrainMeshGpu,
    pub dirty: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedPropInfo {
    pub id: u64,
    pub asset_id: String,
    pub position: [f32; 3],
    pub rotation_deg: [f32; 3],
    pub scale: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorStatusSnapshot {
    pub area_id: String,
    pub area_name: String,
    pub is_outside: bool,
    pub current_tile: [i32; 2],
    pub dimensions: Option<[u32; 2]>,
    pub cursor_pos: Option<[f32; 3]>,
    pub dirty_count: usize,
    pub save_state: String,
    pub tool: &'static str,
    pub height_mode: &'static str,
    pub texture_layer: usize,
    pub decoration_submode: &'static str,
    pub selected_prop: Option<SelectedPropInfo>,
    pub preview_rotation_deg: [f32; 3],
    pub preview_scale: f32,
}
