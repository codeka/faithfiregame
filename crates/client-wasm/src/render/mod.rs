mod character_mesh;
mod gizmo_mesh;
mod preview;
mod prop_mesh;
mod renderer;
mod road_mesh;
mod road_textures;
mod shaders;
mod shadow_map;
mod studio_stage;
mod terrain_mesh;
mod terrain_textures;

pub use character_mesh::CharacterMeshGpu;
pub use preview::compute_item_thumbnail_camera;
#[cfg_attr(not(feature = "editor"), allow(unused_imports))]
pub use prop_mesh::{PropMeshGpu, prop_model_matrix};
pub use renderer::Renderer;
#[cfg_attr(not(feature = "editor"), allow(dead_code, unused_imports))]
pub use road_mesh::RoadMeshGpu;
pub use road_textures::{RoadLayerImage, decode_road_rgba_png};
#[cfg_attr(not(feature = "editor"), allow(unused_imports))]
pub use terrain_mesh::{BrushOverlay, TerrainMeshGpu};
pub use terrain_textures::{TerrainLayerImage, decode_albedo_png, decode_height_png};
