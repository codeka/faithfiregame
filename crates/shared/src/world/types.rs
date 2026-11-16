use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InsideKind {
    Generated,
    Ephemeral,
    Permanent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AreaKind {
    Outside,
    Inside(InsideKind),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AreaMetadata {
    pub id: String,
    pub name: String,
    pub kind: AreaKind,
    /// `(width, length)` in meters for fixed Inside areas; `None` for Outside or Generated areas.
    pub dimensions: Option<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerVolume {
    pub id: String,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub target_area: String,
    pub target_pos: [f32; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntitySpawn {
    pub id: u64,
    pub template_id: String,
    pub position: [f32; 3],
    pub leash_radius: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropPlacement {
    pub id: u64,
    pub asset_id: String,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
    pub cast_shadow: bool,
}

pub(crate) fn decode_exact<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, postcard::Error> {
    let (value, rest) = postcard::take_from_bytes::<T>(bytes)?;
    if !rest.is_empty() {
        return Err(postcard::Error::SerdeDeCustom);
    }
    Ok(value)
}

#[derive(Deserialize)]
pub(crate) struct PropPlacementV2 {
    pub id: u64,
    pub asset_id: String,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

impl From<PropPlacementV2> for PropPlacement {
    fn from(v2: PropPlacementV2) -> Self {
        Self {
            id: v2.id,
            asset_id: v2.asset_id,
            position: v2.position,
            rotation: v2.rotation,
            scale: v2.scale,
            cast_shadow: true,
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct LegacyPropPlacement {
    pub id: u64,
    pub asset_id: String,
    pub position: [f32; 3],
    pub rotation_y: f32,
    pub scale: [f32; 3],
}

impl From<LegacyPropPlacement> for PropPlacement {
    fn from(legacy: LegacyPropPlacement) -> Self {
        Self {
            id: legacy.id,
            asset_id: legacy.asset_id,
            position: legacy.position,
            rotation: [0.0, legacy.rotation_y, 0.0],
            scale: legacy.scale,
            cast_shadow: true,
        }
    }
}

/// Catalog entry for a `.glb` decoration or prop asset served over HTTP(S).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub category: String,
}

/// Catalog entry for a terrain splat material texture pair served over HTTP(S).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerrainTextureEntry {
    pub layer: u8,
    pub id: String,
    pub name: String,
    pub category: String,
    pub albedo_path: String,
    #[serde(default)]
    pub height_path: Option<String>,
}

fn default_road_edge() -> String {
    "Curbed".to_owned()
}

/// Catalog entry for a road ribbon RGBA texture served over HTTP(S).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoadTextureEntry {
    pub layer: u8,
    pub id: String,
    pub name: String,
    #[serde(default = "default_road_edge")]
    pub edge: String,
    pub path: String,
}

impl RoadTextureEntry {
    #[inline]
    pub fn is_soft_edge(&self) -> bool {
        self.edge.eq_ignore_ascii_case("soft")
    }
}

/// Unified asset catalog served from `/world/assets/catalog.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "AssetCatalogWire")]
pub struct AssetCatalog {
    pub decorations: Vec<AssetEntry>,
    pub terrain_textures: Vec<TerrainTextureEntry>,
    pub road_textures: Vec<RoadTextureEntry>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum AssetCatalogWire {
    Structured {
        #[serde(default)]
        decorations: Vec<AssetEntry>,
        #[serde(default)]
        terrain_textures: Vec<TerrainTextureEntry>,
        #[serde(default)]
        road_textures: Vec<RoadTextureEntry>,
    },
    LegacyDecorations(Vec<AssetEntry>),
}

impl From<AssetCatalogWire> for AssetCatalog {
    fn from(wire: AssetCatalogWire) -> Self {
        match wire {
            AssetCatalogWire::Structured {
                decorations,
                terrain_textures,
                road_textures,
            } => Self {
                decorations,
                terrain_textures,
                road_textures,
            },
            AssetCatalogWire::LegacyDecorations(decorations) => Self {
                decorations,
                terrain_textures: Vec::new(),
                road_textures: Vec::new(),
            },
        }
    }
}
