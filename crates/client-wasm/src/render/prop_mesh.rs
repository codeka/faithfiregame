use std::f32::consts::TAU;

use glam::{Mat4, Vec3};
use glow::HasContext;
use shared::world::PropPlacement;

use crate::glb_loader::ParsedPropMesh;

pub struct PropMeshGpu {
    pub(super) vao: glow::VertexArray,
    #[allow(dead_code)]
    vbo: glow::Buffer,
    #[allow(dead_code)]
    ebo: glow::Buffer,
    pub(super) index_count: i32,
    pub aabb_min: Vec3,
    pub aabb_max: Vec3,
}

impl PropMeshGpu {
    pub fn from_parsed(gl: &glow::Context, parsed: &ParsedPropMesh) -> Result<Self, String> {
        unsafe {
            let vao = gl.create_vertex_array()?;
            let vbo = gl.create_buffer()?;
            let ebo = gl.create_buffer()?;

            let mut floats = Vec::with_capacity(parsed.vertices.len() * 10);
            for v in &parsed.vertices {
                floats.extend_from_slice(&v.pos);
                floats.extend_from_slice(&v.normal);
                floats.extend_from_slice(&v.color);
            }

            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            let vbytes: &[u8] =
                std::slice::from_raw_parts(floats.as_ptr() as *const u8, floats.len() * 4);
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, vbytes, glow::STATIC_DRAW);

            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ebo));
            let ibytes: &[u8] = std::slice::from_raw_parts(
                parsed.indices.as_ptr() as *const u8,
                parsed.indices.len() * 2,
            );
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, ibytes, glow::STATIC_DRAW);

            let stride = 10 * 4;
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, stride, 12);
            gl.enable_vertex_attrib_array(2);
            gl.vertex_attrib_pointer_f32(2, 4, glow::FLOAT, false, stride, 24);
            gl.bind_vertex_array(None);

            Ok(Self {
                vao,
                vbo,
                ebo,
                index_count: parsed.indices.len() as i32,
                aabb_min: parsed.aabb_min,
                aabb_max: parsed.aabb_max,
            })
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
