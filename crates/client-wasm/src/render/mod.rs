mod gizmo_mesh;
mod procedural_textures;
mod prop_mesh;
mod renderer;
mod road_mesh;
mod road_textures;
mod shaders;
mod shadow_map;
mod terrain_mesh;

#[cfg_attr(not(feature = "editor"), allow(unused_imports))]
pub use prop_mesh::{PropMeshGpu, prop_model_matrix};
pub use renderer::Renderer;
#[cfg_attr(not(feature = "editor"), allow(dead_code, unused_imports))]
pub use road_mesh::RoadMeshGpu;
#[cfg_attr(not(feature = "editor"), allow(unused_imports))]
pub use terrain_mesh::{BrushOverlay, TerrainMeshGpu};
