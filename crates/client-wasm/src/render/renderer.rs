use glam::{Mat4, Vec3, Vec4};
use glow::HasContext;
use shared::character::{CharacterPalette, MAX_CHARACTER_JOINTS};
use shared::world::PropPlacement;

use crate::camera::Camera;

use super::character_mesh::CharacterMeshGpu;
use super::gizmo_mesh::GizmoMeshGpu;
use super::preview::PreviewTargetGpu;
use super::prop_mesh::{PropMeshGpu, create_selection_ring_mesh, prop_model_matrix};
use super::road_mesh::RoadMeshGpu;
use super::road_textures::{
    RoadLayerImage, create_placeholder_road_texture_array, create_road_texture_array_from_images,
};
use super::shaders::{
    CHARACTER_FRAG, CHARACTER_VERT, GIZMO_FRAG, GIZMO_VERT, LINE_FRAG, LINE_VERT, PROP_FRAG,
    PROP_VERT, ROAD_FRAG, ROAD_VERT, TERRAIN_FRAG, TERRAIN_VERT, compile_program,
};
use super::shadow_map::{ShadowMapGpu, compute_light_space_matrix_with_min_extent, sun_direction};
use super::studio_stage::StudioStageGpu;
use super::terrain_mesh::{BrushOverlay, TerrainMeshGpu};
use super::terrain_textures::{
    TerrainLayerImage, create_placeholder_terrain_texture_array,
    create_terrain_texture_array_from_images,
};

pub struct Renderer {
    pub gl: glow::Context,
    terrain_prog: glow::Program,
    prop_prog: glow::Program,
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    char_prog: glow::Program,
    line_prog: glow::Program,
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    road_prog: glow::Program,
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    gizmo_prog: glow::Program,
    tex_array: glow::Texture,
    white_tex: glow::Texture,
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    road_tex_array: glow::Texture,
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    road_soft_edge_mask: u16,
    ring_vao: glow::VertexArray,
    ring_vert_count: i32,
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    dyn_line_vao: glow::VertexArray,
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    dyn_line_vbo: glow::Buffer,
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    gizmo_mesh: GizmoMeshGpu,
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    studio_stage: StudioStageGpu,
    shadow_map: ShadowMapGpu,
    preview_target: Option<PreviewTargetGpu>,
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
        let char_prog = compile_program(&gl, CHARACTER_VERT, CHARACTER_FRAG)?;
        let line_prog = compile_program(&gl, LINE_VERT, LINE_FRAG)?;
        let road_prog = compile_program(&gl, ROAD_VERT, ROAD_FRAG)?;
        let gizmo_prog = compile_program(&gl, GIZMO_VERT, GIZMO_FRAG)?;
        let tex_array = create_placeholder_terrain_texture_array(&gl)?;
        let white_tex = unsafe {
            let tex = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                1,
                1,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&[255, 255, 255, 255])),
            );
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
            gl.bind_texture(glow::TEXTURE_2D, None);
            tex
        };
        let road_tex_array = create_placeholder_road_texture_array(&gl)?;
        let (ring_vao, ring_vert_count) = create_selection_ring_mesh(&gl)?;
        let (dyn_line_vao, dyn_line_vbo) = unsafe {
            let vao = gl.create_vertex_array()?;
            let vbo = gl.create_buffer()?;
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 12, 0);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            (vao, vbo)
        };
        let gizmo_mesh = GizmoMeshGpu::new(&gl)?;
        let studio_stage = StudioStageGpu::new(&gl)?;
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
            char_prog,
            line_prog,
            road_prog,
            gizmo_prog,
            tex_array,
            white_tex,
            road_tex_array,
            road_soft_edge_mask: 1u16 << shared::world::ROAD_MAT_DIRT,
            ring_vao,
            ring_vert_count,
            dyn_line_vao,
            dyn_line_vbo,
            gizmo_mesh,
            studio_stage,
            shadow_map,
            preview_target: None,
            light_view_proj: Mat4::IDENTITY,
            sun_dir: sun_direction(),
            fog_start: 140.0,
            fog_end: 240.0,
            fog_color: [0.56, 0.72, 0.86],
        })
    }

    pub fn set_terrain_textures(&mut self, images: &[TerrainLayerImage]) -> Result<(), String> {
        let next_tex = create_terrain_texture_array_from_images(&self.gl, images)?;
        unsafe {
            self.gl.delete_texture(self.tex_array);
        }
        self.tex_array = next_tex;
        Ok(())
    }

    pub fn set_road_textures(&mut self, images: &[RoadLayerImage]) -> Result<(), String> {
        let next_tex = create_road_texture_array_from_images(&self.gl, images)?;
        unsafe {
            self.gl.delete_texture(self.road_tex_array);
        }
        self.road_tex_array = next_tex;

        let mut mask = 1u16 << shared::world::ROAD_MAT_DIRT;
        for img in images {
            if (img.layer as usize) < 16 {
                if img.is_soft_edge {
                    mask |= 1u16 << img.layer;
                } else {
                    mask &= !(1u16 << img.layer);
                }
            }
        }
        self.road_soft_edge_mask = mask;
        Ok(())
    }

    /// Binds and clears the directional shadow depth map FBO for shadow-caster draws.
    pub fn begin_shadow_pass(&mut self, camera: &Camera) {
        self.begin_shadow_pass_with_min_extent(camera, 28.0);
    }

    /// Binds and clears the shadow depth map FBO with a configurable minimum orthographic half-extent.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn begin_shadow_pass_with_min_extent(&mut self, camera: &Camera, min_half_extent: f32) {
        self.light_view_proj = compute_light_space_matrix_with_min_extent(
            camera,
            self.shadow_map.size,
            min_half_extent,
        );
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
        if mesh.total_index_count <= 0 {
            return;
        }
        unsafe {
            let gl = &self.gl;
            gl.uniform_matrix_4_f32_slice(
                self.shadow_map.u_model_loc.as_ref(),
                false,
                model.as_ref(),
            );
            gl.bind_vertex_array(Some(mesh.vao));
            gl.draw_elements(
                glow::TRIANGLES,
                mesh.total_index_count,
                glow::UNSIGNED_INT,
                0,
            );
            gl.bind_vertex_array(None);
        }
    }

    /// Renders a skinned or rigid modular character mesh into the active shadow depth map.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn draw_character_shadow(
        &self,
        mesh: &CharacterMeshGpu,
        model: &Mat4,
        skin_matrices: Option<&[[f32; 16]; MAX_CHARACTER_JOINTS]>,
    ) {
        if mesh.total_index_count <= 0 {
            return;
        }
        let skinned = mesh.is_skinned && skin_matrices.is_some();
        unsafe {
            let gl = &self.gl;
            gl.use_program(Some(self.shadow_map.char_prog));
            gl.uniform_matrix_4_f32_slice(
                self.shadow_map.u_char_light_vp_loc.as_ref(),
                false,
                self.light_view_proj.as_ref(),
            );
            gl.uniform_matrix_4_f32_slice(
                self.shadow_map.u_char_model_loc.as_ref(),
                false,
                model.as_ref(),
            );
            gl.uniform_1_i32(
                self.shadow_map.u_char_is_skinned_loc.as_ref(),
                i32::from(skinned),
            );
            if skinned {
                if let Some(mats) = skin_matrices {
                    gl.uniform_matrix_4_f32_slice(
                        self.shadow_map.u_char_joints_loc.as_ref(),
                        false,
                        mats.as_flattened(),
                    );
                }
            }
            gl.bind_vertex_array(Some(mesh.vao));
            gl.draw_elements(
                glow::TRIANGLES,
                mesh.total_index_count,
                glow::UNSIGNED_INT,
                0,
            );
            gl.bind_vertex_array(None);
            gl.use_program(Some(self.shadow_map.prog));
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

    /// Renders `draw` into a transparent offscreen RGBA8 framebuffer and returns top-down RGBA bytes.
    pub fn render_to_rgba<F: FnOnce(&Renderer)>(
        &mut self,
        width: i32,
        height: i32,
        sun_dir: Vec3,
        draw: F,
    ) -> Result<Vec<u8>, String> {
        PreviewTargetGpu::ensure(&mut self.preview_target, &self.gl, width, height)?;
        let Some(target) = self.preview_target.as_ref() else {
            return Err("preview target unavailable".to_owned());
        };
        let w = target.width;
        let h = target.height;

        let saved_lvp = self.light_view_proj;
        let saved_sun = self.sun_dir;
        let saved_fog_start = self.fog_start;
        let saved_fog_end = self.fog_end;

        // Place light clip outside [0, 1]^3 so `sample_shadow` returns 1.0 without scene shadow artifacts.
        self.light_view_proj = Mat4::from_translation(Vec3::splat(10.0));
        self.sun_dir = sun_dir.normalize_or_zero();
        self.fog_start = 1000.0;
        self.fog_end = 2000.0;

        let mut raw = vec![0u8; (w as usize) * (h as usize) * 4];
        unsafe {
            let gl = &self.gl;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.fbo));
            gl.viewport(0, 0, w, h);
            gl.disable(glow::POLYGON_OFFSET_FILL);
            gl.enable(glow::DEPTH_TEST);
            gl.enable(glow::CULL_FACE);
            gl.color_mask(true, true, true, true);
            gl.depth_mask(true);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        }

        draw(self);

        unsafe {
            let gl = &self.gl;
            gl.read_pixels(
                0,
                0,
                w,
                h,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut raw)),
            );
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        }

        self.light_view_proj = saved_lvp;
        self.sun_dir = saved_sun;
        self.fog_start = saved_fog_start;
        self.fog_end = saved_fog_end;

        let stride = (w as usize) * 4;
        let mut flipped = vec![0u8; raw.len()];
        for row in 0..(h as usize) {
            let src_start = ((h as usize) - 1 - row) * stride;
            let dst_start = row * stride;
            flipped[dst_start..dst_start + stride]
                .copy_from_slice(&raw[src_start..src_start + stride]);
        }
        Ok(flipped)
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
        show_walkability: bool,
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
            gl.uniform_1_i32(
                loc("u_show_walkability").as_ref(),
                i32::from(show_walkability),
            );
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

            gl.active_texture(glow::TEXTURE4);
            gl.bind_texture(glow::TEXTURE_2D, Some(mesh.walk_tex));
            gl.uniform_1_i32(loc("u_walk_tex").as_ref(), 4);

            gl.bind_vertex_array(Some(mesh.vao));
            gl.draw_elements(glow::TRIANGLES, mesh.index_count, glow::UNSIGNED_INT, 0);
            gl.bind_vertex_array(None);
        }
    }

    /// Renders any static prop or dynamic entity mesh with embedded textures, emissive highlights, and shadows.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn draw_mesh(&self, camera: &Camera, mesh: &PropMeshGpu, model: &Mat4, tint: Vec4) {
        if mesh.submeshes.is_empty() {
            return;
        }
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

            let base_tex_loc = loc("u_base_tex");
            let emissive_loc = loc("u_emissive");
            let cutoff_loc = loc("u_alpha_cutoff");
            gl.uniform_1_i32(base_tex_loc.as_ref(), 0);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_vertex_array(Some(mesh.vao));

            let mut culling_enabled = true;
            for sub in &mesh.submeshes {
                if sub.double_sided && culling_enabled {
                    gl.disable(glow::CULL_FACE);
                    culling_enabled = false;
                } else if !sub.double_sided && !culling_enabled {
                    gl.enable(glow::CULL_FACE);
                    culling_enabled = true;
                }

                let tex = sub
                    .texture_index
                    .and_then(|idx| mesh.textures.get(idx).copied())
                    .unwrap_or(self.white_tex);
                gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                gl.uniform_3_f32(
                    emissive_loc.as_ref(),
                    sub.emissive[0],
                    sub.emissive[1],
                    sub.emissive[2],
                );
                gl.uniform_1_f32(cutoff_loc.as_ref(), sub.alpha_cutoff);
                gl.draw_elements(
                    glow::TRIANGLES,
                    sub.index_count,
                    glow::UNSIGNED_INT,
                    sub.index_offset_bytes,
                );
            }

            if !culling_enabled {
                gl.enable(glow::CULL_FACE);
            }
            gl.bind_vertex_array(None);
        }
    }

    /// Renders a skinned or rigid modular character mesh with per-material palette overrides and shadows.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn draw_character_mesh(
        &self,
        camera: &Camera,
        mesh: &CharacterMeshGpu,
        model: &Mat4,
        skin_matrices: Option<&[[f32; 16]; MAX_CHARACTER_JOINTS]>,
        palette: Option<&CharacterPalette>,
        tint: Vec4,
    ) {
        if mesh.submeshes.is_empty() {
            return;
        }
        let vp = camera.view_proj_matrix();
        let skinned = mesh.is_skinned && skin_matrices.is_some();
        unsafe {
            let gl = &self.gl;
            gl.use_program(Some(self.char_prog));
            let loc = |name| gl.get_uniform_location(self.char_prog, name);

            gl.uniform_matrix_4_f32_slice(loc("u_view_proj").as_ref(), false, vp.as_ref());
            gl.uniform_matrix_4_f32_slice(loc("u_model").as_ref(), false, model.as_ref());
            gl.uniform_1_i32(loc("u_is_skinned").as_ref(), i32::from(skinned));
            if skinned {
                if let Some(mats) = skin_matrices {
                    gl.uniform_matrix_4_f32_slice(
                        loc("u_joints").as_ref(),
                        false,
                        mats.as_flattened(),
                    );
                }
            }
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

            self.bind_shadow_uniforms(self.char_prog);

            let base_tex_loc = loc("u_base_tex");
            let base_color_loc = loc("u_base_color");
            let emissive_loc = loc("u_emissive");
            let cutoff_loc = loc("u_alpha_cutoff");
            gl.uniform_1_i32(base_tex_loc.as_ref(), 0);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_vertex_array(Some(mesh.vao));

            let mut culling_enabled = true;
            for sub in &mesh.submeshes {
                if sub.double_sided && culling_enabled {
                    gl.disable(glow::CULL_FACE);
                    culling_enabled = false;
                } else if !sub.double_sided && !culling_enabled {
                    gl.enable(glow::CULL_FACE);
                    culling_enabled = true;
                }

                let tex = sub
                    .texture_index
                    .and_then(|idx| mesh.textures.get(idx).copied())
                    .unwrap_or(self.white_tex);
                gl.bind_texture(glow::TEXTURE_2D, Some(tex));

                let base_color = palette
                    .and_then(|p| p.override_for_material(&sub.material_name))
                    .map(|rgb| [rgb[0], rgb[1], rgb[2], sub.base_color[3]])
                    .unwrap_or(sub.base_color);
                gl.uniform_4_f32(
                    base_color_loc.as_ref(),
                    base_color[0],
                    base_color[1],
                    base_color[2],
                    base_color[3],
                );
                gl.uniform_3_f32(
                    emissive_loc.as_ref(),
                    sub.emissive[0],
                    sub.emissive[1],
                    sub.emissive[2],
                );
                gl.uniform_1_f32(cutoff_loc.as_ref(), sub.alpha_cutoff);
                gl.draw_elements(
                    glow::TRIANGLES,
                    sub.index_count,
                    glow::UNSIGNED_INT,
                    sub.index_offset_bytes,
                );
            }

            if !culling_enabled {
                gl.enable(glow::CULL_FACE);
            }
            gl.bind_vertex_array(None);
        }
    }

    /// Renders the Character Studio lit pedestal floor, reference grid, and +Z forward indicator.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn draw_studio_stage(&self, camera: &Camera, draw_pedestal: bool) {
        if draw_pedestal {
            self.draw_mesh(camera, &self.studio_stage.pedestal, &Mat4::IDENTITY, Vec4::ONE);
        }
        self.draw_line_segments(
            camera,
            &self.studio_stage.grid_segments,
            [0.34, 0.39, 0.48, 0.55],
            false,
        );
        self.draw_line_segments(
            camera,
            &self.studio_stage.axis_x,
            [0.75, 0.28, 0.28, 0.72],
            false,
        );
        self.draw_line_segments(
            camera,
            &self.studio_stage.axis_z,
            [0.30, 0.62, 0.95, 0.85],
            false,
        );
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
    pub fn draw_road(
        &self,
        camera: &Camera,
        mesh: &RoadMeshGpu,
        material: u8,
        opacity: f32,
        tint: Vec3,
        brush: &BrushOverlay,
    ) {
        if mesh.index_count <= 0 {
            return;
        }
        let vp = camera.view_proj_matrix();
        let is_soft =
            (material as usize) < 16 && (self.road_soft_edge_mask & (1u16 << material)) != 0;
        let write_depth = opacity >= 0.999 && !is_soft;

        unsafe {
            let gl = &self.gl;
            gl.use_program(Some(self.road_prog));
            let loc = |name| gl.get_uniform_location(self.road_prog, name);

            gl.uniform_matrix_4_f32_slice(loc("u_view_proj").as_ref(), false, vp.as_ref());
            gl.uniform_1_i32(loc("u_material").as_ref(), material as i32);
            gl.uniform_1_f32(loc("u_opacity").as_ref(), opacity);
            gl.uniform_3_f32(loc("u_tint").as_ref(), tint.x, tint.y, tint.z);
            gl.uniform_3_f32(loc("u_camera_pos").as_ref(), camera.eye.x, camera.eye.y, camera.eye.z);
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

            self.bind_shadow_uniforms(self.road_prog);

            gl.active_texture(glow::TEXTURE3);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.road_tex_array));
            gl.uniform_1_i32(loc("u_road_tex_array").as_ref(), 3);

            gl.enable(glow::POLYGON_OFFSET_FILL);
            gl.polygon_offset(-1.0, -1.0);
            gl.depth_mask(write_depth);

            gl.bind_vertex_array(Some(mesh.vao));
            gl.draw_elements(glow::TRIANGLES, mesh.index_count, glow::UNSIGNED_INT, 0);
            gl.bind_vertex_array(None);

            gl.depth_mask(true);
            gl.disable(glow::POLYGON_OFFSET_FILL);
        }
    }

    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn draw_world_lines(
        &self,
        camera: &Camera,
        points: &[[f32; 3]],
        color: [f32; 4],
        on_top: bool,
    ) {
        self.draw_dynamic_lines(camera, points, color, on_top, glow::LINE_STRIP);
    }

    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn draw_line_segments(
        &self,
        camera: &Camera,
        points: &[[f32; 3]],
        color: [f32; 4],
        on_top: bool,
    ) {
        self.draw_dynamic_lines(camera, points, color, on_top, glow::LINES);
    }

    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    fn draw_dynamic_lines(
        &self,
        camera: &Camera,
        points: &[[f32; 3]],
        color: [f32; 4],
        on_top: bool,
        mode: u32,
    ) {
        if points.len() < 2 {
            return;
        }
        let vp = camera.view_proj_matrix();
        let floats: Vec<f32> = points.iter().flat_map(|p| *p).collect();

        unsafe {
            let gl = &self.gl;
            if on_top {
                gl.disable(glow::DEPTH_TEST);
            }
            gl.use_program(Some(self.line_prog));
            let loc = |name| gl.get_uniform_location(self.line_prog, name);
            gl.uniform_matrix_4_f32_slice(loc("u_view_proj").as_ref(), false, vp.as_ref());
            gl.uniform_matrix_4_f32_slice(loc("u_model").as_ref(), false, Mat4::IDENTITY.as_ref());
            gl.uniform_4_f32(loc("u_color").as_ref(), color[0], color[1], color[2], color[3]);

            gl.bind_vertex_array(Some(self.dyn_line_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.dyn_line_vbo));
            let bytes: &[u8] =
                std::slice::from_raw_parts(floats.as_ptr() as *const u8, floats.len() * 4);
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::DYNAMIC_DRAW);
            gl.draw_arrays(mode, 0, points.len() as i32);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            if on_top {
                gl.enable(glow::DEPTH_TEST);
            }
        }
    }

    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn draw_node_ring(
        &self,
        camera: &Camera,
        pos: Vec3,
        radius: f32,
        color: [f32; 4],
        on_top: bool,
    ) {
        let vp = camera.view_proj_matrix();
        let model = Mat4::from_translation(pos) * Mat4::from_scale(Vec3::splat(radius.max(0.25)));
        unsafe {
            let gl = &self.gl;
            if on_top {
                gl.disable(glow::DEPTH_TEST);
            }
            gl.use_program(Some(self.line_prog));
            let loc = |name| gl.get_uniform_location(self.line_prog, name);
            gl.uniform_matrix_4_f32_slice(loc("u_view_proj").as_ref(), false, vp.as_ref());
            gl.uniform_matrix_4_f32_slice(loc("u_model").as_ref(), false, model.as_ref());
            gl.uniform_4_f32(loc("u_color").as_ref(), color[0], color[1], color[2], color[3]);
            gl.bind_vertex_array(Some(self.ring_vao));
            gl.draw_arrays(glow::LINES, 0, self.ring_vert_count);
            gl.bind_vertex_array(None);
            if on_top {
                gl.enable(glow::DEPTH_TEST);
            }
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
