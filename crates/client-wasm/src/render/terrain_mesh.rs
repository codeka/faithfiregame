use glam::Vec3;
use glow::HasContext;
use shared::world::{default_splat_ids, default_splat_weights};

pub struct TerrainMeshGpu {
    pub(super) vao: glow::VertexArray,
    vbo: glow::Buffer,
    ebo: glow::Buffer,
    pub(super) splat_id_tex: glow::Texture,
    pub(super) splat_weight_tex: glow::Texture,
    pub(super) walk_tex: glow::Texture,
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
        splat_ids: &[[u8; 4]],
        splat_weights: &[[u8; 4]],
    ) -> Result<Self, String> {
        unsafe {
            let vao = gl.create_vertex_array()?;
            let vbo = gl.create_buffer()?;
            let ebo = gl.create_buffer()?;
            let splat_id_tex = gl.create_texture()?;
            let splat_weight_tex = gl.create_texture()?;
            let walk_tex = gl.create_texture()?;

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

            // Splat and walkability textures use NEAREST + CLAMP_TO_EDGE; integer RGBA8UI requires NEAREST in WebGL2.
            for tex in [splat_id_tex, splat_weight_tex, walk_tex] {
                gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            }
            gl.bind_texture(glow::TEXTURE_2D, None);

            let mesh = Self {
                vao,
                vbo,
                ebo,
                splat_id_tex,
                splat_weight_tex,
                walk_tex,
                index_count: indices.len() as i32,
                width,
                length,
            };

            let default_normals = vec![[0.0, 1.0, 0.0]; (width + 1) * (length + 1)];
            mesh.upload_vertices(gl, 0.0, 0.0, heights, &default_normals);
            mesh.upload_splat(gl, splat_ids, splat_weights);
            mesh.upload_walkability(gl, |_, _| true);
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

    pub fn upload_splat(
        &self,
        gl: &glow::Context,
        splat_ids: &[[u8; 4]],
        splat_weights: &[[u8; 4]],
    ) {
        let tex_w = (self.width + 1) as i32;
        let tex_h = (self.length + 1) as i32;
        let expected = (self.width + 1) * (self.length + 1);

        let fallback_ids;
        let fallback_weights;
        let (id_slice, weight_slice) =
            if splat_ids.len() == expected && splat_weights.len() == expected {
                (splat_ids, splat_weights)
            } else {
                fallback_ids = vec![default_splat_ids(); expected];
                fallback_weights = vec![default_splat_weights(); expected];
                (fallback_ids.as_slice(), fallback_weights.as_slice())
            };

        let id_bytes: Vec<u8> = id_slice.iter().flat_map(|c| *c).collect();
        let weight_bytes: Vec<u8> = weight_slice.iter().flat_map(|c| *c).collect();

        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.splat_id_tex));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8UI as i32,
                tex_w,
                tex_h,
                0,
                glow::RGBA_INTEGER,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&id_bytes)),
            );

            gl.bind_texture(glow::TEXTURE_2D, Some(self.splat_weight_tex));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                tex_w,
                tex_h,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&weight_bytes)),
            );
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
    }

    pub fn upload_walkability(
        &self,
        gl: &glow::Context,
        mut is_walkable: impl FnMut(usize, usize) -> bool,
    ) {
        let mut bytes = Vec::with_capacity(self.width * self.length);
        for z in 0..self.length {
            for x in 0..self.width {
                bytes.push(if is_walkable(x, z) { 255 } else { 0 });
            }
        }
        unsafe {
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.walk_tex));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::R8 as i32,
                self.width as i32,
                self.length as i32,
                0,
                glow::RED,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&bytes)),
            );
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
        }
    }

    pub fn destroy(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_vertex_array(self.vao);
            gl.delete_buffer(self.vbo);
            gl.delete_buffer(self.ebo);
            gl.delete_texture(self.splat_id_tex);
            gl.delete_texture(self.splat_weight_tex);
            gl.delete_texture(self.walk_tex);
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
