use glam::Vec3;

use super::mesh_builder::MeshBuilder;

pub(super) fn starter_decoration_assets() -> Vec<(&'static str, &'static str, &'static str, Vec<u8>)> {
    vec![
        ("pine_tree", "Pine Tree", "Nature", build_pine_tree_glb()),
        ("oak_tree", "Oak Tree", "Nature", build_oak_tree_glb()),
        ("rock_boulder", "Boulder", "Rocks", build_rock_boulder_glb()),
        ("rock_cluster", "Rock Cluster", "Rocks", build_rock_cluster_glb()),
        ("bush", "Bush", "Nature", build_bush_glb()),
        ("ruin_stone", "Ruin Stone", "Props", build_ruin_stone_glb()),
    ]
}

fn build_pine_tree_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let bark = [0.38, 0.24, 0.14, 1.0];
    let pine_dark = [0.11, 0.34, 0.16, 1.0];
    let pine_mid = [0.15, 0.42, 0.19, 1.0];
    let pine_light = [0.19, 0.48, 0.22, 1.0];

    mb.add_frustum(Vec3::ZERO, 2.2, 0.36, 0.22, 7, bark);
    mb.add_frustum(Vec3::new(0.0, 1.6, 0.0), 2.0, 1.65, 0.15, 8, pine_dark);
    mb.add_frustum(Vec3::new(0.0, 2.8, 0.0), 1.8, 1.30, 0.12, 8, pine_mid);
    mb.add_frustum(Vec3::new(0.0, 3.9, 0.0), 1.6, 0.90, 0.02, 8, pine_light);
    // Small asymmetric side branch so orientation is clearly visible
    mb.add_frustum(Vec3::new(0.0, 1.1, 0.32), 0.7, 0.24, 0.06, 6, pine_dark);
    mb.to_glb_bytes()
}

fn build_oak_tree_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let bark = [0.42, 0.27, 0.16, 1.0];
    let leaf1 = [0.22, 0.50, 0.18, 1.0];
    let leaf2 = [0.28, 0.57, 0.21, 1.0];
    let leaf3 = [0.18, 0.44, 0.15, 1.0];

    mb.add_frustum(Vec3::ZERO, 2.5, 0.48, 0.30, 8, bark);
    mb.add_ellipsoid(Vec3::new(0.0, 3.3, 0.0), Vec3::new(1.75, 1.25, 1.65), 5, 8, leaf1);
    mb.add_ellipsoid(Vec3::new(0.85, 3.0, 0.65), Vec3::new(1.1, 0.95, 1.05), 4, 7, leaf2);
    mb.add_ellipsoid(Vec3::new(-0.75, 2.9, -0.45), Vec3::new(1.0, 0.85, 1.0), 4, 7, leaf3);
    mb.to_glb_bytes()
}

fn build_rock_boulder_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let stone1 = [0.50, 0.51, 0.54, 1.0];
    let stone2 = [0.42, 0.44, 0.47, 1.0];
    mb.add_ellipsoid(Vec3::new(0.0, 0.55, 0.0), Vec3::new(1.15, 0.78, 0.95), 4, 7, stone1);
    mb.add_ellipsoid(Vec3::new(0.55, 0.38, 0.48), Vec3::new(0.62, 0.48, 0.55), 3, 6, stone2);
    mb.to_glb_bytes()
}

fn build_rock_cluster_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let stone1 = [0.48, 0.49, 0.52, 1.0];
    let stone2 = [0.56, 0.55, 0.53, 1.0];
    let stone3 = [0.39, 0.40, 0.43, 1.0];
    mb.add_ellipsoid(Vec3::new(-0.35, 0.45, -0.2), Vec3::new(0.85, 0.65, 0.75), 4, 6, stone1);
    mb.add_ellipsoid(Vec3::new(0.55, 0.32, 0.35), Vec3::new(0.65, 0.45, 0.55), 4, 6, stone2);
    mb.add_ellipsoid(Vec3::new(0.15, 0.22, -0.65), Vec3::new(0.48, 0.34, 0.44), 3, 6, stone3);
    mb.to_glb_bytes()
}

fn build_bush_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let bush1 = [0.19, 0.48, 0.17, 1.0];
    let bush2 = [0.25, 0.55, 0.21, 1.0];
    mb.add_ellipsoid(Vec3::new(0.0, 0.42, 0.0), Vec3::new(0.95, 0.62, 0.85), 4, 7, bush1);
    mb.add_ellipsoid(Vec3::new(0.38, 0.36, 0.32), Vec3::new(0.62, 0.48, 0.58), 4, 6, bush2);
    mb.to_glb_bytes()
}

fn build_ruin_stone_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let stone = [0.62, 0.60, 0.56, 1.0];
    let trim = [0.50, 0.49, 0.46, 1.0];
    // Plinth + tall carved pillar + projecting front altar ledge for unmistakable facing
    mb.add_frustum(Vec3::ZERO, 0.35, 0.85, 0.78, 4, trim);
    mb.add_frustum(Vec3::new(0.0, 0.35, 0.0), 2.15, 0.52, 0.42, 4, stone);
    mb.add_frustum(Vec3::new(0.0, 0.35, 0.52), 0.65, 0.32, 0.26, 4, trim);
    mb.to_glb_bytes()
}
