use engine::camera::Camera;
use engine::scene::{Node, NodeData};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("Starting Faith & Fire...");

    // 2. Enumerate 3D Graphics Adapters
    engine::enumerate_adapters();

    // 3. Build Camera
    // Position the camera in front of the center of the screen, looking at the origin
    let camera = Camera::new(
        glam::Vec3::new(0.0, 0.0, 2.0), // Eye position
        glam::Vec3::new(0.0, 0.0, 0.0), // Target position
        1920.0 / 1080.0,                  // Aspect ratio (will be updated on resize)
    );

    // 4. Construct Scene Graph
    let mut root = Node::new("RootScene", NodeData::None);

    let mut triangle = Node::new("MainTriangle", NodeData::Triangle);
    // Put a small translation or rotation on the triangle if desired, e.g. identity for now.
    triangle.local_transform = glam::Mat4::IDENTITY;

    root.add_child(triangle);

    // 5. Run the Engine
    log::info!("Launching game engine event loop...");
    engine::renderer::run_engine("Faith & Fire", root, camera)?;

    Ok(())
}
