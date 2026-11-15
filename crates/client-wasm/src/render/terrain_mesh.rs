use glam::Vec3;
use glow::HasContext;

pub struct TerrainMeshGpu {
    pub(super) vao: glow::VertexArray,
    vbo: glow::Buffer,
    ebo: glow::Buffer,
    pub(super) splat_tex: glow::Texture,
    pub(super) index_count: i32,
    pub(super) width: usize,
    pub(super) length: usize,
}

impl TerrainMeshGpu {
    pub fn new(
        gl: &glow::Context,
        width: usize,
        length: usize,
        heights: &[f32],
        splat: &[u32],
    ) -> Result<Self, String> {
        unsafe {
            let vao = gl.create_vertex_array()?;
            let vbo = gl.create_buffer()?;
            let ebo = gl.create_buffer()?;
            let splat_tex = gl.create_texture()?;

            let mut indices: Vec<u32> = Vec::with_capacity(width * length * 6);
            let vw = width + 1;
            for z in 0..length {
                for x in 0..width {
                    let i00 = (z * vw + x) as u32;
                    let i10 = (z * vw + x + 1) as u32;
                    let i01 = ((z + 1) * vw + x) as u32;
                    let i11 = ((z + 1) * vw + x + 1) as u32;
                    indices.extend_from_slice(&[i00, i01, i10, i10, i01, i11]);
                }
            }

            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ebo));
            let idx_bytes: &[u8] =
                std::slice::from_raw_parts(indices.as_ptr() as *const u8, indices.len() * 4);
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, idx_bytes, glow::STATIC_DRAW);

            let stride = 8 * 4;
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, stride, 12);
            gl.enable_vertex_attrib_array(2);
            gl.vertex_attrib_pointer_f32(2, 2, glow::FLOAT, false, stride, 24);
            gl.bind_vertex_array(None);

            gl.bind_texture(glow::TEXTURE_2D, Some(splat_tex));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            gl.bind_texture(glow::TEXTURE_2D, None);

            let mesh = Self {
                vao,
                vbo,
                ebo,
                splat_tex,
                index_count: indices.len() as i32,
                width,
                length,
            };

            let default_normals = vec![[0.0, 1.0, 0.0]; (width + 1) * (length + 1)];
            mesh.upload_vertices(gl, 0.0, 0.0, heights, &default_normals);
            mesh.upload_splat(gl, splat);
            Ok(mesh)
        }
    }

    pub fn upload_vertices(
        &self,
        gl: &glow::Context,
        origin_x: f32,
        origin_z: f32,
        heights: &[f32],
        normals: &[[f32; 3]],
    ) {
        let vw = self.width + 1;
        let vh = self.length + 1;
        let mut floats = Vec::with_capacity(vw * vh * 8);

        for vz in 0..vh {
            for vx in 0..vw {
                let idx = vz * vw + vx;
                let h = heights.get(idx).copied().unwrap_or(0.0);
                let n = normals.get(idx).copied().unwrap_or([0.0, 1.0, 0.0]);
                floats.extend_from_slice(&[
                    origin_x + vx as f32,
                    h,
                    origin_z + vz as f32,
                    n[0],
                    n[1],
                    n[2],
                    vx as f32 / self.width as f32,
                    vz as f32 / self.length as f32,
                ]);
            }
        }

        unsafe {
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            let bytes: &[u8] =
                std::slice::from_raw_parts(floats.as_ptr() as *const u8, floats.len() * 4);
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::DYNAMIC_DRAW);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
        }
    }

    pub fn upload_splat(&self, gl: &glow::Context, splat: &[u32]) {
        let mut rgba = Vec::with_capacity(self.width * self.length * 4);
        for &packed in splat {
            rgba.extend_from_slice(&packed.to_le_bytes());
        }
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.splat_tex));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                self.width as i32,
                self.length as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&rgba)),
            );
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
    }

    pub fn destroy(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_vertex_array(self.vao);
            gl.delete_buffer(self.vbo);
            gl.delete_buffer(self.ebo);
            gl.delete_texture(self.splat_tex);
        }
    }
}

pub struct BrushOverlay {
    pub active: bool,
    pub center: Vec3,
    pub radius: f32,
    pub color: [f32; 3],
}

impl Default for BrushOverlay {
    fn default() -> Self {
        Self {
            active: false,
            center: Vec3::ZERO,
            radius: 0.0,
            color: [0.35, 0.85, 1.0],
        }
    }
}
