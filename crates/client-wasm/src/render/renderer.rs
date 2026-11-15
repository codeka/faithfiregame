use glam::{Mat4, Vec3, Vec4};
use glow::HasContext;
use shared::world::PropPlacement;

use crate::camera::Camera;

use super::gizmo_mesh::GizmoMeshGpu;
use super::procedural_textures::create_terrain_texture_array;
use super::prop_mesh::{PropMeshGpu, create_selection_ring_mesh, prop_model_matrix};
use super::shaders::{
    GIZMO_FRAG, GIZMO_VERT, LINE_FRAG, LINE_VERT, PROP_FRAG, PROP_VERT, TERRAIN_FRAG, TERRAIN_VERT,
    compile_program,
};
use super::shadow_map::{ShadowMapGpu, compute_light_space_matrix, sun_direction};
use super::terrain_mesh::{BrushOverlay, TerrainMeshGpu};

pub struct Renderer {
    pub gl: glow::Context,
    terrain_prog: glow::Program,
    prop_prog: glow::Program,
    line_prog: glow::Program,
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    gizmo_prog: glow::Program,
    tex_array: glow::Texture,
    ring_vao: glow::VertexArray,
    ring_vert_count: i32,
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    gizmo_mesh: GizmoMeshGpu,
    shadow_map: ShadowMapGpu,
    light_view_proj: Mat4,
    pub sun_dir: Vec3,
    pub fog_start: f32,
    pub fog_end: f32,
    pub fog_color: [f32; 3],
}

impl Renderer {
    pub fn new(gl: glow::Context) -> Result<Self, String> {
        let terrain_prog = compile_program(&gl, TERRAIN_VERT, TERRAIN_FRAG)?;
        let prop_prog = compile_program(&gl, PROP_VERT, PROP_FRAG)?;
        let line_prog = compile_program(&gl, LINE_VERT, LINE_FRAG)?;
        let gizmo_prog = compile_program(&gl, GIZMO_VERT, GIZMO_FRAG)?;
        let tex_array = create_terrain_texture_array(&gl)?;
        let (ring_vao, ring_vert_count) = create_selection_ring_mesh(&gl)?;
        let gizmo_mesh = GizmoMeshGpu::new(&gl)?;
        let shadow_map = ShadowMapGpu::new(&gl)?;

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
            gizmo_prog,
            tex_array,
            ring_vao,
            ring_vert_count,
            gizmo_mesh,
            shadow_map,
            light_view_proj: Mat4::IDENTITY,
            sun_dir: sun_direction(),
            fog_start: 140.0,
            fog_end: 240.0,
            fog_color: [0.56, 0.72, 0.86],
        })
    }

    /// Binds and clears the directional shadow depth map FBO for shadow-caster draws.
    pub fn begin_shadow_pass(&mut self, camera: &Camera) {
        self.light_view_proj = compute_light_space_matrix(camera, self.shadow_map.size);
        unsafe {
            let gl = &self.gl;
            // Unbind TEXTURE2 before binding the shadow FBO to avoid WebGL2 feedback loops.
            gl.active_texture(glow::TEXTURE2);
            gl.bind_texture(glow::TEXTURE_2D, None);

            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.shadow_map.fbo));
            gl.viewport(0, 0, self.shadow_map.size, self.shadow_map.size);
            gl.color_mask(false, false, false, false);
            gl.depth_mask(true);
            gl.disable(glow::CULL_FACE);
            gl.enable(glow::POLYGON_OFFSET_FILL);
            gl.polygon_offset(2.2, 4.0);
            gl.clear(glow::DEPTH_BUFFER_BIT);

            gl.use_program(Some(self.shadow_map.prog));
            gl.uniform_matrix_4_f32_slice(
                self.shadow_map.u_light_vp_loc.as_ref(),
                false,
                self.light_view_proj.as_ref(),
            );
        }
    }

    /// Renders any static prop or dynamic entity mesh into the active shadow depth map.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn draw_mesh_shadow(&self, mesh: &PropMeshGpu, model: &Mat4) {
        unsafe {
            let gl = &self.gl;
            gl.uniform_matrix_4_f32_slice(
                self.shadow_map.u_model_loc.as_ref(),
                false,
                model.as_ref(),
            );
            gl.bind_vertex_array(Some(mesh.vao));
            gl.draw_elements(glow::TRIANGLES, mesh.index_count, glow::UNSIGNED_SHORT, 0);
            gl.bind_vertex_array(None);
        }
    }

    /// Renders a placed world decoration into the active shadow depth map.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn draw_prop_shadow(&self, mesh: &PropMeshGpu, placement: &PropPlacement) {
        let model = prop_model_matrix(placement);
        self.draw_mesh_shadow(mesh, &model);
    }

    /// Renders a terrain chunk (whose vertices are already in world space) into the active shadow depth map.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn draw_terrain_shadow(&self, mesh: &TerrainMeshGpu) {
        unsafe {
            let gl = &self.gl;
            gl.uniform_matrix_4_f32_slice(
                self.shadow_map.u_model_loc.as_ref(),
                false,
                Mat4::IDENTITY.as_ref(),
            );
            gl.bind_vertex_array(Some(mesh.vao));
            gl.draw_elements(glow::TRIANGLES, mesh.index_count, glow::UNSIGNED_INT, 0);
            gl.bind_vertex_array(None);
        }
    }

    pub fn begin_frame(&self, width: i32, height: i32) {
        unsafe {
            let gl = &self.gl;
            gl.disable(glow::POLYGON_OFFSET_FILL);
            gl.enable(glow::CULL_FACE);
            gl.color_mask(true, true, true, true);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.viewport(0, 0, width.max(1), height.max(1));
            gl.clear_color(self.fog_color[0], self.fog_color[1], self.fog_color[2], 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        }
    }

    unsafe fn bind_shadow_uniforms(&self, prog: glow::Program) {
        unsafe {
            let gl = &self.gl;
            let loc = |name| gl.get_uniform_location(prog, name);
            gl.uniform_matrix_4_f32_slice(
                loc("u_light_view_proj").as_ref(),
                false,
                self.light_view_proj.as_ref(),
            );
            let sun = self.sun_dir.normalize_or_zero();
            gl.uniform_3_f32(loc("u_sun_dir").as_ref(), sun.x, sun.y, sun.z);
            gl.uniform_1_f32(
                loc("u_shadow_texel").as_ref(),
                1.0 / (self.shadow_map.size.max(1) as f32),
            );
            gl.active_texture(glow::TEXTURE2);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.shadow_map.depth_tex));
            gl.uniform_1_i32(loc("u_shadow_map").as_ref(), 2);
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
            gl.bind_texture(glow::TEXTURE_2D, Some(mesh.splat_id_tex));
            gl.uniform_1_i32(loc("u_splat_ids").as_ref(), 0);

            gl.active_texture(glow::TEXTURE1);
            gl.bind_texture(glow::TEXTURE_2D, Some(mesh.splat_weight_tex));
            gl.uniform_1_i32(loc("u_splat_weights").as_ref(), 1);

            self.bind_shadow_uniforms(self.terrain_prog);

            gl.active_texture(glow::TEXTURE3);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.tex_array));
            gl.uniform_1_i32(loc("u_tex_array").as_ref(), 3);

            gl.bind_vertex_array(Some(mesh.vao));
            gl.draw_elements(glow::TRIANGLES, mesh.index_count, glow::UNSIGNED_INT, 0);
            gl.bind_vertex_array(None);
        }
    }

    /// Renders any static prop or dynamic entity mesh with directional shadow receiving.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn draw_mesh(&self, camera: &Camera, mesh: &PropMeshGpu, model: &Mat4, tint: Vec4) {
        let vp = camera.view_proj_matrix();
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

            self.bind_shadow_uniforms(self.prop_prog);

            gl.bind_vertex_array(Some(mesh.vao));
            gl.draw_elements(glow::TRIANGLES, mesh.index_count, glow::UNSIGNED_SHORT, 0);
            gl.bind_vertex_array(None);
        }
    }

    pub fn draw_prop(&self, camera: &Camera, mesh: &PropMeshGpu, placement: &PropPlacement, tint: Vec4) {
        let model = prop_model_matrix(placement);
        self.draw_mesh(camera, mesh, &model, tint);
    }

    pub fn draw_selection_ring(&self, camera: &Camera, placement: &PropPlacement, radius: f32) {
        let vp = camera.view_proj_matrix();
        let model = Mat4::from_translation(Vec3::new(
            placement.position[0],
            placement.position[1] + 0.08,
            placement.position[2],
        )) * Mat4::from_rotation_y(placement.rotation[1])
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

    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn draw_translate_gizmo(
        &self,
        camera: &Camera,
        origin: Vec3,
        scale: f32,
        active_axis: Option<u8>,
        ground_y: Option<f32>,
    ) {
        let vp = camera.view_proj_matrix();

        unsafe {
            let gl = &self.gl;
            if let Some(gy) = ground_y {
                if (origin.y - gy).abs() > 0.03 {
                    gl.use_program(Some(self.line_prog));
                    let loc = |name| gl.get_uniform_location(self.line_prog, name);
                    gl.uniform_matrix_4_f32_slice(loc("u_view_proj").as_ref(), false, vp.as_ref());
                    gl.uniform_4_f32(loc("u_color").as_ref(), 0.85, 0.92, 1.0, 0.78);
                    gl.bind_vertex_array(Some(self.gizmo_mesh.ref_vao));

                    let line_model = Mat4::from_translation(Vec3::new(origin.x, gy, origin.z))
                        * Mat4::from_scale(Vec3::new(1.0, origin.y - gy, 1.0));
                    gl.uniform_matrix_4_f32_slice(
                        loc("u_model").as_ref(),
                        false,
                        line_model.as_ref(),
                    );
                    gl.draw_arrays(glow::LINES, 0, 2);

                    let cross_model = Mat4::from_translation(Vec3::new(origin.x, gy + 0.05, origin.z))
                        * Mat4::from_scale(Vec3::splat(0.35));
                    gl.uniform_matrix_4_f32_slice(
                        loc("u_model").as_ref(),
                        false,
                        cross_model.as_ref(),
                    );
                    gl.draw_arrays(glow::LINES, 2, 4);
                }
            }

            // Clear depth so the gizmo renders reliably on top of terrain/props while self-depth-sorting.
            gl.clear(glow::DEPTH_BUFFER_BIT);
            gl.disable(glow::CULL_FACE);

            gl.use_program(Some(self.gizmo_prog));
            let loc = |name| gl.get_uniform_location(self.gizmo_prog, name);
            gl.uniform_matrix_4_f32_slice(loc("u_view_proj").as_ref(), false, vp.as_ref());

            let hub_model = Mat4::from_translation(origin) * Mat4::from_scale(Vec3::splat(scale));
            gl.uniform_matrix_4_f32_slice(loc("u_model").as_ref(), false, hub_model.as_ref());
            gl.uniform_4_f32(loc("u_color").as_ref(), 0.92, 0.94, 0.98, 1.0);
            gl.bind_vertex_array(Some(self.gizmo_mesh.hub_vao));
            gl.draw_elements(
                glow::TRIANGLES,
                self.gizmo_mesh.hub_index_count,
                glow::UNSIGNED_SHORT,
                0,
            );

            self.draw_gizmo_axis_meshes(
                vp,
                origin,
                scale,
                active_axis,
                self.gizmo_mesh.arrow_vao,
                self.gizmo_mesh.arrow_index_count,
            );
            gl.enable(glow::CULL_FACE);
        }
    }

    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn draw_rotate_gizmo(
        &self,
        camera: &Camera,
        origin: Vec3,
        scale: f32,
        active_axis: Option<u8>,
    ) {
        let vp = camera.view_proj_matrix();
        unsafe {
            let gl = &self.gl;
            gl.clear(glow::DEPTH_BUFFER_BIT);
            gl.disable(glow::CULL_FACE);
            gl.use_program(Some(self.gizmo_prog));
            self.draw_gizmo_axis_meshes(
                vp,
                origin,
                scale,
                active_axis,
                self.gizmo_mesh.ring_vao,
                self.gizmo_mesh.ring_index_count,
            );
            gl.enable(glow::CULL_FACE);
        }
    }

    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    fn draw_gizmo_axis_meshes(
        &self,
        vp: Mat4,
        origin: Vec3,
        scale: f32,
        active_axis: Option<u8>,
        vao: glow::VertexArray,
        index_count: i32,
    ) {
        let highlight = [1.0f32, 0.90, 0.28, 1.0];
        let axes: [(u8, Mat4, [f32; 4]); 3] = [
            (0, Mat4::from_rotation_z(-std::f32::consts::FRAC_PI_2), [0.95, 0.26, 0.26, 1.0]),
            (1, Mat4::IDENTITY, [0.28, 0.86, 0.36, 1.0]),
            (2, Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2), [0.28, 0.56, 0.98, 1.0]),
        ];
        unsafe {
            let gl = &self.gl;
            let loc = |name| gl.get_uniform_location(self.gizmo_prog, name);
            gl.uniform_matrix_4_f32_slice(loc("u_view_proj").as_ref(), false, vp.as_ref());
            gl.bind_vertex_array(Some(vao));
            let translate_origin = Mat4::from_translation(origin);
            let scale_mat = Mat4::from_scale(Vec3::splat(scale));
            for (axis_idx, rot, base_color) in axes {
                let color = if active_axis == Some(axis_idx) {
                    highlight
                } else {
                    base_color
                };
                let model = translate_origin * rot * scale_mat;
                gl.uniform_matrix_4_f32_slice(loc("u_model").as_ref(), false, model.as_ref());
                gl.uniform_4_f32(loc("u_color").as_ref(), color[0], color[1], color[2], color[3]);
                gl.draw_elements(glow::TRIANGLES, index_count, glow::UNSIGNED_SHORT, 0);
            }
            gl.bind_vertex_array(None);
        }
    }
}
