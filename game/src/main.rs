use engine::camera::Camera;
use engine::scene::{Node, NodeData};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("Starting Faith & Fire...");

    // 2. Enumerate 3D Graphics Adapters
    engine::enumerate_adapters();

    // 3. Build Camera with Z-up coordinates
    // Center of terrain is around 128, 128. Place eye behind and above it.
    let camera = Camera::new(
        glam::Vec3::new(128.0, -100.0, 150.0), // Eye position
        glam::Vec3::new(128.0, 128.0, 0.0),   // Target position (focus on center of terrain)
        1920.0 / 1080.0,                      // Aspect ratio (will be updated on resize)
    );

    // 4. Construct Scene Graph
    let mut root = Node::new("RootScene", NodeData::None);

    // Add Flat Terrain Node
    let mut terrain = Node::new("MainTerrainChunk", NodeData::Terrain);
    terrain.local_transform = glam::Mat4::IDENTITY;
    root.add_child(terrain);

    // 5. Run the Engine
    log::info!("Launching game engine event loop...");
    engine::renderer::run_engine("Faith & Fire - Terrain Demo", root, camera)?;

    Ok(())
}
