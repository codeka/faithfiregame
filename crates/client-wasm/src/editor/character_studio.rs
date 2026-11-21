use std::collections::{HashMap, HashSet};

use glam::{EulerRot, Mat4, Quat, Vec3, Vec4};
use shared::character::{
    CharacterCatalog, CharacterDefinition, CharacterPartDef, PartAttachment, SkeletonDef,
};

use crate::camera::Camera;
use crate::character::{
    AnimationClip, ParsedSkeletonRig, SkeletonPose, evaluate_pose, parse_character_glb,
    parse_glb_animations, parse_skeleton_glb,
};
use crate::input::InputState;
use crate::render::{CharacterMeshGpu, Renderer};

use super::types::CharacterStudioStatusSnapshot;

#[derive(Debug, Clone)]
pub struct StudioCameraController {
    pub target: Vec3,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub distance: f32,
}

impl Default for StudioCameraController {
    fn default() -> Self {
        Self {
            target: Vec3::new(0.0, 0.92, 0.0),
            yaw_deg: 68.0,
            pitch_deg: 14.0,
            distance: 3.2,
        }
    }
}

impl StudioCameraController {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn update(&mut self, camera: &mut Camera, input: &InputState, dt: f32, turntable: bool) {
        let shift = input.is_key_down("ShiftLeft") || input.is_key_down("ShiftRight");
        let drag = input.left_mouse_down || input.middle_mouse_down;

        if drag {
            if input.left_mouse_down && shift {
                self.target.y =
                    (self.target.y + input.mouse_delta.1 * 0.004 * self.distance).clamp(0.15, 1.90);
            } else {
                self.yaw_deg = (self.yaw_deg + input.mouse_delta.0 * 0.45).rem_euclid(360.0);
                self.pitch_deg = (self.pitch_deg + input.mouse_delta.1 * 0.35).clamp(-5.0, 75.0);
            }
        } else if turntable {
            self.yaw_deg = (self.yaw_deg + 24.0 * dt).rem_euclid(360.0);
        }

        if input.scroll_delta.abs() > 0.0 {
            let factor = (1.0 + input.scroll_delta * 0.12).clamp(0.7, 1.4);
            self.distance = (self.distance * factor).clamp(0.8, 9.0);
        }

        if input.is_key_down("ArrowUp") || input.is_key_down("KeyW") {
            self.target.y = (self.target.y + 1.1 * dt).clamp(0.15, 1.90);
        }
        if input.is_key_down("ArrowDown") || input.is_key_down("KeyS") {
            self.target.y = (self.target.y - 1.1 * dt).clamp(0.15, 1.90);
        }
        if input.is_key_down("ArrowLeft") || input.is_key_down("KeyA") {
            self.yaw_deg = (self.yaw_deg - 75.0 * dt).rem_euclid(360.0);
        }
        if input.is_key_down("ArrowRight") || input.is_key_down("KeyD") {
            self.yaw_deg = (self.yaw_deg + 75.0 * dt).rem_euclid(360.0);
        }
        if input.is_key_just_pressed("KeyF") {
            self.reset();
        }

        let yaw = self.yaw_deg.to_radians();
        let pitch = self.pitch_deg.to_radians();
        let offset = Vec3::new(
            self.distance * pitch.cos() * yaw.cos(),
            self.distance * pitch.sin(),
            self.distance * pitch.cos() * yaw.sin(),
        );
        camera.target = self.target;
        camera.eye = self.target + offset;
    }
}

pub struct CharacterStudioState {
    pub catalog: CharacterCatalog,
    pub active_character: CharacterDefinition,
    pub rigs: HashMap<String, ParsedSkeletonRig>,
    pub base_meshes: HashMap<String, CharacterMeshGpu>,
    pub part_meshes: HashMap<String, CharacterMeshGpu>,
    pub extra_anims: HashMap<String, Vec<AnimationClip>>,
    pub requested_urls: HashSet<String>,
    pub pending_glb_fetches: Vec<String>,
    pub pose: SkeletonPose,

    pub active_clip: Option<String>,
    pub user_selected_bind_pose: bool,
    pub playing: bool,
    pub anim_time: f32,
    pub playback_speed: f32,
    pub show_skeleton_debug: bool,
    pub turntable: bool,
    pub selected_socket_id: Option<String>,
    pub save_state: String,
}

impl CharacterStudioState {
    pub fn new() -> Self {
        Self {
            catalog: CharacterCatalog::default(),
            active_character: CharacterDefinition::default(),
            rigs: HashMap::new(),
            base_meshes: HashMap::new(),
            part_meshes: HashMap::new(),
            extra_anims: HashMap::new(),
            requested_urls: HashSet::new(),
            pending_glb_fetches: Vec::new(),
            pose: SkeletonPose::default(),
            active_clip: None,
            user_selected_bind_pose: false,
            playing: true,
            anim_time: 0.0,
            playback_speed: 1.0,
            show_skeleton_debug: false,
            turntable: false,
            selected_socket_id: None,
            save_state: "Saved".to_owned(),
        }
    }

    pub fn set_catalog(&mut self, catalog: CharacterCatalog) {
        self.catalog = catalog;
        if self.active_character.skeleton_id.is_empty() {
            if let Some(first) = self.catalog.skeletons.first() {
                self.active_character.skeleton_id = first.id.clone();
            }
        }
        self.queue_needed_glbs();
        self.sync_active_clip_choice();
    }

    pub fn set_active_character(&mut self, def: CharacterDefinition) {
        let skeleton_changed = self.active_character.skeleton_id != def.skeleton_id;
        self.active_character = def;
        if skeleton_changed {
            self.anim_time = 0.0;
        }
        self.queue_needed_glbs();
        self.sync_active_clip_choice();
    }

    fn skeleton_for(&self, skeleton_id: &str) -> Option<&SkeletonDef> {
        self.catalog
            .skeletons
            .iter()
            .find(|s| s.id == skeleton_id)
            .or_else(|| self.catalog.skeletons.first())
    }

    fn active_skeleton_def(&self) -> Option<&SkeletonDef> {
        self.skeleton_for(&self.active_character.skeleton_id)
    }

    fn find_part_def(&self, part_id: &str) -> Option<&CharacterPartDef> {
        self.catalog.parts.iter().find(|p| p.id == part_id)
    }

    fn request_glb_if_missing(&mut self, url: &str, already_loaded: bool) {
        if url.is_empty() || already_loaded {
            return;
        }
        if self.requested_urls.insert(url.to_owned()) {
            self.pending_glb_fetches.push(url.to_owned());
        }
    }

    pub fn queue_needed_glbs(&mut self) {
        let def = self.active_character.clone();
        self.queue_glbs_for_definition(&def);
    }

    pub fn queue_glbs_for_definition(&mut self, def: &CharacterDefinition) {
        let Some(skel) = self.skeleton_for(&def.skeleton_id).cloned() else {
            return;
        };
        let rig_loaded = self.rigs.contains_key(&skel.rig_path);
        self.request_glb_if_missing(&skel.rig_path, rig_loaded);
        for anim_url in &skel.animation_paths {
            let loaded = self.extra_anims.contains_key(anim_url);
            self.request_glb_if_missing(anim_url, loaded);
        }

        let part_ids: Vec<String> = def
            .skinned_parts
            .values()
            .chain(def.socket_parts.values())
            .cloned()
            .collect();
        for part_id in part_ids {
            if let Some(part) = self.find_part_def(&part_id).cloned() {
                let loaded = self.part_meshes.contains_key(&part.path);
                self.request_glb_if_missing(&part.path, loaded);
            }
        }
    }

    pub fn on_glb_loaded(
        &mut self,
        gl: &glow::Context,
        path: String,
        bytes: &[u8],
    ) -> Result<(), String> {
        self.requested_urls.insert(path.clone());
        let is_rig = self.catalog.skeletons.iter().any(|s| s.rig_path == path);
        let is_anim = self
            .catalog
            .skeletons
            .iter()
            .any(|s| s.animation_paths.iter().any(|a| a == &path));
        let is_part = self.catalog.parts.iter().any(|p| p.path == path);

        if is_rig {
            let rig = parse_skeleton_glb(bytes)?;
            if let Some(ref parsed_base) = rig.base_mesh {
                let gpu = CharacterMeshGpu::from_parsed(gl, parsed_base)?;
                if let Some(old) = self.base_meshes.insert(path.clone(), gpu) {
                    old.destroy(gl);
                }
            }
            self.rigs.insert(path.clone(), rig);
        }

        if is_anim {
            let clips = parse_glb_animations(bytes)?;
            self.extra_anims.insert(path.clone(), clips);
        }

        if is_part || (!is_rig && !is_anim) {
            if let Ok(parsed) = parse_character_glb(bytes) {
                if !parsed.vertices.is_empty() && !parsed.indices.is_empty() {
                    let gpu = CharacterMeshGpu::from_parsed(gl, &parsed)?;
                    if let Some(old) = self.part_meshes.insert(path.clone(), gpu) {
                        old.destroy(gl);
                    }
                }
            }
            if !is_rig {
                if let Ok(rig) = parse_skeleton_glb(bytes) {
                    if let Some(ref parsed_base) = rig.base_mesh {
                        let gpu = CharacterMeshGpu::from_parsed(gl, parsed_base)?;
                        if let Some(old) = self.base_meshes.insert(path.clone(), gpu) {
                            old.destroy(gl);
                        }
                    }
                    self.rigs.insert(path.clone(), rig);
                }
            }
            if !is_anim {
                if let Ok(clips) = parse_glb_animations(bytes) {
                    if !clips.is_empty() {
                        self.extra_anims.insert(path, clips);
                    }
                }
            }
        }

        self.sync_active_clip_choice();
        Ok(())
    }

    fn available_clips(&self) -> Vec<AnimationClip> {
        let Some(skel) = self.active_skeleton_def() else {
            return Vec::new();
        };
        let mut clips = Vec::new();
        let mut idx_by_name: HashMap<String, usize> = HashMap::new();
        if let Some(rig) = self.rigs.get(&skel.rig_path) {
            for clip in &rig.animations {
                idx_by_name.insert(clip.name.clone(), clips.len());
                clips.push(clip.clone());
            }
        }
        for anim_url in &skel.animation_paths {
            if let Some(extra) = self.extra_anims.get(anim_url) {
                for clip in extra {
                    if let Some(&pos) = idx_by_name.get(&clip.name) {
                        clips[pos] = clip.clone();
                    } else {
                        idx_by_name.insert(clip.name.clone(), clips.len());
                        clips.push(clip.clone());
                    }
                }
            }
        }
        clips
    }

    fn sync_active_clip_choice(&mut self) {
        if self.user_selected_bind_pose {
            self.active_clip = None;
            return;
        }
        let clips = self.available_clips();
        if clips.is_empty() {
            self.active_clip = None;
            return;
        }
        if let Some(ref current) = self.active_clip {
            if clips.iter().any(|c| c.name == *current) {
                return;
            }
        }
        let preferred = clips
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case("Idle"))
            .or_else(|| clips.first())
            .map(|c| c.name.clone());
        self.active_clip = preferred;
        self.anim_time = 0.0;
    }

    pub fn select_clip(&mut self, clip: &str) {
        if clip.is_empty() || clip == "__bind__" {
            self.user_selected_bind_pose = true;
            self.active_clip = None;
            self.anim_time = 0.0;
        } else {
            self.user_selected_bind_pose = false;
            self.active_clip = Some(clip.to_owned());
            self.anim_time = 0.0;
        }
    }

    pub fn update(&mut self, input: &InputState, dt: f32) {
        if input.is_key_just_pressed("Space") {
            self.playing = !self.playing;
        }
        let Some(skel) = self.active_skeleton_def() else {
            return;
        };
        let Some(rig) = self.rigs.get(&skel.rig_path) else {
            return;
        };
        let clips = self.available_clips();
        let selected = self
            .active_clip
            .as_deref()
            .and_then(|name| clips.iter().find(|c| c.name == name));
        if let Some(clip) = selected {
            if self.playing && clip.duration > 1e-6 {
                self.anim_time =
                    (self.anim_time + dt * self.playback_speed).rem_euclid(clip.duration);
            }
        }
        evaluate_pose(rig, selected, self.anim_time, true, &mut self.pose);
    }

    fn part_local_matrix(part: &CharacterPartDef) -> Mat4 {
        match &part.attachment {
            PartAttachment::Skinned { .. } => Mat4::IDENTITY,
            PartAttachment::Socket {
                offset,
                rotation_deg,
                scale,
                ..
            } => {
                let rot = Quat::from_euler(
                    EulerRot::YXZ,
                    rotation_deg[1].to_radians(),
                    rotation_deg[0].to_radians(),
                    rotation_deg[2].to_radians(),
                );
                let s = if scale.abs() < 1e-4 { 1.0 } else { *scale };
                Mat4::from_scale_rotation_translation(Vec3::splat(s), rot, Vec3::from(*offset))
            }
        }
    }

    pub fn evaluate_idle_pose(
        &self,
        def: &CharacterDefinition,
        time_s: f32,
        out_pose: &mut SkeletonPose,
    ) -> bool {
        let Some(skel) = self.skeleton_for(&def.skeleton_id) else {
            return false;
        };
        let Some(rig) = self.rigs.get(&skel.rig_path) else {
            return false;
        };
        let mut idle_clip: Option<&AnimationClip> = None;
        for anim_url in &skel.animation_paths {
            if let Some(clips) = self.extra_anims.get(anim_url) {
                if let Some(found) = clips.iter().find(|c| c.name.eq_ignore_ascii_case("Idle")) {
                    idle_clip = Some(found);
                    break;
                }
            }
        }
        if idle_clip.is_none() {
            idle_clip = rig
                .animations
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case("Idle"))
                .or_else(|| rig.animations.first())
                .or_else(|| {
                    skel.animation_paths
                        .iter()
                        .filter_map(|u| self.extra_anims.get(u))
                        .flatten()
                        .next()
                });
        }
        let t = idle_clip
            .filter(|c| c.duration > 1e-6)
            .map(|c| time_s.rem_euclid(c.duration))
            .unwrap_or(0.0);
        evaluate_pose(rig, idle_clip, t, true, out_pose);
        true
    }

    pub fn draw_definition_shadow(
        &self,
        renderer: &Renderer,
        def: &CharacterDefinition,
        model: &Mat4,
        pose: &SkeletonPose,
    ) {
        let Some(skel) = self.skeleton_for(&def.skeleton_id) else {
            return;
        };
        let Some(rig) = self.rigs.get(&skel.rig_path) else {
            return;
        };
        if pose.world_matrices.len() != rig.bones.len() {
            return;
        }

        if skel.render_base_mesh && def.show_base_mesh {
            if let Some(base_gpu) = self.base_meshes.get(&skel.rig_path) {
                let palette = base_gpu.remapped_skin_palette(rig, pose);
                renderer.draw_character_shadow(base_gpu, model, Some(&palette));
            }
        }

        for part_id in def.skinned_parts.values() {
            if let Some(part) = self.find_part_def(part_id) {
                if let Some(mesh) = self.part_meshes.get(&part.path) {
                    let palette = mesh.is_skinned.then(|| mesh.remapped_skin_palette(rig, pose));
                    renderer.draw_character_shadow(mesh, model, palette.as_ref());
                }
            }
        }

        for (socket_id, part_id) in &def.socket_parts {
            let Some(sock) = skel.sockets.iter().find(|s| s.id == *socket_id) else {
                continue;
            };
            let Some(part) = self.find_part_def(part_id) else {
                continue;
            };
            let Some(mesh) = self.part_meshes.get(&part.path) else {
                continue;
            };
            let world = *model * pose.socket_world_matrix(rig, sock) * Self::part_local_matrix(part);
            renderer.draw_character_shadow(mesh, &world, None);
        }
    }

    pub fn draw_definition(
        &self,
        renderer: &Renderer,
        camera: &Camera,
        def: &CharacterDefinition,
        model: &Mat4,
        pose: &SkeletonPose,
        tint: Vec4,
    ) {
        let Some(skel) = self.skeleton_for(&def.skeleton_id) else {
            return;
        };
        let Some(rig) = self.rigs.get(&skel.rig_path) else {
            return;
        };
        if pose.world_matrices.len() != rig.bones.len() {
            return;
        }
        let char_palette = &def.palette;

        if skel.render_base_mesh && def.show_base_mesh {
            if let Some(base_gpu) = self.base_meshes.get(&skel.rig_path) {
                let skin_mats = base_gpu.remapped_skin_palette(rig, pose);
                renderer.draw_character_mesh(
                    camera,
                    base_gpu,
                    model,
                    Some(&skin_mats),
                    Some(char_palette),
                    tint,
                );
            }
        }

        for part_id in def.skinned_parts.values() {
            if let Some(part) = self.find_part_def(part_id) {
                if let Some(mesh) = self.part_meshes.get(&part.path) {
                    let skin_mats = mesh
                        .is_skinned
                        .then(|| mesh.remapped_skin_palette(rig, pose));
                    renderer.draw_character_mesh(
                        camera,
                        mesh,
                        model,
                        skin_mats.as_ref(),
                        Some(char_palette),
                        tint,
                    );
                }
            }
        }

        for (socket_id, part_id) in &def.socket_parts {
            let Some(sock) = skel.sockets.iter().find(|s| s.id == *socket_id) else {
                continue;
            };
            let Some(part) = self.find_part_def(part_id) else {
                continue;
            };
            let Some(mesh) = self.part_meshes.get(&part.path) else {
                continue;
            };
            let world = *model * pose.socket_world_matrix(rig, sock) * Self::part_local_matrix(part);
            renderer.draw_character_mesh(
                camera,
                mesh,
                &world,
                None,
                Some(char_palette),
                tint,
            );
        }
    }

    pub fn render_shadows(&self, renderer: &mut Renderer, camera: &Camera) {
        renderer.begin_shadow_pass_with_min_extent(camera, 4.0);
        self.draw_definition_shadow(renderer, &self.active_character, &Mat4::IDENTITY, &self.pose);
    }

    pub fn render(&self, renderer: &Renderer, camera: &Camera) {
        renderer.draw_studio_stage(camera, true);
        self.draw_definition(
            renderer,
            camera,
            &self.active_character,
            &Mat4::IDENTITY,
            &self.pose,
            Vec4::ONE,
        );

        if self.show_skeleton_debug {
            if let (Some(skel), Some(rig)) = (
                self.active_skeleton_def(),
                self.active_skeleton_def()
                    .and_then(|s| self.rigs.get(&s.rig_path)),
            ) {
                self.render_skeleton_debug(renderer, camera, skel, rig);
            }
        }
    }

    fn render_skeleton_debug(
        &self,
        renderer: &Renderer,
        camera: &Camera,
        skel: &SkeletonDef,
        rig: &ParsedSkeletonRig,
    ) {
        let mut bone_lines = Vec::new();
        let mut joint_crosses = Vec::new();
        let tick = 0.024f32;

        for (idx, bone) in rig.bones.iter().enumerate() {
            let Some(&world) = self.pose.world_matrices.get(idx) else {
                continue;
            };
            let p = world.w_axis.truncate();
            if let Some(parent_idx) = bone.parent {
                if let Some(&pw) = self.pose.world_matrices.get(parent_idx) {
                    let parent_pos = pw.w_axis.truncate();
                    bone_lines.push(parent_pos.to_array());
                    bone_lines.push(p.to_array());
                }
            }
            joint_crosses.extend_from_slice(&[
                [p.x - tick, p.y, p.z],
                [p.x + tick, p.y, p.z],
                [p.x, p.y - tick, p.z],
                [p.x, p.y + tick, p.z],
                [p.x, p.y, p.z - tick],
                [p.x, p.y, p.z + tick],
            ]);
        }

        renderer.draw_line_segments(camera, &bone_lines, [0.25, 0.90, 1.0, 0.95], true);
        renderer.draw_line_segments(camera, &joint_crosses, [1.0, 0.86, 0.25, 1.0], true);

        let mut x_segs = Vec::new();
        let mut y_segs = Vec::new();
        let mut z_segs = Vec::new();
        for sock in &skel.sockets {
            let mat = self.pose.socket_world_matrix(rig, sock);
            let origin = mat.w_axis.truncate();
            let selected = self.selected_socket_id.as_deref() == Some(sock.id.as_str());
            let len = if selected { 0.18 } else { 0.10 };
            let ax = mat.x_axis.truncate().normalize_or_zero() * len;
            let ay = mat.y_axis.truncate().normalize_or_zero() * len;
            let az = mat.z_axis.truncate().normalize_or_zero() * len;

            x_segs.push(origin.to_array());
            x_segs.push((origin + ax).to_array());
            y_segs.push(origin.to_array());
            y_segs.push((origin + ay).to_array());
            z_segs.push(origin.to_array());
            z_segs.push((origin + az).to_array());

            if selected {
                renderer.draw_node_ring(camera, origin, 0.14, [1.0, 0.92, 0.28, 1.0], true);
            }
        }
        renderer.draw_line_segments(camera, &x_segs, [0.98, 0.30, 0.30, 1.0], true);
        renderer.draw_line_segments(camera, &y_segs, [0.32, 0.92, 0.42, 1.0], true);
        renderer.draw_line_segments(camera, &z_segs, [0.32, 0.62, 1.00, 1.0], true);
    }

    pub fn status_snapshot(&self) -> CharacterStudioStatusSnapshot {
        let clips = self.available_clips();
        let clip_duration = self
            .active_clip
            .as_deref()
            .and_then(|name| clips.iter().find(|c| c.name == name))
            .map(|c| c.duration)
            .unwrap_or(0.0);
        let bone_names = self
            .active_skeleton_def()
            .and_then(|s| self.rigs.get(&s.rig_path))
            .map(|rig| rig.bones.iter().map(|b| b.name.clone()).collect())
            .unwrap_or_default();

        CharacterStudioStatusSnapshot {
            active_preset_id: self.active_character.id.clone(),
            active_skeleton_id: self.active_character.skeleton_id.clone(),
            available_clips: clips.into_iter().map(|c| c.name).collect(),
            active_clip: self.active_clip.clone(),
            playing: self.playing,
            anim_time: self.anim_time,
            clip_duration,
            playback_speed: self.playback_speed,
            show_skeleton_debug: self.show_skeleton_debug,
            turntable: self.turntable,
            selected_socket_id: self.selected_socket_id.clone(),
            bone_names,
            save_state: self.save_state.clone(),
        }
    }
}
