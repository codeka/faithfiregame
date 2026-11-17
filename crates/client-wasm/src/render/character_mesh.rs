use glam::{Mat4, Vec3};
use glow::HasContext;
use shared::character::MAX_CHARACTER_JOINTS;

use crate::character::{CharacterVertex, ParsedCharacterMesh, ParsedSkeletonRig, SkeletonPose};

use super::prop_mesh::upload_prop_textures;

#[cfg_attr(not(feature = "editor"), allow(dead_code))]
pub struct CharacterSubmeshGpu {
    pub(super) index_offset_bytes: i32,
    pub(super) index_count: i32,
    pub(super) texture_index: Option<usize>,
    pub(super) material_name: String,
    pub(super) base_color: [f32; 4],
    pub(super) emissive: [f32; 3],
    pub(super) double_sided: bool,
    pub(super) alpha_cutoff: f32,
}

#[cfg_attr(not(feature = "editor"), allow(dead_code))]
pub struct CharacterMeshGpu {
    pub(super) vao: glow::VertexArray,
    vbo: glow::Buffer,
    ebo: glow::Buffer,
    pub is_skinned: bool,
    pub joint_names: Vec<String>,
    pub(super) total_index_count: i32,
    pub(super) textures: Vec<glow::Texture>,
    pub(super) submeshes: Vec<CharacterSubmeshGpu>,
    #[allow(dead_code)]
    pub aabb_min: Vec3,
    #[allow(dead_code)]
    pub aabb_max: Vec3,
}

pub(crate) const CHARACTER_VERTEX_STRIDE: i32 = 68;

pub(crate) fn pack_character_vertices(vertices: &[CharacterVertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * CHARACTER_VERTEX_STRIDE as usize);
    for v in vertices {
        for f in v.pos {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
        for f in v.normal {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
        for f in v.color {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
        for f in v.uv {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
        bytes.extend_from_slice(&v.joints);
        for f in v.weights {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
    }
    bytes
}

impl CharacterMeshGpu {
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn from_parsed(gl: &glow::Context, parsed: &ParsedCharacterMesh) -> Result<Self, String> {
        unsafe {
            let vao = gl.create_vertex_array()?;
            let vbo = gl.create_buffer()?;
            let ebo = gl.create_buffer()?;

            let vbytes = pack_character_vertices(&parsed.vertices);
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &vbytes, glow::STATIC_DRAW);

            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ebo));
            let ibytes: &[u8] = std::slice::from_raw_parts(
                parsed.indices.as_ptr() as *const u8,
                parsed.indices.len() * 4,
            );
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, ibytes, glow::STATIC_DRAW);

            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, CHARACTER_VERTEX_STRIDE, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, CHARACTER_VERTEX_STRIDE, 12);
            gl.enable_vertex_attrib_array(2);
            gl.vertex_attrib_pointer_f32(2, 4, glow::FLOAT, false, CHARACTER_VERTEX_STRIDE, 24);
            gl.enable_vertex_attrib_array(3);
            gl.vertex_attrib_pointer_f32(3, 2, glow::FLOAT, false, CHARACTER_VERTEX_STRIDE, 40);
            gl.enable_vertex_attrib_array(4);
            gl.vertex_attrib_pointer_i32(4, 4, glow::UNSIGNED_BYTE, CHARACTER_VERTEX_STRIDE, 48);
            gl.enable_vertex_attrib_array(5);
            gl.vertex_attrib_pointer_f32(5, 4, glow::FLOAT, false, CHARACTER_VERTEX_STRIDE, 52);
            gl.bind_vertex_array(None);

            let textures = upload_prop_textures(gl, &parsed.textures)?;

            let submeshes = if parsed.submeshes.is_empty() && !parsed.indices.is_empty() {
                vec![CharacterSubmeshGpu {
                    index_offset_bytes: 0,
                    index_count: parsed.indices.len() as i32,
                    texture_index: None,
                    material_name: String::new(),
                    base_color: [1.0, 1.0, 1.0, 1.0],
                    emissive: [0.0; 3],
                    double_sided: false,
                    alpha_cutoff: 0.0,
                }]
            } else {
                parsed
                    .submeshes
                    .iter()
                    .map(|s| CharacterSubmeshGpu {
                        index_offset_bytes: (s.index_offset as i32) * 4,
                        index_count: s.index_count as i32,
                        texture_index: s.texture_index,
                        material_name: s.material_name.clone(),
                        base_color: s.base_color,
                        emissive: s.emissive,
                        double_sided: s.double_sided,
                        alpha_cutoff: s.alpha_cutoff,
                    })
                    .collect()
            };

            Ok(Self {
                vao,
                vbo,
                ebo,
                is_skinned: parsed.is_skinned,
                joint_names: parsed.joint_names.clone(),
                total_index_count: parsed.indices.len() as i32,
                textures,
                submeshes,
                aabb_min: parsed.aabb_min,
                aabb_max: parsed.aabb_max,
            })
        }
    }

    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn remapped_skin_palette(
        &self,
        rig: &ParsedSkeletonRig,
        pose: &SkeletonPose,
    ) -> [[f32; 16]; MAX_CHARACTER_JOINTS] {
        let mut out = [Mat4::IDENTITY.to_cols_array(); MAX_CHARACTER_JOINTS];
        for (dst_idx, joint_name) in self.joint_names.iter().enumerate().take(MAX_CHARACTER_JOINTS) {
            if let Some(&rig_idx) = rig.bone_index_by_name.get(joint_name) {
                if rig_idx < MAX_CHARACTER_JOINTS {
                    out[dst_idx] = pose.skin_matrices[rig_idx];
                }
            }
        }
        out
    }

    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn destroy(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_vertex_array(self.vao);
            gl.delete_buffer(self.vbo);
            gl.delete_buffer(self.ebo);
            for &tex in &self.textures {
                gl.delete_texture(tex);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_character_vertex_matches_68_byte_stride() {
        let v = CharacterVertex {
            pos: [1.0, 2.0, 3.0],
            normal: [0.0, 1.0, 0.0],
            color: [1.0, 0.5, 0.25, 1.0],
            uv: [0.25, 0.75],
            joints: [1, 2, 3, 4],
            weights: [0.5, 0.25, 0.25, 0.0],
        };
        let bytes = pack_character_vertices(&[v]);
        assert_eq!(bytes.len(), CHARACTER_VERTEX_STRIDE as usize);
        assert_eq!(&bytes[48..52], &[1, 2, 3, 4]);
    }
}
