use glam::{Quat, Vec3, Vec4};
use gltf::Gltf;
use gltf::animation::util::ReadOutputs;

use super::rig::ParsedSkeletonRig;
use super::rig::SkeletonPose;

#[cfg_attr(not(feature = "editor"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    Step,
    Linear,
    CubicSpline,
}

#[cfg_attr(not(feature = "editor"), allow(dead_code))]
#[derive(Debug, Clone)]
pub enum AnimTrack {
    Translations(Vec<Vec3>),
    Rotations(Vec<Quat>),
    Scales(Vec<Vec3>),
}

#[cfg_attr(not(feature = "editor"), allow(dead_code))]
#[derive(Debug, Clone)]
pub struct AnimChannel {
    pub target_bone: String,
    pub times: Vec<f32>,
    pub interpolation: Interpolation,
    pub track: AnimTrack,
}

#[cfg_attr(not(feature = "editor"), allow(dead_code))]
#[derive(Debug, Clone)]
pub struct AnimationClip {
    pub name: String,
    pub duration: f32,
    pub channels: Vec<AnimChannel>,
}

/// Parses animation clips from an already-parsed `Gltf` document and its binary chunk.
pub(super) fn extract_gltf_animations(gltf: &Gltf) -> Vec<AnimationClip> {
    let blob = gltf.blob.as_deref();
    let mut clips = Vec::new();
    for (clip_idx, anim) in gltf.animations().enumerate() {
        let mut channels = Vec::new();
        let mut duration = 0.0f32;
        for ch in anim.channels() {
            let target_node = ch.target().node();
            let target_bone = target_node
                .name()
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("node_{}", target_node.index()));

            let reader = ch.reader(|buffer| if buffer.index() == 0 { blob } else { None });
            let Some(times_iter) = reader.read_inputs() else {
                continue;
            };
            let times: Vec<f32> = times_iter.collect();
            if times.is_empty() {
                continue;
            }

            let Some(outputs) = reader.read_outputs() else {
                continue;
            };
            let track = match outputs {
                ReadOutputs::Translations(iter) => AnimTrack::Translations(iter.map(Vec3::from).collect()),
                ReadOutputs::Rotations(rots) => AnimTrack::Rotations(
                    rots.into_f32()
                        .map(|q| Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize())
                        .collect(),
                ),
                ReadOutputs::Scales(iter) => AnimTrack::Scales(iter.map(Vec3::from).collect()),
                ReadOutputs::MorphTargetWeights(_) => continue,
            };

            let interpolation = match ch.sampler().interpolation() {
                gltf::animation::Interpolation::Step => Interpolation::Step,
                gltf::animation::Interpolation::Linear => Interpolation::Linear,
                gltf::animation::Interpolation::CubicSpline => Interpolation::CubicSpline,
            };

            if let Some(&last_t) = times.last() {
                duration = duration.max(last_t);
            }
            channels.push(AnimChannel {
                target_bone,
                times,
                interpolation,
                track,
            });
        }

        let name = anim
            .name()
            .filter(|s| !s.trim().is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("Clip {}", clip_idx + 1));
        clips.push(AnimationClip {
            name,
            duration,
            channels,
        });
    }
    clips
}

/// Parses standalone animation clips from a binary `.glb` payload.
#[cfg_attr(not(feature = "editor"), allow(dead_code))]
pub fn parse_glb_animations(bytes: &[u8]) -> Result<Vec<AnimationClip>, String> {
    let gltf = Gltf::from_slice(bytes).map_err(|e| format!("gltf parse failed: {e}"))?;
    Ok(extract_gltf_animations(&gltf))
}

fn find_keyframe_interval(times: &[f32], t: f32) -> (usize, usize, f32, f32) {
    let n = times.len();
    if n <= 1 || t <= times[0] {
        return (0, 0, 0.0, 0.0);
    }
    let last = n - 1;
    if t >= times[last] {
        return (last, last, 0.0, 0.0);
    }
    let idx = times.partition_point(|&kt| kt <= t);
    let i1 = idx.clamp(1, last);
    let i0 = i1 - 1;
    let dt = times[i1] - times[i0];
    let alpha = if dt > 1e-6 {
        ((t - times[i0]) / dt).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (i0, i1, alpha, dt)
}

fn sample_vec3(values: &[Vec3], times: &[f32], interp: Interpolation, t: f32) -> Option<Vec3> {
    if values.is_empty() || times.is_empty() {
        return None;
    }
    let (i0, i1, alpha, dt) = find_keyframe_interval(times, t);
    match interp {
        Interpolation::Step => values.get(i0).copied(),
        Interpolation::Linear => {
            let v0 = *values.get(i0)?;
            let v1 = *values.get(i1).unwrap_or(&v0);
            Some(v0.lerp(v1, alpha))
        }
        Interpolation::CubicSpline => {
            if values.len() < times.len() * 3 {
                let v0 = *values.get(i0)?;
                let v1 = *values.get(i1).unwrap_or(&v0);
                return Some(v0.lerp(v1, alpha));
            }
            let v0 = values[i0 * 3 + 1];
            if i0 == i1 {
                return Some(v0);
            }
            let b0 = values[i0 * 3 + 2] * dt;
            let a1 = values[i1 * 3] * dt;
            let v1 = values[i1 * 3 + 1];
            let s = alpha;
            let s2 = s * s;
            let s3 = s2 * s;
            let h00 = 2.0 * s3 - 3.0 * s2 + 1.0;
            let h10 = s3 - 2.0 * s2 + s;
            let h01 = -2.0 * s3 + 3.0 * s2;
            let h11 = s3 - s2;
            Some(v0 * h00 + b0 * h10 + v1 * h01 + a1 * h11)
        }
    }
}

fn sample_quat(values: &[Quat], times: &[f32], interp: Interpolation, t: f32) -> Option<Quat> {
    if values.is_empty() || times.is_empty() {
        return None;
    }
    let (i0, i1, alpha, dt) = find_keyframe_interval(times, t);
    match interp {
        Interpolation::Step => values.get(i0).copied(),
        Interpolation::Linear => {
            let q0 = *values.get(i0)?;
            let mut q1 = *values.get(i1).unwrap_or(&q0);
            if q0.dot(q1) < 0.0 {
                q1 = -q1;
            }
            Some(q0.slerp(q1, alpha).normalize())
        }
        Interpolation::CubicSpline => {
            if values.len() < times.len() * 3 {
                let q0 = *values.get(i0)?;
                let mut q1 = *values.get(i1).unwrap_or(&q0);
                if q0.dot(q1) < 0.0 {
                    q1 = -q1;
                }
                return Some(q0.slerp(q1, alpha).normalize());
            }
            let q0 = values[i0 * 3 + 1];
            if i0 == i1 {
                return Some(q0.normalize());
            }
            let b0 = Vec4::from_array(values[i0 * 3 + 2].to_array()) * dt;
            let a1 = Vec4::from_array(values[i1 * 3].to_array()) * dt;
            let mut v1 = Vec4::from_array(values[i1 * 3 + 1].to_array());
            let v0 = Vec4::from_array(q0.to_array());
            if v0.dot(v1) < 0.0 {
                v1 = -v1;
            }
            let s = alpha;
            let s2 = s * s;
            let s3 = s2 * s;
            let res = v0 * (2.0 * s3 - 3.0 * s2 + 1.0)
                + b0 * (s3 - 2.0 * s2 + s)
                + v1 * (-2.0 * s3 + 3.0 * s2)
                + a1 * (s3 - s2);
            let q = Quat::from_xyzw(res.x, res.y, res.z, res.w);
            Some(if q.length_squared() > 1e-8 {
                q.normalize()
            } else {
                Quat::IDENTITY
            })
        }
    }
}

/// Evaluates an optional animation clip into `pose` and recomputes world & skinning matrices.
#[cfg_attr(not(feature = "editor"), allow(dead_code))]
pub fn evaluate_pose(
    rig: &ParsedSkeletonRig,
    clip: Option<&AnimationClip>,
    time_sec: f32,
    looping: bool,
    pose: &mut SkeletonPose,
) {
    pose.reset_to_rest(rig);
    if let Some(c) = clip {
        let sample_t = if c.duration <= 1e-6 {
            0.0
        } else if looping {
            time_sec.rem_euclid(c.duration)
        } else {
            time_sec.clamp(0.0, c.duration)
        };
        for ch in &c.channels {
            let Some(&bone_idx) = rig.bone_index_by_name.get(&ch.target_bone) else {
                continue;
            };
            match &ch.track {
                AnimTrack::Translations(vals) => {
                    if let Some(v) = sample_vec3(vals, &ch.times, ch.interpolation, sample_t) {
                        pose.local_translations[bone_idx] = v;
                    }
                }
                AnimTrack::Rotations(vals) => {
                    if let Some(q) = sample_quat(vals, &ch.times, ch.interpolation, sample_t) {
                        pose.local_rotations[bone_idx] = q;
                    }
                }
                AnimTrack::Scales(vals) => {
                    if let Some(s) = sample_vec3(vals, &ch.times, ch.interpolation, sample_t) {
                        pose.local_scales[bone_idx] = s;
                    }
                }
            }
        }
    }
    pose.compute_matrices(rig);
}
