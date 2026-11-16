use std::collections::HashMap;

use serde::Serialize;
use shared::world::{InsideVisData, RoadStyle, TileVisData};

use crate::render::{RoadMeshGpu, TerrainMeshGpu};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorTool {
    Heightmap,
    Texture,
    Road,
    Decoration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoadSubMode {
    Place,
    Edit,
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
    pub road_meshes: HashMap<u64, RoadMeshGpu>,
    pub dirty: bool,
}

pub struct EditorInsideState {
    pub vis: InsideVisData,
    pub gpu: TerrainMeshGpu,
    pub road_meshes: HashMap<u64, RoadMeshGpu>,
    pub dirty: bool,
}

pub struct RoadDraft {
    pub points: Vec<[f32; 3]>,
    pub locked_tile: Option<(i32, i32)>,
    pub preview_gpu: Option<RoadMeshGpu>,
    pub preview_key: Option<([f32; 3], usize, RoadStyle, u8, f32)>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedPropInfo {
    pub id: u64,
    pub asset_id: String,
    pub position: [f32; 3],
    pub rotation_deg: [f32; 3],
    pub scale: f32,
    pub cast_shadow: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedRoadInfo {
    pub id: u64,
    pub style: &'static str,
    pub material: u8,
    pub width: f32,
    pub shoulder: f32,
    pub point_count: usize,
    pub selected_node: Option<usize>,
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
    pub road_submode: &'static str,
    pub road_style: &'static str,
    pub road_material: u8,
    pub road_width: f32,
    pub road_shoulder: f32,
    pub road_draft_points: usize,
    pub selected_road: Option<SelectedRoadInfo>,
    pub decoration_submode: &'static str,
    pub selected_prop: Option<SelectedPropInfo>,
    pub preview_rotation_deg: [f32; 3],
    pub preview_scale: f32,
    pub preview_cast_shadow: bool,
}
