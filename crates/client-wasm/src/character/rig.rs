use std::collections::HashMap;

use glam::{EulerRot, Mat4, Quat, Vec3};
use shared::character::{MAX_CHARACTER_JOINTS, SocketDef};

use super::animation::AnimationClip;
use super::mesh::ParsedCharacterMesh;

#[cfg_attr(not(feature = "editor"), allow(dead_code))]
#[derive(Debug, Clone)]
pub struct JointDef {
    pub name: String,
    pub parent: Option<usize>,
    pub rest_translation: Vec3,
    pub rest_rotation: Quat,
    pub rest_scale: Vec3,
    pub inverse_bind: Mat4,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RigSocketNode {
    pub name: String,
    pub parent_bone: String,
    pub offset: [f32; 3],
    pub rotation_deg: [f32; 3],
    pub scale: f32,
}

#[cfg_attr(not(feature = "editor"), allow(dead_code))]
#[derive(Debug, Clone)]
pub struct ParsedSkeletonRig {
    pub bones: Vec<JointDef>,
    pub topo_order: Vec<usize>,
    pub bone_index_by_name: HashMap<String, usize>,
    #[allow(dead_code)]
    pub embedded_sockets: Vec<RigSocketNode>,
    pub base_mesh: Option<ParsedCharacterMesh>,
    pub animations: Vec<AnimationClip>,
}

fn build_topo_order(bones: &[JointDef]) -> Vec<usize> {
    let n = bones.len();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut roots = Vec::new();
    for (idx, b) in bones.iter().enumerate() {
        match b.parent {
            Some(p) if p < n && p != idx => children[p].push(idx),
            _ => roots.push(idx),
        }
    }
    let mut order = Vec::with_capacity(n);
    let mut visited = vec![false; n];
    let mut stack: Vec<usize> = roots.into_iter().rev().collect();
    while let Some(idx) = stack.pop() {
        if visited[idx] {
            continue;
        }
        visited[idx] = true;
        order.push(idx);
        for &child in children[idx].iter().rev() {
            if !visited[child] {
                stack.push(child);
            }
        }
    }
    for (idx, seen) in visited.into_iter().enumerate() {
        if !seen {
            order.push(idx);
        }
    }
    order
}

impl ParsedSkeletonRig {
    pub(super) fn new(
        bones: Vec<JointDef>,
        bone_index_by_name: HashMap<String, usize>,
        embedded_sockets: Vec<RigSocketNode>,
        base_mesh: Option<ParsedCharacterMesh>,
        animations: Vec<AnimationClip>,
    ) -> Self {
        let topo_order = build_topo_order(&bones);
        Self {
            bones,
            topo_order,
            bone_index_by_name,
            embedded_sockets,
            base_mesh,
            animations,
        }
    }
}

#[cfg_attr(not(feature = "editor"), allow(dead_code))]
#[derive(Debug, Clone)]
pub struct SkeletonPose {
    pub local_translations: Vec<Vec3>,
    pub local_rotations: Vec<Quat>,
    pub local_scales: Vec<Vec3>,
    pub world_matrices: Vec<Mat4>,
    pub skin_matrices: [[f32; 16]; MAX_CHARACTER_JOINTS],
}

impl Default for SkeletonPose {
    fn default() -> Self {
        Self {
            local_translations: Vec::new(),
            local_rotations: Vec::new(),
            local_scales: Vec::new(),
            world_matrices: Vec::new(),
            skin_matrices: [Mat4::IDENTITY.to_cols_array(); MAX_CHARACTER_JOINTS],
        }
    }
}

impl SkeletonPose {
    #[allow(dead_code)]
    pub fn from_rig(rig: &ParsedSkeletonRig) -> Self {
        let mut pose = Self::default();
        pose.reset_to_rest(rig);
        pose.compute_matrices(rig);
        pose
    }

    pub fn reset_to_rest(&mut self, rig: &ParsedSkeletonRig) {
        let n = rig.bones.len();
        self.local_translations.resize(n, Vec3::ZERO);
        self.local_rotations.resize(n, Quat::IDENTITY);
        self.local_scales.resize(n, Vec3::ONE);
        self.world_matrices.resize(n, Mat4::IDENTITY);
        for (i, bone) in rig.bones.iter().enumerate() {
            self.local_translations[i] = bone.rest_translation;
            self.local_rotations[i] = bone.rest_rotation;
            self.local_scales[i] = bone.rest_scale;
        }
    }

    pub fn compute_matrices(&mut self, rig: &ParsedSkeletonRig) {
        let n = rig.bones.len();
        self.world_matrices.resize(n, Mat4::IDENTITY);
        self.skin_matrices = [Mat4::IDENTITY.to_cols_array(); MAX_CHARACTER_JOINTS];

        for &idx in &rig.topo_order {
            let local = Mat4::from_scale_rotation_translation(
                self.local_scales[idx],
                self.local_rotations[idx],
                self.local_translations[idx],
            );
            let world = match rig.bones[idx].parent {
                Some(p) if p < n => self.world_matrices[p] * local,
                _ => local,
            };
            self.world_matrices[idx] = world;
            if idx < MAX_CHARACTER_JOINTS {
                self.skin_matrices[idx] = (world * rig.bones[idx].inverse_bind).to_cols_array();
            }
        }
    }

    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn socket_world_matrix(&self, rig: &ParsedSkeletonRig, sock: &SocketDef) -> Mat4 {
        let parent_world = rig
            .bone_index_by_name
            .get(&sock.parent_bone)
            .and_then(|&idx| self.world_matrices.get(idx).copied())
            .unwrap_or(Mat4::IDENTITY);
        let rot = Quat::from_euler(
            EulerRot::YXZ,
            sock.rotation_deg[1].to_radians(),
            sock.rotation_deg[0].to_radians(),
            sock.rotation_deg[2].to_radians(),
        );
        let scale = if sock.scale.abs() < 1e-4 {
            1.0
        } else {
            sock.scale
        };
        let local =
            Mat4::from_scale_rotation_translation(Vec3::splat(scale), rot, Vec3::from(sock.offset));
        parent_world * local
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use shared::character::SocketDef;

    use super::*;
    use crate::character::{evaluate_pose, parse_character_glb, parse_skeleton_glb};

    fn starter_asset(name: &str) -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("server")
            .join("assets")
            .join("characters")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("failed reading {}: {e}", path.display()))
    }

    #[test]
    fn parses_human_base_body_skeleton_sockets_and_retargeted_animations() {
        let rig = parse_skeleton_glb(&starter_asset("human_base_body.glb")).unwrap();
        assert_eq!(rig.bones.len(), 20);
        assert_eq!(rig.embedded_sockets.len(), 5);
        assert!(rig.base_mesh.is_some());
        let anim_names: Vec<&str> = rig.animations.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(anim_names, vec!["Idle", "Walk", "Run"]);

        // Bind pose skinning matrices must match identity within 1e-4
        let mut pose = SkeletonPose::from_rig(&rig);
        evaluate_pose(&rig, None, 0.0, true, &mut pose);
        for (i, mat_arr) in pose.skin_matrices[..20].iter().enumerate() {
            let m = Mat4::from_cols_array(mat_arr);
            let max_diff = m
                .to_cols_array()
                .iter()
                .zip(Mat4::IDENTITY.to_cols_array().iter())
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert!(
                max_diff < 1e-4,
                "bind-pose skin matrix {i} deviated by {max_diff}"
            );
        }

        // Socket world matrix at bind pose for socket_hand_r
        let sock = SocketDef {
            id: "socket_hand_r".to_owned(),
            name: "Right Hand".to_owned(),
            parent_bone: "hand_r".to_owned(),
            offset: [0.0, -0.06, 0.04],
            rotation_deg: [0.0, 0.0, 0.0],
            scale: 1.0,
        };
        let bind_sock_pos = pose.socket_world_matrix(&rig, &sock).w_axis.truncate();

        // Walk at t=0.25s should deform limbs and move socket_hand_r
        let walk = rig.animations.iter().find(|a| a.name == "Walk").unwrap();
        evaluate_pose(&rig, Some(walk), 0.25, true, &mut pose);
        let walk_sock_pos = pose.socket_world_matrix(&rig, &sock).w_axis.truncate();
        assert!(
            (walk_sock_pos - bind_sock_pos).length() > 0.01,
            "expected Walk animation to move hand socket (bind={bind_sock_pos:?}, walk={walk_sock_pos:?})"
        );

        let thigh_l = rig.bone_index_by_name["thigh_l"];
        let thigh_mat = Mat4::from_cols_array(&pose.skin_matrices[thigh_l]);
        let thigh_diff = thigh_mat
            .to_cols_array()
            .iter()
            .zip(Mat4::IDENTITY.to_cols_array().iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(thigh_diff > 0.05, "expected thigh_l to rotate during Walk");
    }

    #[test]
    fn parses_skinned_and_rigid_starter_parts_with_material_names() {
        let head = parse_character_glb(&starter_asset("head_aug_visor.glb")).unwrap();
        assert!(head.is_skinned);
        assert_eq!(head.joint_names.len(), 20);
        assert!(head.submeshes.iter().any(|s| s.material_name == "mat_skin"));
        assert!(
            head.submeshes
                .iter()
                .any(|s| s.material_name == "mat_emissive_fire")
        );

        let tunic = parse_character_glb(&starter_asset("torso_faith_tunic.glb")).unwrap();
        assert!(tunic.is_skinned);
        assert!(tunic.submeshes.iter().any(|s| s.material_name == "mat_primary"));
        assert!(tunic.submeshes.iter().any(|s| s.material_name == "mat_accent"));
        assert!(tunic.submeshes.iter().any(|s| s.material_name == "mat_trim"));

        let rifle = parse_character_glb(&starter_asset("weapon_guard_rifle.glb")).unwrap();
        assert!(!rifle.is_skinned);
        assert!(rifle.joint_names.is_empty());
        assert!(!rifle.vertices.is_empty());
    }

    #[test]
    fn walk_and_run_cycles_move_forward_along_plus_z() {
        let rig = parse_skeleton_glb(&starter_asset("human_base_body.glb")).unwrap();
        let foot_l_idx = rig.bone_index_by_name["foot_l"];
        let mut pose = SkeletonPose::from_rig(&rig);

        for anim_name in ["Walk", "Run"] {
            let clip = rig
                .animations
                .iter()
                .find(|a| a.name == anim_name)
                .unwrap_or_else(|| panic!("missing {anim_name} clip"));
            let dur = clip.duration;

            let mut sample_foot = |t: f32| -> Vec3 {
                evaluate_pose(&rig, Some(clip), t, true, &mut pose);
                pose.world_matrices[foot_l_idx].w_axis.truncate()
            };

            // Around t = 0.0 (stance phase: foot low on ground), foot sweeps backward (-Z) relative to body.
            let stance_before = sample_foot(0.95 * dur);
            let stance_mid = sample_foot(0.0);
            let stance_after = sample_foot(0.05 * dur);
            assert!(
                stance_after.z < stance_before.z,
                "{anim_name} stance phase must sweep -Z (before={}, after={})",
                stance_before.z,
                stance_after.z
            );

            // Around t = 0.5 * dur (swing phase: knee bent, foot lifted), foot swings forward (+Z).
            let swing_before = sample_foot(0.45 * dur);
            let swing_mid = sample_foot(0.50 * dur);
            let swing_after = sample_foot(0.55 * dur);
            assert!(
                swing_mid.y > stance_mid.y + 0.10,
                "{anim_name} swing foot must lift above stance (stance_y={}, swing_y={})",
                stance_mid.y,
                swing_mid.y
            );
            assert!(
                swing_after.z > swing_before.z,
                "{anim_name} swing phase must swing +Z (before={}, after={})",
                swing_before.z,
                swing_after.z
            );
        }
    }
}
