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

    fn world_character_asset(name: &str) -> Option<Vec<u8>> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("world")
            .join("assets")
            .join("characters")
            .join(name);
        std::fs::read(&path).ok()
    }

    fn pack_test_glb(gltf_json: serde_json::Value, mut bin: Vec<u8>) -> Vec<u8> {
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let mut json_bytes = serde_json::to_vec(&gltf_json).unwrap();
        while json_bytes.len() % 4 != 0 {
            json_bytes.push(b' ');
        }
        let total = 12 + 8 + json_bytes.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
        out.extend_from_slice(&json_bytes);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x004E_4942u32.to_le_bytes());
        out.extend_from_slice(&bin);
        out
    }

    #[test]
    fn synthetic_skeleton_glb_evaluates_bind_pose_sockets_and_animation() {
        let mut bin = Vec::<u8>::new();
        let push_f32s = |buf: &mut Vec<u8>, vals: &[f32]| {
            let offset = buf.len();
            for v in vals {
                buf.extend_from_slice(&v.to_le_bytes());
            }
            (offset, buf.len() - offset)
        };

        // 0: Triangle vertex positions (3 * VEC3)
        let (pos_off, pos_len) = push_f32s(
            &mut bin,
            &[0.0, 0.0, 0.0, 0.2, 0.0, 0.0, 0.1, 0.9, 0.0],
        );
        // 1: JOINTS_0 (3 * VEC4 u8, 12 bytes)
        let joints_off = bin.len();
        bin.extend_from_slice(&[0, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0]);
        let joints_len = 12usize;
        // 2: WEIGHTS_0 (3 * VEC4 f32)
        let (weights_off, weights_len) = push_f32s(
            &mut bin,
            &[
                1.0, 0.0, 0.0, 0.0, //
                1.0, 0.0, 0.0, 0.0, //
                1.0, 0.0, 0.0, 0.0,
            ],
        );
        // 3: INDICES (3 * u16 = 6 bytes + 2 pad)
        let idx_off = bin.len();
        for idx in [0u16, 1, 2] {
            bin.extend_from_slice(&idx.to_le_bytes());
        }
        let idx_len = 6usize;
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        // 4: Inverse bind matrices for root (0,0,0), thigh_l (0.1,0.9,0), foot_l (0.1,0.1,0)
        let ibm_root = Mat4::IDENTITY;
        let ibm_thigh = Mat4::from_translation(Vec3::new(-0.1, -0.9, 0.0));
        let ibm_foot = Mat4::from_translation(Vec3::new(-0.1, -0.1, 0.0));
        let mut ibm_floats = Vec::with_capacity(48);
        ibm_floats.extend_from_slice(&ibm_root.to_cols_array());
        ibm_floats.extend_from_slice(&ibm_thigh.to_cols_array());
        ibm_floats.extend_from_slice(&ibm_foot.to_cols_array());
        let (ibm_off, ibm_len) = push_f32s(&mut bin, &ibm_floats);

        // 5 & 6: Walk keyframe times (0.0, 0.5, 1.0) and thigh_l X rotations (-45deg forward swing at 0.5)
        let (t_off, t_len) = push_f32s(&mut bin, &[0.0, 0.5, 1.0]);
        let q0 = Quat::IDENTITY;
        let q_fwd = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_4);
        let (r_off, r_len) = push_f32s(
            &mut bin,
            &[
                q0.x, q0.y, q0.z, q0.w, //
                q_fwd.x, q_fwd.y, q_fwd.z, q_fwd.w, //
                q0.x, q0.y, q0.z, q0.w,
            ],
        );

        let doc = serde_json::json!({
            "asset": { "version": "2.0" },
            "scene": 0,
            "scenes": [{ "nodes": [0, 4] }],
            "nodes": [
                { "name": "root", "children": [1], "translation": [0.0, 0.0, 0.0] },
                { "name": "thigh_l", "children": [2], "translation": [0.1, 0.9, 0.0] },
                { "name": "foot_l", "children": [3], "translation": [0.0, -0.8, 0.0] },
                { "name": "socket_foot_l", "translation": [0.0, 0.0, 0.1] },
                { "name": "BaseBody", "mesh": 0, "skin": 0 }
            ],
            "skins": [{
                "joints": [0, 1, 2],
                "inverseBindMatrices": 4
            }],
            "materials": [{
                "name": "mat_primary",
                "pbrMetallicRoughness": { "baseColorFactor": [0.8, 0.2, 0.2, 1.0] }
            }],
            "meshes": [{
                "primitives": [{
                    "attributes": { "POSITION": 0, "JOINTS_0": 1, "WEIGHTS_0": 2 },
                    "indices": 3,
                    "material": 0
                }]
            }],
            "animations": [{
                "name": "Walk",
                "samplers": [{ "input": 5, "output": 6, "interpolation": "LINEAR" }],
                "channels": [{ "sampler": 0, "target": { "node": 1, "path": "rotation" } }]
            }],
            "buffers": [{ "byteLength": bin.len() }],
            "bufferViews": [
                { "buffer": 0, "byteOffset": pos_off, "byteLength": pos_len },
                { "buffer": 0, "byteOffset": joints_off, "byteLength": joints_len },
                { "buffer": 0, "byteOffset": weights_off, "byteLength": weights_len },
                { "buffer": 0, "byteOffset": idx_off, "byteLength": idx_len },
                { "buffer": 0, "byteOffset": ibm_off, "byteLength": ibm_len },
                { "buffer": 0, "byteOffset": t_off, "byteLength": t_len },
                { "buffer": 0, "byteOffset": r_off, "byteLength": r_len }
            ],
            "accessors": [
                { "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [0.2, 0.9, 0.0] },
                { "bufferView": 1, "componentType": 5121, "count": 3, "type": "VEC4" },
                { "bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC4" },
                { "bufferView": 3, "componentType": 5123, "count": 3, "type": "SCALAR" },
                { "bufferView": 4, "componentType": 5126, "count": 3, "type": "MAT4" },
                { "bufferView": 5, "componentType": 5126, "count": 3, "type": "SCALAR", "min": [0.0], "max": [1.0] },
                { "bufferView": 6, "componentType": 5126, "count": 3, "type": "VEC4" }
            ]
        });

        let glb = pack_test_glb(doc, bin);
        let rig = parse_skeleton_glb(&glb).unwrap();
        assert_eq!(rig.bones.len(), 3);
        assert_eq!(rig.embedded_sockets.len(), 1);
        assert_eq!(rig.embedded_sockets[0].name, "socket_foot_l");
        assert_eq!(rig.embedded_sockets[0].parent_bone, "foot_l");
        assert!(rig.base_mesh.is_some());
        assert_eq!(rig.animations.len(), 1);
        assert_eq!(rig.animations[0].name, "Walk");

        let parsed_mesh = parse_character_glb(&glb).unwrap();
        assert!(parsed_mesh.is_skinned);
        assert_eq!(parsed_mesh.joint_names, vec!["root", "thigh_l", "foot_l"]);
        assert_eq!(parsed_mesh.submeshes[0].material_name, "mat_primary");

        let mut pose = SkeletonPose::from_rig(&rig);
        evaluate_pose(&rig, None, 0.0, true, &mut pose);
        for (i, mat_arr) in pose.skin_matrices[..3].iter().enumerate() {
            let max_diff = mat_arr
                .iter()
                .zip(Mat4::IDENTITY.to_cols_array().iter())
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert!(max_diff < 1e-4, "bind skin matrix {i} deviated by {max_diff}");
        }

        let sock = SocketDef {
            id: "socket_foot_l".to_owned(),
            name: "Left Foot".to_owned(),
            parent_bone: "foot_l".to_owned(),
            offset: [0.0, 0.0, 0.1],
            rotation_deg: [0.0, 0.0, 0.0],
            scale: 1.0,
        };
        let bind_foot_z = pose.world_matrices[2].w_axis.z;
        let bind_sock_pos = pose.socket_world_matrix(&rig, &sock).w_axis.truncate();

        evaluate_pose(&rig, Some(&rig.animations[0]), 0.5, true, &mut pose);
        let swing_foot_z = pose.world_matrices[2].w_axis.z;
        let swing_sock_pos = pose.socket_world_matrix(&rig, &sock).w_axis.truncate();
        assert!(
            swing_foot_z > bind_foot_z + 0.4,
            "expected forward leg swing (+Z): bind={bind_foot_z}, swing={swing_foot_z}"
        );
        assert!((swing_sock_pos - bind_sock_pos).length() > 0.4);
    }

    #[test]
    fn parses_human_base_body_skeleton_sockets_and_retargeted_animations() {
        let Some(bytes) = world_character_asset("human_base_body.glb") else {
            return;
        };
        let rig = parse_skeleton_glb(&bytes).unwrap();
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
        let (Some(head_bytes), Some(tunic_bytes), Some(rifle_bytes)) = (
            world_character_asset("head_aug_visor.glb"),
            world_character_asset("torso_faith_tunic.glb"),
            world_character_asset("weapon_guard_rifle.glb"),
        ) else {
            return;
        };
        let head = parse_character_glb(&head_bytes).unwrap();
        assert!(head.is_skinned);
        assert_eq!(head.joint_names.len(), 20);
        assert!(head.submeshes.iter().any(|s| s.material_name == "mat_skin"));
        assert!(
            head.submeshes
                .iter()
                .any(|s| s.material_name == "mat_emissive_fire")
        );

        let tunic = parse_character_glb(&tunic_bytes).unwrap();
        assert!(tunic.is_skinned);
        assert!(tunic.submeshes.iter().any(|s| s.material_name == "mat_primary"));
        assert!(tunic.submeshes.iter().any(|s| s.material_name == "mat_accent"));
        assert!(tunic.submeshes.iter().any(|s| s.material_name == "mat_trim"));

        let rifle = parse_character_glb(&rifle_bytes).unwrap();
        assert!(!rifle.is_skinned);
        assert!(rifle.joint_names.is_empty());
        assert!(!rifle.vertices.is_empty());
    }

    #[test]
    fn walk_and_run_cycles_move_forward_along_plus_z() {
        let Some(bytes) = world_character_asset("human_base_body.glb") else {
            return;
        };
        let rig = parse_skeleton_glb(&bytes).unwrap();
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
