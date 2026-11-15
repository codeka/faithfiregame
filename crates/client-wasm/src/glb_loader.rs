use glam::{Mat3, Mat4, Vec3};
use gltf::Gltf;

#[derive(Debug, Clone, Copy)]
pub struct PropVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
}

pub struct ParsedPropMesh {
    pub vertices: Vec<PropVertex>,
    pub indices: Vec<u16>,
    pub aabb_min: Vec3,
    pub aabb_max: Vec3,
}

/// Parses a binary `.glb` asset into a flattened mesh with local AABB bounds.
pub fn parse_glb(bytes: &[u8]) -> Result<ParsedPropMesh, String> {
    let gltf = Gltf::from_slice(bytes).map_err(|e| format!("gltf parse failed: {e}"))?;
    let blob = gltf
        .blob
        .as_deref()
        .ok_or_else(|| "GLB is missing embedded binary buffer".to_owned())?;

    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut aabb_min = Vec3::splat(f32::MAX);
    let mut aabb_max = Vec3::splat(f32::MIN);

    let scene = gltf
        .default_scene()
        .or_else(|| gltf.scenes().next())
        .ok_or_else(|| "GLB has no scenes".to_owned())?;

    for node in scene.nodes() {
        visit_node(
            &node,
            Mat4::IDENTITY,
            blob,
            &mut vertices,
            &mut indices,
            &mut aabb_min,
            &mut aabb_max,
        );
    }

    if vertices.is_empty() {
        aabb_min = Vec3::splat(-0.5);
        aabb_max = Vec3::splat(0.5);
    }

    Ok(ParsedPropMesh {
        vertices,
        indices,
        aabb_min,
        aabb_max,
    })
}

fn visit_node(
    node: &gltf::Node<'_>,
    parent_xform: Mat4,
    blob: &[u8],
    vertices: &mut Vec<PropVertex>,
    indices: &mut Vec<u16>,
    aabb_min: &mut Vec3,
    aabb_max: &mut Vec3,
) {
    let local = Mat4::from_cols_array_2d(&node.transform().matrix());
    let world = parent_xform * local;
    let normal_mat = Mat3::from_mat4(world).inverse().transpose();

    if let Some(mesh) = node.mesh() {
        for prim in mesh.primitives() {
            let reader = prim.reader(|buffer| {
                if buffer.index() == 0 {
                    Some(blob)
                } else {
                    None
                }
            });

            let Some(positions) = reader.read_positions() else {
                continue;
            };
            let base_color = prim.material().pbr_metallic_roughness().base_color_factor();
            let normals: Vec<[f32; 3]> = reader
                .read_normals()
                .map(|iter| iter.collect())
                .unwrap_or_default();
            let colors: Vec<[f32; 4]> = reader
                .read_colors(0)
                .map(|c| c.into_rgba_f32().collect())
                .unwrap_or_default();

            let base_idx = vertices.len() as u16;
            for (i, pos) in positions.enumerate() {
                let wp = world.transform_point3(Vec3::from(pos));
                *aabb_min = aabb_min.min(wp);
                *aabb_max = aabb_max.max(wp);

                let n_raw = normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
                let wn = (normal_mat * Vec3::from(n_raw)).normalize_or_zero();
                let vc = colors.get(i).copied().unwrap_or([1.0, 1.0, 1.0, 1.0]);
                let col = [
                    vc[0] * base_color[0],
                    vc[1] * base_color[1],
                    vc[2] * base_color[2],
                    vc[3] * base_color[3],
                ];

                vertices.push(PropVertex {
                    pos: wp.to_array(),
                    normal: wn.to_array(),
                    color: col,
                });
            }

            if let Some(idx_reader) = reader.read_indices() {
                for idx in idx_reader.into_u32() {
                    indices.push(base_idx + idx as u16);
                }
            } else {
                let count = (vertices.len() as u16).saturating_sub(base_idx);
                for i in 0..count {
                    indices.push(base_idx + i);
                }
            }
        }
    }

    for child in node.children() {
        visit_node(&child, world, blob, vertices, indices, aabb_min, aabb_max);
    }
}
