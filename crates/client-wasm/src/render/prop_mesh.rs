use std::f32::consts::TAU;

use glam::{Mat4, Vec3};
use glow::HasContext;
use shared::world::PropPlacement;

use crate::glb_loader::{ParsedPropMesh, ParsedPropTexture, PropTexWrap};

pub struct PropSubmeshGpu {
    pub(super) index_offset_bytes: i32,
    pub(super) index_count: i32,
    pub(super) texture_index: Option<usize>,
    pub(super) emissive: [f32; 3],
    pub(super) double_sided: bool,
    pub(super) alpha_cutoff: f32,
}

pub struct PropMeshGpu {
    pub(super) vao: glow::VertexArray,
    vbo: glow::Buffer,
    ebo: glow::Buffer,
    pub(super) total_index_count: i32,
    pub(super) textures: Vec<glow::Texture>,
    pub(super) submeshes: Vec<PropSubmeshGpu>,
    pub aabb_min: Vec3,
    pub aabb_max: Vec3,
}

fn gl_wrap_mode(mode: PropTexWrap) -> i32 {
    match mode {
        PropTexWrap::Repeat => glow::REPEAT as i32,
        PropTexWrap::ClampToEdge => glow::CLAMP_TO_EDGE as i32,
        PropTexWrap::MirroredRepeat => glow::MIRRORED_REPEAT as i32,
    }
}

pub(super) fn upload_prop_textures(
    gl: &glow::Context,
    parsed_textures: &[ParsedPropTexture],
) -> Result<Vec<glow::Texture>, String> {
    unsafe {
        let mut textures = Vec::with_capacity(parsed_textures.len());
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
        for tex_data in parsed_textures {
            let tex = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                tex_data.width as i32,
                tex_data.height as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&tex_data.rgba8)),
            );
            if tex_data.nearest_filter {
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MIN_FILTER,
                    glow::NEAREST as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MAG_FILTER,
                    glow::NEAREST as i32,
                );
            } else {
                gl.generate_mipmap(glow::TEXTURE_2D);
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MIN_FILTER,
                    glow::LINEAR_MIPMAP_LINEAR as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MAG_FILTER,
                    glow::LINEAR as i32,
                );
            }
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                gl_wrap_mode(tex_data.wrap_s),
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                gl_wrap_mode(tex_data.wrap_t),
            );
            textures.push(tex);
        }
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
        gl.bind_texture(glow::TEXTURE_2D, None);
        Ok(textures)
    }
}

impl PropMeshGpu {
    pub fn from_parsed(gl: &glow::Context, parsed: &ParsedPropMesh) -> Result<Self, String> {
        unsafe {
            let vao = gl.create_vertex_array()?;
            let vbo = gl.create_buffer()?;
            let ebo = gl.create_buffer()?;

            let mut floats = Vec::with_capacity(parsed.vertices.len() * 12);
            for v in &parsed.vertices {
                floats.extend_from_slice(&v.pos);
                floats.extend_from_slice(&v.normal);
                floats.extend_from_slice(&v.color);
                floats.extend_from_slice(&v.uv);
            }

            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            let vbytes: &[u8] =
                std::slice::from_raw_parts(floats.as_ptr() as *const u8, floats.len() * 4);
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, vbytes, glow::STATIC_DRAW);

            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ebo));
            let ibytes: &[u8] = std::slice::from_raw_parts(
                parsed.indices.as_ptr() as *const u8,
                parsed.indices.len() * 4,
            );
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, ibytes, glow::STATIC_DRAW);

            let stride = 12 * 4;
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, stride, 12);
            gl.enable_vertex_attrib_array(2);
            gl.vertex_attrib_pointer_f32(2, 4, glow::FLOAT, false, stride, 24);
            gl.enable_vertex_attrib_array(3);
            gl.vertex_attrib_pointer_f32(3, 2, glow::FLOAT, false, stride, 40);
            gl.bind_vertex_array(None);

            let textures = upload_prop_textures(gl, &parsed.textures)?;

            let submeshes = if parsed.submeshes.is_empty() && !parsed.indices.is_empty() {
                vec![PropSubmeshGpu {
                    index_offset_bytes: 0,
                    index_count: parsed.indices.len() as i32,
                    texture_index: None,
                    emissive: [0.0; 3],
                    double_sided: false,
                    alpha_cutoff: 0.0,
                }]
            } else {
                parsed
                    .submeshes
                    .iter()
                    .map(|s| PropSubmeshGpu {
                        index_offset_bytes: (s.index_offset as i32) * 4,
                        index_count: s.index_count as i32,
                        texture_index: s.texture_index,
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
                total_index_count: parsed.indices.len() as i32,
                textures,
                submeshes,
                aabb_min: parsed.aabb_min,
                aabb_max: parsed.aabb_max,
            })
        }
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

pub fn prop_model_matrix(p: &PropPlacement) -> Mat4 {
    Mat4::from_translation(Vec3::from(p.position))
        * Mat4::from_euler(glam::EulerRot::YXZ, p.rotation[1], p.rotation[0], p.rotation[2])
        * Mat4::from_scale(Vec3::from(p.scale))
}

pub(super) fn create_selection_ring_mesh(
    gl: &glow::Context,
) -> Result<(glow::VertexArray, i32), String> {
    let segs = 32usize;
    let mut pts = Vec::with_capacity((segs + 1) * 6);
    for i in 0..segs {
        let a0 = (i as f32 / segs as f32) * TAU;
        let a1 = ((i + 1) as f32 / segs as f32) * TAU;
        pts.extend_from_slice(&[a0.cos(), 0.0, a0.sin(), a1.cos(), 0.0, a1.sin()]);
    }
    // Forward heading line (+Z in local space)
    pts.extend_from_slice(&[0.0, 0.0, 0.0, 0.0, 0.0, 1.45]);

    unsafe {
        let vao = gl.create_vertex_array()?;
        let vbo = gl.create_buffer()?;
        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        let bytes: &[u8] = std::slice::from_raw_parts(pts.as_ptr() as *const u8, pts.len() * 4);
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STATIC_DRAW);
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 12, 0);
        gl.bind_vertex_array(None);
        Ok((vao, (pts.len() / 3) as i32))
    }
}
