use std::collections::HashMap;

use glam::{Mat4, Vec3, Vec4};
use shared::character::CharacterDefinition;
use shared::entity::{EntityTemplate, VisualConfig};

use crate::camera::Camera;
use crate::character::SkeletonPose;
use crate::render::{PropMeshGpu, Renderer};

use super::character_studio::CharacterStudioState;

pub struct EntityStudioState {
    pub active_template: Option<EntityTemplate>,
    pub anim_time: f32,
    pub pose: SkeletonPose,
    pub turntable: bool,
}

impl EntityStudioState {
    pub fn new() -> Self {
        Self {
            active_template: None,
            anim_time: 0.0,
            pose: SkeletonPose::default(),
            turntable: false,
        }
    }

    pub fn set_active_template(&mut self, tpl: Option<EntityTemplate>) {
        let changed = self
            .active_template
            .as_ref()
            .map(|t| &t.id)
            != tpl.as_ref().map(|t| &t.id);
        self.active_template = tpl;
        if changed {
            self.anim_time = 0.0;
        }
    }

    pub fn update(
        &mut self,
        dt: f32,
        character_presets: &HashMap<String, CharacterDefinition>,
        char_studio: &CharacterStudioState,
    ) {
        self.anim_time += dt;
        let Some(tpl) = &self.active_template else {
            return;
        };
        if let Some(VisualConfig::Character {
            preset_id,
            pose_clip,
        }) = &tpl.visual
        {
            if let Some(def) = character_presets.get(preset_id) {
                char_studio.evaluate_pose_or_idle(
                    def,
                    pose_clip.as_deref(),
                    self.anim_time,
                    &mut self.pose,
                );
            }
        }
    }

    pub fn render_shadows(
        &self,
        renderer: &mut Renderer,
        camera: &Camera,
        character_presets: &HashMap<String, CharacterDefinition>,
        char_studio: &CharacterStudioState,
        prop_meshes: &HashMap<String, PropMeshGpu>,
    ) {
        renderer.begin_shadow_pass_with_min_extent(camera, 8.0);
        let Some(tpl) = &self.active_template else {
            return;
        };
        match &tpl.visual {
            Some(VisualConfig::Character { preset_id, .. }) => {
                if let Some(def) = character_presets.get(preset_id) {
                    char_studio.draw_definition_shadow(renderer, def, &Mat4::IDENTITY, &self.pose);
                }
            }
            Some(VisualConfig::Prop {
                asset_id,
                scale,
                cast_shadow,
            }) => {
                if *cast_shadow {
                    if let Some(mesh) = prop_meshes.get(asset_id) {
                        let model = Mat4::from_scale(Vec3::from(*scale));
                        renderer.draw_mesh_shadow(mesh, &model);
                    }
                }
            }
            None => {}
        }
    }

    pub fn render(
        &self,
        renderer: &Renderer,
        camera: &Camera,
        character_presets: &HashMap<String, CharacterDefinition>,
        char_studio: &CharacterStudioState,
        prop_meshes: &HashMap<String, PropMeshGpu>,
    ) {
        renderer.draw_studio_stage(camera, true);
        let Some(tpl) = &self.active_template else {
            return;
        };

        match &tpl.visual {
            Some(VisualConfig::Character { preset_id, .. }) => {
                if let Some(def) = character_presets.get(preset_id) {
                    char_studio.draw_definition(
                        renderer,
                        camera,
                        def,
                        &Mat4::IDENTITY,
                        &self.pose,
                        Vec4::ONE,
                    );
                }
            }
            Some(VisualConfig::Prop {
                asset_id, scale, ..
            }) => {
                if let Some(mesh) = prop_meshes.get(asset_id) {
                    let model = Mat4::from_scale(Vec3::from(*scale));
                    renderer.draw_mesh(camera, mesh, &model, Vec4::ONE);
                }
            }
            None => {}
        }

        if let Some(trig) = &tpl.trigger {
            let segs = trig.wireframe_segments([0.0, 0.0, 0.0], 0.0);
            renderer.draw_line_segments(camera, &segs, [0.25, 0.92, 1.0, 0.95], true);
        } else if tpl.visual.is_none() {
            renderer.draw_node_ring(
                camera,
                Vec3::Y * 0.05,
                0.65,
                [0.45, 0.78, 1.0, 0.85],
                false,
            );
        }
    }
}
