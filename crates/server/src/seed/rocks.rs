use glam::Vec3;

use super::mesh_builder::{MeshBuilder, RockSlice};

pub(super) fn extra_rock_decoration_assets() -> Vec<(&'static str, &'static str, &'static str, Vec<u8>)> {
    vec![
        ("cliff_wall", "Cliff Wall", "Rocks", build_cliff_wall_glb()),
        ("cliff_tall_pillar", "Tall Cliff Pillar", "Rocks", build_cliff_tall_pillar_glb()),
        ("cliff_ledge", "Cliff Ledge", "Rocks", build_cliff_ledge_glb()),
        ("cliff_spire", "Jagged Spire", "Rocks", build_cliff_spire_glb()),
        ("boulder_angular", "Angular Boulder", "Rocks", build_boulder_angular_glb()),
        ("boulder_flat", "Flat Slab Boulder", "Rocks", build_boulder_flat_glb()),
        ("boulder_mossy", "Mossy Boulder", "Rocks", build_boulder_mossy_glb()),
        ("rock_scree", "Scree Pile", "Rocks", build_rock_scree_glb()),
    ]
}

pub(super) fn build_rock_boulder_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let stone1 = [0.50, 0.51, 0.54, 1.0];
    let stone2 = [0.42, 0.44, 0.47, 1.0];
    mb.add_ellipsoid(Vec3::new(0.0, 0.55, 0.0), Vec3::new(1.15, 0.78, 0.95), 4, 7, stone1);
    mb.add_ellipsoid(Vec3::new(0.55, 0.38, 0.48), Vec3::new(0.62, 0.48, 0.55), 3, 6, stone2);
    mb.to_glb_bytes()
}

pub(super) fn build_rock_cluster_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let stone1 = [0.48, 0.49, 0.52, 1.0];
    let stone2 = [0.56, 0.55, 0.53, 1.0];
    let stone3 = [0.39, 0.40, 0.43, 1.0];
    mb.add_ellipsoid(Vec3::new(-0.35, 0.45, -0.2), Vec3::new(0.85, 0.65, 0.75), 4, 6, stone1);
    mb.add_ellipsoid(Vec3::new(0.55, 0.32, 0.35), Vec3::new(0.65, 0.45, 0.55), 4, 6, stone2);
    mb.add_ellipsoid(Vec3::new(0.15, 0.22, -0.65), Vec3::new(0.48, 0.34, 0.44), 3, 6, stone3);
    mb.to_glb_bytes()
}

fn build_cliff_wall_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let base = [0.38, 0.39, 0.43, 1.0];
    let mid = [0.47, 0.48, 0.51, 1.0];
    let upper = [0.55, 0.54, 0.55, 1.0];
    let crest = [0.60, 0.58, 0.56, 1.0];

    mb.add_crag_column(
        &[
            RockSlice { center: Vec3::new(0.0, -1.2, 0.0), rx: 2.60, rz: 1.15, color: base },
            RockSlice { center: Vec3::new(0.05, 0.3, 0.04), rx: 2.52, rz: 1.12, color: mid },
            RockSlice { center: Vec3::new(-0.04, 2.0, -0.05), rx: 2.45, rz: 1.04, color: mid },
            RockSlice { center: Vec3::new(0.06, 3.6, 0.02), rx: 2.38, rz: 1.00, color: upper },
            RockSlice { center: Vec3::new(0.0, 4.9, -0.04), rx: 2.25, rz: 0.95, color: crest },
        ],
        9,
        1.1,
        0.20,
    );
    // Left buttress
    mb.add_crag_column(
        &[
            RockSlice { center: Vec3::new(-1.7, -1.1, 0.25), rx: 1.15, rz: 0.95, color: base },
            RockSlice { center: Vec3::new(-1.68, 1.4, 0.22), rx: 1.02, rz: 0.86, color: mid },
            RockSlice { center: Vec3::new(-1.65, 3.8, 0.16), rx: 0.85, rz: 0.72, color: upper },
        ],
        7,
        2.7,
        0.20,
    );
    // Right crag spur
    mb.add_crag_column(
        &[
            RockSlice { center: Vec3::new(1.85, -1.1, -0.20), rx: 1.10, rz: 0.92, color: base },
            RockSlice { center: Vec3::new(1.82, 1.6, -0.18), rx: 0.96, rz: 0.82, color: mid },
            RockSlice { center: Vec3::new(1.76, 4.2, -0.15), rx: 0.76, rz: 0.66, color: crest },
        ],
        7,
        4.3,
        0.22,
    );
    mb.to_glb_bytes()
}

fn build_cliff_tall_pillar_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let dark = [0.39, 0.40, 0.44, 1.0];
    let mid = [0.49, 0.50, 0.53, 1.0];
    let pale = [0.58, 0.57, 0.57, 1.0];

    mb.add_crag_column(
        &[
            RockSlice { center: Vec3::new(0.0, -1.2, 0.0), rx: 1.25, rz: 1.15, color: dark },
            RockSlice { center: Vec3::new(0.04, 0.5, -0.02), rx: 1.18, rz: 1.08, color: mid },
            RockSlice { center: Vec3::new(-0.05, 2.2, 0.05), rx: 1.08, rz: 0.98, color: mid },
            RockSlice { center: Vec3::new(0.06, 3.8, -0.04), rx: 1.00, rz: 0.92, color: pale },
            RockSlice { center: Vec3::new(-0.02, 5.2, 0.03), rx: 0.92, rz: 0.86, color: pale },
            RockSlice { center: Vec3::new(0.0, 6.3, 0.0), rx: 0.85, rz: 0.80, color: pale },
        ],
        8,
        3.4,
        0.22,
    );
    // Side buttress
    mb.add_crag_column(
        &[
            RockSlice { center: Vec3::new(0.85, -1.0, 0.45), rx: 0.82, rz: 0.74, color: dark },
            RockSlice { center: Vec3::new(0.82, 1.2, 0.42), rx: 0.72, rz: 0.65, color: mid },
            RockSlice { center: Vec3::new(0.75, 3.4, 0.36), rx: 0.54, rz: 0.50, color: pale },
        ],
        7,
        5.8,
        0.20,
    );
    mb.to_glb_bytes()
}

fn build_cliff_ledge_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let slate = [0.41, 0.42, 0.45, 1.0];
    let warm = [0.52, 0.51, 0.49, 1.0];
    let top_stone = [0.60, 0.58, 0.54, 1.0];

    mb.add_crag_column(
        &[
            RockSlice { center: Vec3::new(0.0, -1.0, 0.0), rx: 2.00, rz: 1.30, color: slate },
            RockSlice { center: Vec3::new(0.04, 0.6, 0.02), rx: 1.98, rz: 1.34, color: warm },
            RockSlice { center: Vec3::new(-0.03, 1.8, 0.08), rx: 2.08, rz: 1.42, color: warm },
            RockSlice { center: Vec3::new(0.02, 2.7, 0.14), rx: 2.25, rz: 1.55, color: top_stone },
            RockSlice { center: Vec3::new(0.0, 3.3, 0.12), rx: 2.15, rz: 1.46, color: top_stone },
        ],
        9,
        6.2,
        0.18,
    );
    // Lower front step
    mb.add_crag_column(
        &[
            RockSlice { center: Vec3::new(0.40, -0.8, 1.05), rx: 1.15, rz: 0.78, color: slate },
            RockSlice { center: Vec3::new(0.38, 0.7, 1.02), rx: 1.06, rz: 0.72, color: warm },
            RockSlice { center: Vec3::new(0.36, 1.65, 0.98), rx: 0.94, rz: 0.64, color: top_stone },
        ],
        8,
        1.9,
        0.16,
    );
    mb.to_glb_bytes()
}

fn build_cliff_spire_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let dark = [0.36, 0.37, 0.41, 1.0];
    let mid = [0.46, 0.47, 0.51, 1.0];
    let tip = [0.58, 0.58, 0.60, 1.0];

    mb.add_crag_column(
        &[
            RockSlice { center: Vec3::new(0.0, -0.9, 0.0), rx: 1.32, rz: 1.18, color: dark },
            RockSlice { center: Vec3::new(0.08, 0.9, 0.05), rx: 1.08, rz: 0.96, color: mid },
            RockSlice { center: Vec3::new(-0.04, 2.6, 0.12), rx: 0.78, rz: 0.68, color: mid },
            RockSlice { center: Vec3::new(0.10, 4.1, -0.04), rx: 0.42, rz: 0.36, color: tip },
            RockSlice { center: Vec3::new(0.14, 5.5, -0.06), rx: 0.0, rz: 0.0, color: tip },
        ],
        7,
        7.5,
        0.24,
    );
    // Basal shard 1
    mb.add_crag_column(
        &[
            RockSlice { center: Vec3::new(-0.85, -0.7, 0.35), rx: 0.68, rz: 0.58, color: dark },
            RockSlice { center: Vec3::new(-0.92, 0.8, 0.40), rx: 0.44, rz: 0.38, color: mid },
            RockSlice { center: Vec3::new(-1.02, 2.1, 0.48), rx: 0.0, rz: 0.0, color: tip },
        ],
        6,
        2.1,
        0.22,
    );
    // Basal shard 2
    mb.add_crag_column(
        &[
            RockSlice { center: Vec3::new(0.76, -0.6, -0.42), rx: 0.56, rz: 0.52, color: dark },
            RockSlice { center: Vec3::new(0.84, 0.5, -0.48), rx: 0.34, rz: 0.30, color: mid },
            RockSlice { center: Vec3::new(0.92, 1.5, -0.52), rx: 0.0, rz: 0.0, color: tip },
        ],
        6,
        4.8,
        0.22,
    );
    mb.to_glb_bytes()
}

fn build_boulder_angular_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let granite1 = [0.40, 0.41, 0.45, 1.0];
    let granite2 = [0.34, 0.35, 0.39, 1.0];
    mb.add_faceted_boulder(
        Vec3::new(0.0, 0.72, 0.0),
        Vec3::new(1.30, 1.15, 1.08),
        4,
        6,
        1.4,
        0.22,
        granite1,
    );
    mb.add_faceted_boulder(
        Vec3::new(-0.82, 0.35, 0.45),
        Vec3::new(0.72, 0.68, 0.62),
        4,
        5,
        3.7,
        0.20,
        granite2,
    );
    mb.to_glb_bytes()
}

fn build_boulder_flat_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let stone1 = [0.51, 0.50, 0.48, 1.0];
    let stone2 = [0.57, 0.56, 0.53, 1.0];
    let stone_dark = [0.43, 0.43, 0.42, 1.0];

    mb.add_crag_column(
        &[
            RockSlice { center: Vec3::new(0.0, -0.45, 0.0), rx: 1.65, rz: 1.30, color: stone_dark },
            RockSlice { center: Vec3::new(0.02, 0.30, -0.02), rx: 1.58, rz: 1.24, color: stone1 },
            RockSlice { center: Vec3::new(-0.02, 0.95, 0.02), rx: 1.42, rz: 1.12, color: stone2 },
        ],
        8,
        5.1,
        0.16,
    );
    mb.add_crag_column(
        &[
            RockSlice { center: Vec3::new(0.95, -0.40, 0.45), rx: 0.90, rz: 0.75, color: stone_dark },
            RockSlice { center: Vec3::new(0.92, 0.15, 0.44), rx: 0.84, rz: 0.70, color: stone1 },
            RockSlice { center: Vec3::new(0.90, 0.52, 0.42), rx: 0.72, rz: 0.60, color: stone2 },
        ],
        7,
        8.3,
        0.16,
    );
    mb.to_glb_bytes()
}

fn build_boulder_mossy_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let warm_grey = [0.49, 0.48, 0.46, 1.0];
    let moss = [0.29, 0.46, 0.23, 1.0];
    let side_stone = [0.43, 0.43, 0.42, 1.0];

    mb.add_faceted_boulder(
        Vec3::new(0.0, 0.52, 0.0),
        Vec3::new(1.18, 0.88, 0.98),
        5,
        8,
        2.5,
        0.14,
        warm_grey,
    );
    mb.add_faceted_boulder(
        Vec3::new(0.02, 0.88, -0.03),
        Vec3::new(0.98, 0.56, 0.82),
        4,
        8,
        2.6,
        0.12,
        moss,
    );
    mb.add_faceted_boulder(
        Vec3::new(0.78, 0.24, 0.46),
        Vec3::new(0.54, 0.44, 0.48),
        4,
        6,
        6.1,
        0.16,
        side_stone,
    );
    mb.to_glb_bytes()
}

fn build_rock_scree_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let t1 = [0.44, 0.45, 0.48, 1.0];
    let t2 = [0.53, 0.52, 0.50, 1.0];
    let t3 = [0.37, 0.38, 0.41, 1.0];

    let stones = [
        (Vec3::new(-0.90, 0.22, -0.45), Vec3::new(0.58, 0.50, 0.52), 1.2, t1),
        (Vec3::new(-0.15, 0.32, 0.20), Vec3::new(0.72, 0.62, 0.64), 2.4, t2),
        (Vec3::new(0.72, 0.18, -0.35), Vec3::new(0.52, 0.46, 0.48), 3.6, t3),
        (Vec3::new(0.98, 0.14, 0.55), Vec3::new(0.44, 0.42, 0.40), 4.9, t1),
        (Vec3::new(-0.62, 0.16, 0.72), Vec3::new(0.48, 0.44, 0.42), 6.1, t2),
        (Vec3::new(0.24, 0.26, -0.78), Vec3::new(0.56, 0.52, 0.48), 7.3, t3),
    ];

    for (center, radii, seed, color) in stones {
        mb.add_faceted_boulder(center, radii, 3, 6, seed, 0.20, color);
    }
    mb.to_glb_bytes()
}
