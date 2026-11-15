use std::f32::consts::TAU;

use glam::{Mat4, Vec3, Vec4};
use glow::HasContext;
use shared::world::PropPlacement;

use crate::camera::Camera;
use crate::glb_loader::ParsedPropMesh;

const TERRAIN_VERT: &str = r#"#version 300 es
precision highp float;

layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_normal;
layout(location = 2) in vec2 a_uv;

uniform mat4 u_view_proj;

out vec3 v_world_pos;
out vec3 v_normal;
out vec2 v_uv;

void main() {
    v_world_pos = a_pos;
    v_normal = a_normal;
    v_uv = a_uv;
    gl_Position = u_view_proj * vec4(a_pos, 1.0);
}
"#;

const TERRAIN_FRAG: &str = r#"#version 300 es
precision highp float;
precision highp sampler2DArray;

in vec3 v_world_pos;
in vec3 v_normal;
in vec2 v_uv;

uniform sampler2D u_splat_tex;
uniform sampler2DArray u_tex_array;
uniform vec3 u_camera_pos;
uniform vec3 u_tile_origin;
uniform vec2 u_tile_size;
uniform int u_show_grid;
uniform vec3 u_brush_center;
uniform float u_brush_radius;
uniform vec3 u_brush_color;
uniform float u_fog_start;
uniform float u_fog_end;
uniform vec3 u_fog_color;

out vec4 frag_color;

void main() {
    vec4 weights = texture(u_splat_tex, v_uv);
    float sum_w = max(weights.r + weights.g + weights.b + weights.a, 0.0001);
    weights /= sum_w;

    vec2 world_uv = v_world_pos.xz * 0.25;
    vec3 c0 = texture(u_tex_array, vec3(world_uv, 0.0)).rgb;
    vec3 c1 = texture(u_tex_array, vec3(world_uv, 1.0)).rgb;
    vec3 c2 = texture(u_tex_array, vec3(world_uv, 2.0)).rgb;
    vec3 c3 = texture(u_tex_array, vec3(world_uv, 3.0)).rgb;

    vec3 albedo = c0 * weights.r + c1 * weights.g + c2 * weights.b + c3 * weights.a;

    vec3 smooth_n = normalize(v_normal);
    vec3 geom_n = normalize(cross(dFdx(v_world_pos), dFdy(v_world_pos)));
    if (geom_n.y < 0.0) {
        geom_n = -geom_n;
    }
    vec3 n = normalize(mix(smooth_n, geom_n, 0.22));

    vec3 sun_dir = normalize(vec3(-0.72, 0.48, 0.50));
    float diff = max(dot(n, sun_dir), 0.0);
    float hemi = 0.5 + 0.5 * n.y;
    vec3 ambient = mix(vec3(0.22, 0.25, 0.30), vec3(0.34, 0.38, 0.45), hemi);
    vec3 sun_light = diff * vec3(1.06, 1.00, 0.88) * 1.05;
    vec3 lit = albedo * (ambient + sun_light);

    if (u_show_grid != 0) {
        vec2 local_xz = v_world_pos.xz - u_tile_origin.xz;
        vec2 cell_f = abs(fract(local_xz - 0.5) - 0.5);
        float cell_dist = min(cell_f.x, cell_f.y);
        float cell_line = 1.0 - smoothstep(0.0, 0.035, cell_dist);
        lit = mix(lit, lit * 0.82 + vec3(0.06), cell_line * 0.35);

        float edge_x = min(local_xz.x, u_tile_size.x - local_xz.x);
        float edge_z = min(local_xz.y, u_tile_size.y - local_xz.y);
        float edge_dist = min(edge_x, edge_z);
        float border = 1.0 - smoothstep(0.0, 0.18, edge_dist);
        lit = mix(lit, vec3(0.95, 0.78, 0.28), border * 0.65);
    }

    if (u_brush_radius > 0.05) {
        float d = length(v_world_pos.xz - u_brush_center.xz);
        float ring = 1.0 - smoothstep(0.0, 0.16, abs(d - u_brush_radius));
        float fill = smoothstep(u_brush_radius, 0.0, d) * 0.12;
        lit = mix(lit, u_brush_color, clamp(ring * 0.85 + fill, 0.0, 1.0));
    }

    float cam_dist = length(v_world_pos - u_camera_pos);
    float fog_t = smoothstep(u_fog_start, u_fog_end, cam_dist);
    vec3 final_rgb = mix(lit, u_fog_color, fog_t);

    frag_color = vec4(final_rgb, 1.0);
}
"#;

const PROP_VERT: &str = r#"#version 300 es
precision highp float;

layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_normal;
layout(location = 2) in vec4 a_color;

uniform mat4 u_view_proj;
uniform mat4 u_model;

out vec3 v_world_pos;
out vec3 v_normal;
out vec4 v_color;

void main() {
    vec4 wp = u_model * vec4(a_pos, 1.0);
    v_world_pos = wp.xyz;
    v_normal = normalize(mat3(u_model) * a_normal);
    v_color = a_color;
    gl_Position = u_view_proj * wp;
}
"#;

const PROP_FRAG: &str = r#"#version 300 es
precision highp float;

in vec3 v_world_pos;
in vec3 v_normal;
in vec4 v_color;

uniform vec3 u_camera_pos;
uniform vec4 u_tint;
uniform float u_fog_start;
uniform float u_fog_end;
uniform vec3 u_fog_color;

out vec4 frag_color;

void main() {
    vec3 n = normalize(v_normal);
    vec3 sun_dir = normalize(vec3(-0.72, 0.48, 0.50));
    float diff = max(dot(n, sun_dir), 0.0);
    float hemi = 0.5 + 0.5 * n.y;
    vec3 ambient = mix(vec3(0.22, 0.25, 0.30), vec3(0.34, 0.38, 0.45), hemi);
    vec3 base = v_color.rgb * u_tint.rgb;
    vec3 lit = base * (ambient + diff * vec3(1.06, 1.00, 0.88) * 1.00);

    float cam_dist = length(v_world_pos - u_camera_pos);
    float fog_t = smoothstep(u_fog_start, u_fog_end, cam_dist);
    vec3 final_rgb = mix(lit, u_fog_color, fog_t);

    frag_color = vec4(final_rgb, v_color.a * u_tint.a);
}
"#;

const LINE_VERT: &str = r#"#version 300 es
precision highp float;

layout(location = 0) in vec3 a_pos;

uniform mat4 u_view_proj;
uniform mat4 u_model;

void main() {
    gl_Position = u_view_proj * u_model * vec4(a_pos, 1.0);
}
"#;

const LINE_FRAG: &str = r#"#version 300 es
precision highp float;

uniform vec4 u_color;
out vec4 frag_color;

void main() {
    frag_color = u_color;
}
"#;

pub struct TerrainMeshGpu {
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    ebo: glow::Buffer,
    splat_tex: glow::Texture,
    index_count: i32,
    width: usize,
    length: usize,
}

impl TerrainMeshGpu {
    pub fn new(gl: &glow::Context, width: usize, length: usize, heights: &[f32], splat: &[u32]) -> Result<Self, String> {
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

pub struct PropMeshGpu {
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    ebo: glow::Buffer,
    index_count: i32,
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

pub struct Renderer {
    pub gl: glow::Context,
    terrain_prog: glow::Program,
    prop_prog: glow::Program,
    line_prog: glow::Program,
    tex_array: glow::Texture,
    ring_vao: glow::VertexArray,
    ring_vert_count: i32,
    pub fog_start: f32,
    pub fog_end: f32,
    pub fog_color: [f32; 3],
}

impl Renderer {
    pub fn new(gl: glow::Context) -> Result<Self, String> {
        let terrain_prog = compile_program(&gl, TERRAIN_VERT, TERRAIN_FRAG)?;
        let prop_prog = compile_program(&gl, PROP_VERT, PROP_FRAG)?;
        let line_prog = compile_program(&gl, LINE_VERT, LINE_FRAG)?;
        let tex_array = create_terrain_texture_array(&gl)?;
        let (ring_vao, ring_vert_count) = create_selection_ring_mesh(&gl)?;

        unsafe {
            gl.enable(glow::DEPTH_TEST);
            gl.enable(glow::CULL_FACE);
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
        }

        Ok(Self {
            gl,
            terrain_prog,
            prop_prog,
            line_prog,
            tex_array,
            ring_vao,
            ring_vert_count,
            fog_start: 140.0,
            fog_end: 240.0,
            fog_color: [0.56, 0.72, 0.86],
        })
    }

    pub fn begin_frame(&self, width: i32, height: i32) {
        unsafe {
            self.gl.viewport(0, 0, width.max(1), height.max(1));
            self.gl
                .clear_color(self.fog_color[0], self.fog_color[1], self.fog_color[2], 1.0);
            self.gl
                .clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        }
    }

    pub fn draw_terrain_chunk(
        &self,
        camera: &Camera,
        mesh: &TerrainMeshGpu,
        origin_x: f32,
        origin_z: f32,
        show_grid: bool,
        brush: &BrushOverlay,
    ) {
        let vp = camera.view_proj_matrix();
        unsafe {
            let gl = &self.gl;
            gl.use_program(Some(self.terrain_prog));

            let loc = |name| gl.get_uniform_location(self.terrain_prog, name);
            gl.uniform_matrix_4_f32_slice(loc("u_view_proj").as_ref(), false, vp.as_ref());
            gl.uniform_3_f32(loc("u_camera_pos").as_ref(), camera.eye.x, camera.eye.y, camera.eye.z);
            gl.uniform_3_f32(loc("u_tile_origin").as_ref(), origin_x, 0.0, origin_z);
            gl.uniform_2_f32(loc("u_tile_size").as_ref(), mesh.width as f32, mesh.length as f32);
            gl.uniform_1_i32(loc("u_show_grid").as_ref(), i32::from(show_grid));
            gl.uniform_3_f32(
                loc("u_brush_center").as_ref(),
                brush.center.x,
                brush.center.y,
                brush.center.z,
            );
            gl.uniform_1_f32(
                loc("u_brush_radius").as_ref(),
                if brush.active { brush.radius } else { 0.0 },
            );
            gl.uniform_3_f32(
                loc("u_brush_color").as_ref(),
                brush.color[0],
                brush.color[1],
                brush.color[2],
            );
            gl.uniform_1_f32(loc("u_fog_start").as_ref(), self.fog_start);
            gl.uniform_1_f32(loc("u_fog_end").as_ref(), self.fog_end);
            gl.uniform_3_f32(
                loc("u_fog_color").as_ref(),
                self.fog_color[0],
                self.fog_color[1],
                self.fog_color[2],
            );

            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(mesh.splat_tex));
            gl.uniform_1_i32(loc("u_splat_tex").as_ref(), 0);

            gl.active_texture(glow::TEXTURE1);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.tex_array));
            gl.uniform_1_i32(loc("u_tex_array").as_ref(), 1);

            gl.bind_vertex_array(Some(mesh.vao));
            gl.draw_elements(glow::TRIANGLES, mesh.index_count, glow::UNSIGNED_INT, 0);
            gl.bind_vertex_array(None);
        }
    }

    pub fn draw_prop(&self, camera: &Camera, mesh: &PropMeshGpu, placement: &PropPlacement, tint: Vec4) {
        let vp = camera.view_proj_matrix();
        let model = prop_model_matrix(placement);
        unsafe {
            let gl = &self.gl;
            gl.use_program(Some(self.prop_prog));
            let loc = |name| gl.get_uniform_location(self.prop_prog, name);

            gl.uniform_matrix_4_f32_slice(loc("u_view_proj").as_ref(), false, vp.as_ref());
            gl.uniform_matrix_4_f32_slice(loc("u_model").as_ref(), false, model.as_ref());
            gl.uniform_3_f32(loc("u_camera_pos").as_ref(), camera.eye.x, camera.eye.y, camera.eye.z);
            gl.uniform_4_f32(loc("u_tint").as_ref(), tint.x, tint.y, tint.z, tint.w);
            gl.uniform_1_f32(loc("u_fog_start").as_ref(), self.fog_start);
            gl.uniform_1_f32(loc("u_fog_end").as_ref(), self.fog_end);
            gl.uniform_3_f32(
                loc("u_fog_color").as_ref(),
                self.fog_color[0],
                self.fog_color[1],
                self.fog_color[2],
            );

            gl.bind_vertex_array(Some(mesh.vao));
            gl.draw_elements(glow::TRIANGLES, mesh.index_count, glow::UNSIGNED_SHORT, 0);
            gl.bind_vertex_array(None);
        }
    }

    pub fn draw_selection_ring(&self, camera: &Camera, placement: &PropPlacement, radius: f32) {
        let vp = camera.view_proj_matrix();
        let model = Mat4::from_translation(Vec3::new(
            placement.position[0],
            placement.position[1] + 0.08,
            placement.position[2],
        )) * Mat4::from_rotation_y(placement.rotation_y)
            * Mat4::from_scale(Vec3::splat(radius.max(0.8)));

        unsafe {
            let gl = &self.gl;
            gl.use_program(Some(self.line_prog));
            let loc = |name| gl.get_uniform_location(self.line_prog, name);
            gl.uniform_matrix_4_f32_slice(loc("u_view_proj").as_ref(), false, vp.as_ref());
            gl.uniform_matrix_4_f32_slice(loc("u_model").as_ref(), false, model.as_ref());
            gl.uniform_4_f32(loc("u_color").as_ref(), 1.0, 0.88, 0.25, 1.0);

            gl.bind_vertex_array(Some(self.ring_vao));
            gl.draw_arrays(glow::LINES, 0, self.ring_vert_count);
            gl.bind_vertex_array(None);
        }
    }
}

pub fn prop_model_matrix(p: &PropPlacement) -> Mat4 {
    Mat4::from_translation(Vec3::from(p.position))
        * Mat4::from_rotation_y(p.rotation_y)
        * Mat4::from_scale(Vec3::from(p.scale))
}

fn compile_program(gl: &glow::Context, vert_src: &str, frag_src: &str) -> Result<glow::Program, String> {
    unsafe {
        let prog = gl.create_program()?;
        let vs = compile_shader(gl, glow::VERTEX_SHADER, vert_src)?;
        let fs = compile_shader(gl, glow::FRAGMENT_SHADER, frag_src)?;
        gl.attach_shader(prog, vs);
        gl.attach_shader(prog, fs);
        gl.link_program(prog);
        if !gl.get_program_link_status(prog) {
            return Err(format!("shader link error: {}", gl.get_program_info_log(prog)));
        }
        gl.delete_shader(vs);
        gl.delete_shader(fs);
        Ok(prog)
    }
}

unsafe fn compile_shader(
    gl: &glow::Context,
    shader_type: u32,
    src: &str,
) -> Result<glow::Shader, String> {
    unsafe {
        let shader = gl.create_shader(shader_type)?;
        gl.shader_source(shader, src);
        gl.compile_shader(shader);
        if !gl.get_shader_compile_status(shader) {
            return Err(format!("shader compile error: {}", gl.get_shader_info_log(shader)));
        }
        Ok(shader)
    }
}

fn create_selection_ring_mesh(gl: &glow::Context) -> Result<(glow::VertexArray, i32), String> {
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

fn hash2(x: i32, y: i32) -> f32 {
    let n = (x.wrapping_mul(374761393) ^ y.wrapping_mul(668265263)).wrapping_mul(1274126177);
    ((n ^ (n >> 13)) as u32 & 0xFFFF) as f32 / 65535.0
}

fn tileable_noise(u: f32, v: f32, period: i32) -> f32 {
    let x = u * period as f32;
    let y = v * period as f32;
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);

    let wrap = |v: i32| v.rem_euclid(period);
    let n00 = hash2(wrap(x0), wrap(y0));
    let n10 = hash2(wrap(x0 + 1), wrap(y0));
    let n01 = hash2(wrap(x0), wrap(y0 + 1));
    let n11 = hash2(wrap(x0 + 1), wrap(y0 + 1));

    let nx0 = n00 + (n10 - n00) * sx;
    let nx1 = n01 + (n11 - n01) * sx;
    nx0 + (nx1 - nx0) * sy
}

fn fbm_tileable(u: f32, v: f32) -> f32 {
    0.55 * tileable_noise(u, v, 8)
        + 0.30 * tileable_noise(u, v, 16)
        + 0.15 * tileable_noise(u, v, 32)
}

fn create_terrain_texture_array(gl: &glow::Context) -> Result<glow::Texture, String> {
    const SIZE: usize = 256;
    const LAYERS: usize = 4;
    let mut pixels = vec![0u8; SIZE * SIZE * LAYERS * 4];

    for layer in 0..LAYERS {
        for y in 0..SIZE {
            for x in 0..SIZE {
                let u = x as f32 / SIZE as f32;
                let v = y as f32 / SIZE as f32;
                let n = fbm_tileable(u, v);
                let fine = tileable_noise(u, v, 64);

                let rgb: [f32; 3] = match layer {
                    // 0: Meadow Grass
                    0 => {
                        let g = 0.52 + (n - 0.5) * 0.12 + (fine - 0.5) * 0.05;
                        [0.24 + (n - 0.5) * 0.06, g, 0.18 + (n - 0.5) * 0.05]
                    }
                    // 1: Earthy Dirt
                    1 => {
                        let base = 0.48 + (n - 0.5) * 0.12 + (fine - 0.5) * 0.06;
                        [base * 1.15, base * 0.85, base * 0.58]
                    }
                    // 2: Slate Rock
                    2 => {
                        let strata = tileable_noise(u * 0.5, v * 2.0, 16);
                        let s = 0.50 + (n - 0.5) * 0.10 + (strata - 0.5) * 0.08 + (fine - 0.5) * 0.05;
                        [s * 0.96, s * 0.98, s * 1.04]
                    }
                    // 3: Warm Sand
                    _ => {
                        let ripple = ((u * TAU * 4.0 + n * 2.5).sin() * 0.5 + 0.5) * 0.05;
                        let s = 0.74 + (n - 0.5) * 0.10 + ripple + (fine - 0.5) * 0.04;
                        [s * 1.04, s * 0.95, s * 0.72]
                    }
                };

                let idx = ((layer * SIZE * SIZE) + y * SIZE + x) * 4;
                pixels[idx] = (rgb[0].clamp(0.0, 1.0) * 255.0) as u8;
                pixels[idx + 1] = (rgb[1].clamp(0.0, 1.0) * 255.0) as u8;
                pixels[idx + 2] = (rgb[2].clamp(0.0, 1.0) * 255.0) as u8;
                pixels[idx + 3] = 255;
            }
        }
    }

    unsafe {
        let tex = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(tex));
        // NOTE: glow 0.16.0's web_sys `tex_image_3d` swaps `border` and `depth`.
        // Use immutable `tex_storage_3d` + `tex_sub_image_3d`, whose parameters are ordered properly.
        let mip_levels = (SIZE.ilog2() + 1) as i32;
        gl.tex_storage_3d(
            glow::TEXTURE_2D_ARRAY,
            mip_levels,
            glow::RGBA8,
            SIZE as i32,
            SIZE as i32,
            LAYERS as i32,
        );
        gl.tex_sub_image_3d(
            glow::TEXTURE_2D_ARRAY,
            0,
            0,
            0,
            0,
            SIZE as i32,
            SIZE as i32,
            LAYERS as i32,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(&pixels)),
        );
        gl.generate_mipmap(glow::TEXTURE_2D_ARRAY);
        gl.tex_parameter_i32(
            glow::TEXTURE_2D_ARRAY,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR_MIPMAP_LINEAR as i32,
        );
        gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_WRAP_S, glow::REPEAT as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_WRAP_T, glow::REPEAT as i32);
        gl.bind_texture(glow::TEXTURE_2D_ARRAY, None);
        Ok(tex)
    }
}
