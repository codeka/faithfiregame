use std::collections::{HashMap, HashSet};

use glam::Vec3;
use serde::Serialize;
use shared::character::{CharacterCatalog, CompactCharacterAppearance};
use shared::entity::{EntityCapabilities, EntityId, TriggerConfig};
use shared::world::{
    AreaKind, AssetCatalog, AssetEntry, InsideKind, InsideVisData, ROAD_MAT_DIRT, TileVisData,
};
use web_sys::HtmlCanvasElement;

use crate::character::{AnimationClip, ParsedSkeletonRig, SkeletonPose};
use crate::render::{CharacterMeshGpu, PropMeshGpu, RoadMeshGpu, TerrainMeshGpu};

use super::io::GameIoBatch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GamePhase {
    Idle,
    Connecting,
    LoadingArea,
    Playing,
    Unauthorized,
    Error(String),
}

impl GamePhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Connecting => "connecting",
            Self::LoadingArea => "loading_area",
            Self::Playing => "playing",
            Self::Unauthorized => "unauthorized",
            Self::Error(_) => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameStatusSnapshot {
    pub phase: &'static str,
    pub player_id: Option<u64>,
    pub area_id: String,
    pub area_name: String,
    pub player_pos: Option<[f32; 3]>,
    pub fresh_cryo_wake: bool,
    pub wake_ready: bool,
    pub error: Option<String>,
}

pub struct GameInsideState {
    pub vis: InsideVisData,
    pub gpu: TerrainMeshGpu,
    pub road_meshes: HashMap<u64, RoadMeshGpu>,
}

pub struct GameTileState {
    pub vis: TileVisData,
    pub gpu: TerrainMeshGpu,
    pub road_meshes: HashMap<u64, RoadMeshGpu>,
}

pub enum ClientEntityVisualState {
    Character {
        appearance: CompactCharacterAppearance,
        pose: SkeletonPose,
        anim_time: f32,
    },
    Prop {
        asset_id: String,
        scale: [f32; 3],
        cast_shadow: bool,
    },
}

#[allow(dead_code)]
pub struct ClientEntityRenderState {
    pub entity_id: EntityId,
    pub name: Option<String>,
    pub capabilities: EntityCapabilities,
    pub render_pos: Vec3,
    pub target_pos: Vec3,
    pub render_yaw: f32,
    pub target_yaw: f32,
    pub moving: bool,
    pub visual: Option<ClientEntityVisualState>,
    pub trigger: Option<TriggerConfig>,
}

pub struct GameClientState {
    pub phase: GamePhase,
    pub local_player_id: Option<u64>,
    pub local_entity_id: Option<EntityId>,
    pub area_id: String,
    pub area_name: String,
    pub area_kind: AreaKind,
    pub dimensions: Option<(u32, u32)>,
    pub fresh_cryo_wake: bool,
    pub wake_ready: bool,
    pub area_enter_elapsed_s: f32,
    pub wake_cam_timer: Option<f32>,

    pub outside_tiles: HashMap<(i32, i32), GameTileState>,
    pub requested_tiles: HashSet<(String, i32, i32)>,
    pub inside_area: Option<GameInsideState>,
    pub requested_inside: Option<String>,

    pub asset_catalog: Vec<AssetEntry>,
    pub road_soft_edge_mask: u16,
    pub prop_meshes: HashMap<String, PropMeshGpu>,
    pub requested_assets: HashSet<String>,

    pub character_catalog: Option<CharacterCatalog>,
    pub character_catalog_requested: bool,
    pub rigs: HashMap<String, ParsedSkeletonRig>,
    pub base_meshes: HashMap<String, CharacterMeshGpu>,
    pub part_meshes: HashMap<String, CharacterMeshGpu>,
    pub extra_anims: HashMap<String, Vec<AnimationClip>>,
    pub requested_char_glbs: HashSet<String>,

    pub entities: HashMap<EntityId, ClientEntityRenderState>,
    pub move_target_marker: Option<Vec3>,
    pub hover_walkable: Option<bool>,
    pub applied_cursor: &'static str,

    pub pending_tile_fetches: Vec<(String, i32, i32)>,
    pub pending_inside_fetch: Option<String>,
    pub pending_asset_fetches: Vec<AssetEntry>,
    pub pending_char_catalog_fetch: bool,
    pub pending_char_glb_fetches: Vec<String>,
}

impl GameClientState {
    pub fn new() -> Self {
        Self {
            phase: GamePhase::Idle,
            local_player_id: None,
            local_entity_id: None,
            area_id: String::new(),
            area_name: String::new(),
            area_kind: AreaKind::Inside(InsideKind::Ephemeral),
            dimensions: None,
            fresh_cryo_wake: false,
            wake_ready: false,
            area_enter_elapsed_s: 0.0,
            wake_cam_timer: None,
            outside_tiles: HashMap::new(),
            requested_tiles: HashSet::new(),
            inside_area: None,
            requested_inside: None,
            asset_catalog: Vec::new(),
            road_soft_edge_mask: 1u16 << ROAD_MAT_DIRT,
            prop_meshes: HashMap::new(),
            requested_assets: HashSet::new(),
            character_catalog: None,
            character_catalog_requested: false,
            rigs: HashMap::new(),
            base_meshes: HashMap::new(),
            part_meshes: HashMap::new(),
            extra_anims: HashMap::new(),
            requested_char_glbs: HashSet::new(),
            entities: HashMap::new(),
            move_target_marker: None,
            hover_walkable: None,
            applied_cursor: "",
            pending_tile_fetches: Vec::new(),
            pending_inside_fetch: None,
            pending_asset_fetches: Vec::new(),
            pending_char_catalog_fetch: false,
            pending_char_glb_fetches: Vec::new(),
        }
    }

    pub fn local_entity(&self) -> Option<&ClientEntityRenderState> {
        self.local_entity_id.and_then(|id| self.entities.get(&id))
    }

    pub fn set_asset_catalog(&mut self, catalog: &AssetCatalog) {
        self.asset_catalog = catalog.decorations.clone();
        let mut mask = 1u16 << ROAD_MAT_DIRT;
        for entry in &catalog.road_textures {
            if (entry.layer as usize) < 16 {
                if entry.is_soft_edge() {
                    mask |= 1u16 << entry.layer;
                } else {
                    mask &= !(1u16 << entry.layer);
                }
            }
        }
        self.road_soft_edge_mask = mask;

        let area_id = self.area_id.clone();
        let entity_prop_ids: Vec<String> = self
            .entities
            .values()
            .filter_map(|entity| match &entity.visual {
                Some(ClientEntityVisualState::Prop { asset_id, .. }) => Some(asset_id.clone()),
                _ => None,
            })
            .collect();
        for asset_id in entity_prop_ids {
            self.ensure_asset_id_requested(&area_id, &asset_id);
        }

        if !area_id.is_empty() {
            let mut props_to_check = Vec::new();
            if let Some(inside) = &self.inside_area {
                props_to_check.extend(inside.vis.props.iter().cloned());
            }
            for tile in self.outside_tiles.values() {
                props_to_check.extend(tile.vis.props.iter().cloned());
            }
            if !props_to_check.is_empty() {
                self.ensure_prop_assets_requested(&area_id, &props_to_check);
            }
        }
    }

    #[inline]
    pub fn is_road_material_soft(&self, mat: u8) -> bool {
        (mat as usize) < 16 && (self.road_soft_edge_mask & (1u16 << mat)) != 0
    }

    pub fn reset_to_idle(&mut self, gl: &glow::Context) {
        self.clear_world_meshes(gl);
        self.phase = GamePhase::Idle;
        self.local_player_id = None;
        self.local_entity_id = None;
        self.area_id.clear();
        self.area_name.clear();
        self.dimensions = None;
        self.fresh_cryo_wake = false;
        self.wake_ready = false;
        self.area_enter_elapsed_s = 0.0;
        self.wake_cam_timer = None;
        self.entities.clear();
        self.move_target_marker = None;
        self.hover_walkable = None;
        self.applied_cursor = "";
        self.pending_tile_fetches.clear();
        self.pending_inside_fetch = None;
        self.pending_asset_fetches.clear();
    }

    pub fn begin_connecting(&mut self, gl: &glow::Context) {
        self.reset_to_idle(gl);
        self.phase = GamePhase::Connecting;
    }

    pub fn status_snapshot(&self) -> GameStatusSnapshot {
        let player_pos = self.local_entity().map(|e| e.render_pos.to_array());
        let error = match &self.phase {
            GamePhase::Error(msg) => Some(msg.clone()),
            _ => None,
        };
        GameStatusSnapshot {
            phase: self.phase.as_str(),
            player_id: self.local_player_id,
            area_id: self.area_id.clone(),
            area_name: self.area_name.clone(),
            player_pos,
            fresh_cryo_wake: self.fresh_cryo_wake,
            wake_ready: self.wake_ready,
            error,
        }
    }

    pub fn apply_cursor(&mut self, canvas: &HtmlCanvasElement, css_cursor: &'static str) {
        if self.applied_cursor != css_cursor {
            let _ = canvas.style().set_property("cursor", css_cursor);
            self.applied_cursor = css_cursor;
        }
    }

    pub fn clear_cursor(&mut self, canvas: &HtmlCanvasElement) {
        let _ = canvas.style().set_property("cursor", "");
        self.applied_cursor = "";
    }

    pub fn take_io_batch(&mut self) -> GameIoBatch {
        let char_catalog_fetch = self.pending_char_catalog_fetch;
        self.pending_char_catalog_fetch = false;
        GameIoBatch {
            tile_fetches: std::mem::take(&mut self.pending_tile_fetches),
            inside_fetch: self.pending_inside_fetch.take(),
            asset_fetches: std::mem::take(&mut self.pending_asset_fetches),
            char_catalog_fetch,
            char_glb_fetches: std::mem::take(&mut self.pending_char_glb_fetches),
        }
    }
}
