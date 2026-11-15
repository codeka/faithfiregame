mod procedural_textures;
mod prop_mesh;
mod renderer;
mod shaders;
mod terrain_mesh;

#[cfg_attr(not(feature = "editor"), allow(unused_imports))]
pub use prop_mesh::{PropMeshGpu, prop_model_matrix};
pub use renderer::Renderer;
#[cfg_attr(not(feature = "editor"), allow(unused_imports))]
pub use terrain_mesh::{BrushOverlay, TerrainMeshGpu};
