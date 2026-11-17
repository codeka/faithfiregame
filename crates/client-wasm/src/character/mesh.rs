use std::collections::HashMap;

use glam::{EulerRot, Mat3, Mat4, Quat, Vec3};
use gltf::Gltf;
use shared::character::MAX_CHARACTER_JOINTS;

use crate::glb_loader::{ParsedPropTexture, compute_vertex_normals, load_gltf_texture};

use super::animation::extract_gltf_animations;
use super::rig::{JointDef, ParsedSkeletonRig, RigSocketNode};

#[cfg_attr(not(feature = "editor"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharacterVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
    pub uv: [f32; 2],
    pub joints: [u8; 4],
    pub weights: [f32; 4],
}

#[cfg_attr(not(feature = "editor"), allow(dead_code))]
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCharacterSubmesh {
    pub index_offset: u32,
    pub index_count: u32,
    pub texture_index: Option<usize>,
    pub material_name: String,
    pub base_color: [f32; 4],
    pub emissive: [f32; 3],
    pub double_sided: bool,
    pub alpha_cutoff: f32,
}

#[cfg_attr(not(feature = "editor"), allow(dead_code))]
#[derive(Debug, Clone)]
pub struct ParsedCharacterMesh {
    pub is_skinned: bool,
    pub joint_names: Vec<String>,
    pub vertices: Vec<CharacterVertex>,
    pub indices: Vec<u32>,
    pub textures: Vec<ParsedPropTexture>,
    pub submeshes: Vec<ParsedCharacterSubmesh>,
    pub aabb_min: Vec3,
    pub aabb_max: Vec3,
}

struct CharBuildState<'a> {
    blob: Option<&'a [u8]>,
    is_skinned: bool,
    joint_names: Vec<String>,
    joint_index_by_name: HashMap<String, usize>,
    vertices: Vec<CharacterVertex>,
    indices: Vec<u32>,
    textures: Vec<ParsedPropTexture>,
    tex_map: HashMap<usize, usize>,
    submeshes: Vec<ParsedCharacterSubmesh>,
    aabb_min: Vec3,
    aabb_max: Vec3,
}

impl<'a> CharBuildState<'a> {
    fn intern_joint(&mut self, name: &str) -> u8 {
        if let Some(&idx) = self.joint_index_by_name.get(name) {
            return idx.min(MAX_CHARACTER_JOINTS - 1) as u8;
        }
        let idx = self.joint_names.len();
        self.joint_names.push(name.to_owned());
        self.joint_index_by_name.insert(name.to_owned(), idx);
        idx.min(MAX_CHARACTER_JOINTS - 1) as u8
    }
}

fn parse_character_gltf(gltf: &Gltf) -> Result<ParsedCharacterMesh, String> {
    let scene = gltf
        .default_scene()
        .or_else(|| gltf.scenes().next())
        .ok_or_else(|| "GLB has no scenes".to_owned())?;

    let mut state = CharBuildState {
        blob: gltf.blob.as_deref(),
        is_skinned: false,
        joint_names: Vec::new(),
        joint_index_by_name: HashMap::new(),
        vertices: Vec::new(),
        indices: Vec::new(),
        textures: Vec::new(),
        tex_map: HashMap::new(),
        submeshes: Vec::new(),
        aabb_min: Vec3::splat(f32::MAX),
        aabb_max: Vec3::splat(f32::MIN),
    };

    for node in scene.nodes() {
        visit_character_node(&node, Mat4::IDENTITY, &mut state);
    }

    if state.vertices.is_empty() {
        state.aabb_min = Vec3::splat(-0.25);
        state.aabb_max = Vec3::splat(0.25);
    }

    Ok(ParsedCharacterMesh {
        is_skinned: state.is_skinned,
        joint_names: state.joint_names,
        vertices: state.vertices,
        indices: state.indices,
        textures: state.textures,
        submeshes: state.submeshes,
        aabb_min: state.aabb_min,
        aabb_max: state.aabb_max,
    })
}

/// Parses a skinned or rigid modular character part `.glb`.
#[cfg_attr(not(feature = "editor"), allow(dead_code))]
pub fn parse_character_glb(bytes: &[u8]) -> Result<ParsedCharacterMesh, String> {
    let gltf = Gltf::from_slice(bytes).map_err(|e| format!("gltf parse failed: {e}"))?;
    parse_character_gltf(&gltf)
}

fn visit_character_node(
    node: &gltf::Node<'_>,
    parent_xform: Mat4,
    state: &mut CharBuildState<'_>,
) {
    let local = Mat4::from_cols_array_2d(&node.transform().matrix());
    let world = parent_xform * local;

    if let Some(mesh) = node.mesh() {
        // Per glTF 2.0 spec, skinned mesh vertices live in bind space ignoring ancestor node transforms.
        let skin_remap: Option<Vec<u8>> = node.skin().map(|skin| {
            state.is_skinned = true;
            skin.joints()
                .enumerate()
                .map(|(j_idx, j_node)| {
                    let name = j_node
                        .name()
                        .filter(|s| !s.is_empty())
                        .map(ToOwned::to_owned)
                        .unwrap_or_else(|| format!("joint_{j_idx}"));
                    state.intern_joint(&name)
                })
                .collect()
        });
        let mesh_xform = if skin_remap.is_some() {
            Mat4::IDENTITY
        } else {
            world
        };
        let normal_mat = Mat3::from_mat4(mesh_xform).inverse().transpose();

        for prim in mesh.primitives() {
            if prim.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let blob = state.blob;
            let reader = prim.reader(|buffer| if buffer.index() == 0 { blob } else { None });
            let Some(positions_iter) = reader.read_positions() else {
                continue;
            };
            let local_positions: Vec<[f32; 3]> = positions_iter.collect();
            if local_positions.is_empty() {
                continue;
            }

            let mat = prim.material();
            let pbr = mat.pbr_metallic_roughness();
            let material_name = mat.name().unwrap_or("").to_owned();
            let base_color = pbr.base_color_factor();
            let emissive = mat.emissive_factor();
            let double_sided = mat.double_sided();
            let alpha_cutoff = match mat.alpha_mode() {
                gltf::material::AlphaMode::Opaque => 0.0,
                gltf::material::AlphaMode::Mask => mat.alpha_cutoff().unwrap_or(0.5),
                gltf::material::AlphaMode::Blend => 0.01,
            };

            let (texture_index, uv_set) = if let Some(info) = pbr.base_color_texture() {
                let tex_idx = load_gltf_texture(
                    &info.texture(),
                    state.blob,
                    &mut state.tex_map,
                    &mut state.textures,
                );
                (tex_idx, info.tex_coord())
            } else {
                (None, 0)
            };

            let normals_opt: Option<Vec<[f32; 3]>> =
                reader.read_normals().map(|iter| iter.collect());
            let colors: Vec<[f32; 4]> = reader
                .read_colors(0)
                .map(|c| c.into_rgba_f32().collect())
                .unwrap_or_default();
            let uvs: Vec<[f32; 2]> = reader
                .read_tex_coords(uv_set)
                .map(|tc| tc.into_f32().collect())
                .unwrap_or_default();
            let raw_joints: Vec<[u16; 4]> = reader
                .read_joints(0)
                .map(|j| j.into_u16().collect())
                .unwrap_or_default();
            let raw_weights: Vec<[f32; 4]> = reader
                .read_weights(0)
                .map(|w| w.into_f32().collect())
                .unwrap_or_default();

            let prim_indices: Vec<u32> = if let Some(idx_reader) = reader.read_indices() {
                idx_reader.into_u32().collect()
            } else {
                (0..local_positions.len() as u32).collect()
            };
            if prim_indices.len() < 3 {
                continue;
            }

            let base_vert = state.vertices.len() as u32;
            let mut world_positions = Vec::with_capacity(local_positions.len());
            for &pos in &local_positions {
                let wp = mesh_xform.transform_point3(Vec3::from(pos));
                state.aabb_min = state.aabb_min.min(wp);
                state.aabb_max = state.aabb_max.max(wp);
                world_positions.push(wp);
            }

            let world_normals: Vec<[f32; 3]> = if let Some(raw_normals) = normals_opt {
                (0..local_positions.len())
                    .map(|i| {
                        let n_raw = raw_normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
                        let wn = (normal_mat * Vec3::from(n_raw)).normalize_or_zero();
                        if wn.length_squared() > 1e-6 {
                            wn.to_array()
                        } else {
                            [0.0, 1.0, 0.0]
                        }
                    })
                    .collect()
            } else {
                compute_vertex_normals(&world_positions, &prim_indices)
            };

            for (i, wp) in world_positions.into_iter().enumerate() {
                let vc = colors.get(i).copied().unwrap_or([1.0, 1.0, 1.0, 1.0]);
                let uv = uvs.get(i).copied().unwrap_or([0.0, 0.0]);

                let (joints, weights) = if let Some(remap) = &skin_remap {
                    let rj = raw_joints.get(i).copied().unwrap_or([0, 0, 0, 0]);
                    let mut rw = raw_weights.get(i).copied().unwrap_or([1.0, 0.0, 0.0, 0.0]);
                    let mapped_j = [
                        remap.get(rj[0] as usize).copied().unwrap_or(0),
                        remap.get(rj[1] as usize).copied().unwrap_or(0),
                        remap.get(rj[2] as usize).copied().unwrap_or(0),
                        remap.get(rj[3] as usize).copied().unwrap_or(0),
                    ];
                    let sum = rw[0] + rw[1] + rw[2] + rw[3];
                    if sum > 1e-6 {
                        rw = [rw[0] / sum, rw[1] / sum, rw[2] / sum, rw[3] / sum];
                    } else {
                        rw = [1.0, 0.0, 0.0, 0.0];
                    }
                    (mapped_j, rw)
                } else {
                    ([0, 0, 0, 0], [1.0, 0.0, 0.0, 0.0])
                };

                state.vertices.push(CharacterVertex {
                    pos: wp.to_array(),
                    normal: world_normals[i],
                    color: vc,
                    uv,
                    joints,
                    weights,
                });
            }

            let index_offset = state.indices.len() as u32;
            let tri_index_count = (prim_indices.len() / 3) * 3;
            for &idx in &prim_indices[..tri_index_count] {
                if (idx as usize) < local_positions.len() {
                    state.indices.push(base_vert + idx);
                }
            }
            let added_count = (state.indices.len() as u32).saturating_sub(index_offset);
            if added_count == 0 {
                continue;
            }

            if let Some(last) = state.submeshes.last_mut().filter(|s| {
                s.index_offset + s.index_count == index_offset
                    && s.texture_index == texture_index
                    && s.material_name == material_name
                    && s.base_color == base_color
                    && s.emissive == emissive
                    && s.double_sided == double_sided
                    && (s.alpha_cutoff - alpha_cutoff).abs() < 1e-5
            }) {
                last.index_count += added_count;
            } else {
                state.submeshes.push(ParsedCharacterSubmesh {
                    index_offset,
                    index_count: added_count,
                    texture_index,
                    material_name,
                    base_color,
                    emissive,
                    double_sided,
                    alpha_cutoff,
                });
            }
        }
    }

    for child in node.children() {
        visit_character_node(&child, world, state);
    }
}

/// Parses a skeleton `.glb` extracting joint hierarchy, inverse bind matrices, embedded `socket_*` nodes,
/// optional base body mesh, and embedded animation clips.
#[cfg_attr(not(feature = "editor"), allow(dead_code))]
pub fn parse_skeleton_glb(bytes: &[u8]) -> Result<ParsedSkeletonRig, String> {
    let gltf = Gltf::from_slice(bytes).map_err(|e| format!("gltf parse failed: {e}"))?;
    let skin = gltf
        .skins()
        .next()
        .ok_or_else(|| "Skeleton GLB has no skin".to_owned())?;

    let mut node_parent: HashMap<usize, usize> = HashMap::new();
    for node in gltf.nodes() {
        for child in node.children() {
            node_parent.insert(child.index(), node.index());
        }
    }

    let joint_nodes: Vec<gltf::Node<'_>> = skin.joints().collect();
    if joint_nodes.is_empty() {
        return Err("Skeleton GLB skin has no joints".to_owned());
    }
    if joint_nodes.len() > MAX_CHARACTER_JOINTS {
        return Err(format!(
            "Skeleton has {} joints (max supported: {})",
            joint_nodes.len(),
            MAX_CHARACTER_JOINTS
        ));
    }

    let mut joint_by_node_idx: HashMap<usize, usize> = HashMap::with_capacity(joint_nodes.len());
    for (j_idx, node) in joint_nodes.iter().enumerate() {
        joint_by_node_idx.insert(node.index(), j_idx);
    }

    let blob = gltf.blob.as_deref();
    let skin_reader = skin.reader(|buffer| if buffer.index() == 0 { blob } else { None });
    let ibms: Vec<Mat4> = skin_reader
        .read_inverse_bind_matrices()
        .map(|iter| iter.map(|m| Mat4::from_cols_array_2d(&m)).collect())
        .unwrap_or_default();

    let mut bones = Vec::with_capacity(joint_nodes.len());
    let mut bone_index_by_name = HashMap::with_capacity(joint_nodes.len());
    for (j_idx, node) in joint_nodes.iter().enumerate() {
        let name = node
            .name()
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("joint_{j_idx}"));
        let parent = node_parent
            .get(&node.index())
            .and_then(|p_node| joint_by_node_idx.get(p_node).copied());
        let (t, r, s) = node.transform().decomposed();
        let inverse_bind = ibms.get(j_idx).copied().unwrap_or(Mat4::IDENTITY);
        bone_index_by_name.insert(name.clone(), j_idx);
        bones.push(JointDef {
            name,
            parent,
            rest_translation: Vec3::from(t),
            rest_rotation: Quat::from_xyzw(r[0], r[1], r[2], r[3]).normalize(),
            rest_scale: Vec3::from(s),
            inverse_bind,
        });
    }

    let mut embedded_sockets = Vec::new();
    for node in gltf.nodes() {
        let Some(name) = node.name() else {
            continue;
        };
        if !name.starts_with("socket_") {
            continue;
        }
        let parent_bone = node_parent
            .get(&node.index())
            .and_then(|&p_node| joint_by_node_idx.get(&p_node).copied())
            .map(|j| bones[j].name.clone())
            .unwrap_or_else(|| bones[0].name.clone());
        let (t, r, s) = node.transform().decomposed();
        let q = Quat::from_xyzw(r[0], r[1], r[2], r[3]).normalize();
        let (ry, rx, rz) = q.to_euler(EulerRot::YXZ);
        embedded_sockets.push(RigSocketNode {
            name: name.to_owned(),
            parent_bone,
            offset: t,
            rotation_deg: [rx.to_degrees(), ry.to_degrees(), rz.to_degrees()],
            scale: s[0],
        });
    }

    let base_mesh = parse_character_gltf(&gltf)
        .ok()
        .filter(|m| !m.vertices.is_empty() && !m.indices.is_empty());
    let animations = extract_gltf_animations(&gltf);

    Ok(ParsedSkeletonRig::new(
        bones,
        bone_index_by_name,
        embedded_sockets,
        base_mesh,
        animations,
    ))
}
