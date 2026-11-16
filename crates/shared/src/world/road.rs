use serde::{Deserialize, Serialize};

pub const ROAD_MATERIAL_COUNT: usize = 5;
pub const MAX_ROAD_MATERIALS: usize = 16;

pub const ROAD_MAT_DIRT: u8 = 0;
pub const ROAD_MAT_COBBLESTONE: u8 = 1;
pub const ROAD_MAT_BRICK: u8 = 2;
pub const ROAD_MAT_ASPHALT: u8 = 3;
pub const ROAD_MAT_FLAGSTONE: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoadStyle {
    Curve,
    Straight,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoadSpline {
    pub id: u64,
    pub style: RoadStyle,
    pub material: u8,
    pub width: f32,
    pub shoulder: f32,
    /// Control points in world space `[wx, wy, wz]`, bounded to the owning tile/area (`>= 2` points).
    pub points: Vec<[f32; 3]>,
    /// Baked longitudinal elevation profile along `sample_road_xz` stations so roads stay fixed when sculpting terrain poke-through.
    pub profile_heights: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadStation {
    pub pos: [f32; 3],
    /// Unit forward tangent `[fx, fz]` in XZ.
    pub forward_xz: [f32; 2],
    /// Unit right-hand cross-section direction `[-fz, fx]` (`forward × up = right` in right-handed Y-up space).
    pub right_xz: [f32; 2],
    /// Miter width multiplier (`1.0..=2.4`) for crisp corners and 2-way endpoint joins.
    pub miter_scale: f32,
    /// Cumulative horizontal arc length in meters from station 0.
    pub arc_len: f32,
}

#[inline]
fn dist_xz(a: [f32; 2], b: [f32; 2]) -> f32 {
    let dx = b[0] - a[0];
    let dz = b[1] - a[1];
    (dx * dx + dz * dz).sqrt()
}

#[inline]
fn norm_xz(v: [f32; 2]) -> [f32; 2] {
    let len = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if len > 1e-6 {
        [v[0] / len, v[1] / len]
    } else {
        [1.0, 0.0]
    }
}

#[inline]
pub fn right_from_forward_xz(fwd: [f32; 2]) -> [f32; 2] {
    let f = norm_xz(fwd);
    [-f[1], f[0]]
}

fn clean_control_points_xz(points: &[[f32; 3]]) -> Vec<[f32; 2]> {
    if points.is_empty() {
        return Vec::new();
    }
    let mut clean: Vec<[f32; 2]> = Vec::with_capacity(points.len());
    for p in points {
        let xz = [p[0], p[2]];
        if let Some(&prev) = clean.last() {
            if dist_xz(prev, xz) >= 0.05 {
                clean.push(xz);
            }
        } else {
            clean.push(xz);
        }
    }
    // Preserve exact final endpoint if it was pruned against a near-duplicate intermediate point.
    if points.len() >= 2 && clean.len() >= 2 {
        let last_raw = [points[points.len() - 1][0], points[points.len() - 1][2]];
        if dist_xz(clean[clean.len() - 2], last_raw) >= 0.05 {
            if let Some(last) = clean.last_mut() {
                *last = last_raw;
            }
        }
    }
    clean
}

fn catmull_rom_centripetal_xz(
    p0: [f32; 2],
    p1: [f32; 2],
    p2: [f32; 2],
    p3: [f32; 2],
    u: f32,
) -> [f32; 2] {
    let t0 = 0.0f32;
    let t1 = t0 + dist_xz(p0, p1).sqrt().max(0.05);
    let t2 = t1 + dist_xz(p1, p2).sqrt().max(0.05);
    let t3 = t2 + dist_xz(p2, p3).sqrt().max(0.05);
    let t = t1 + (t2 - t1) * u.clamp(0.0, 1.0);

    let lerp2 = |a: [f32; 2], b: [f32; 2], ta: f32, tb: f32| -> [f32; 2] {
        let denom = (tb - ta).max(1e-5);
        let w1 = (tb - t) / denom;
        let w2 = (t - ta) / denom;
        [a[0] * w1 + b[0] * w2, a[1] * w1 + b[1] * w2]
    };

    let a1 = lerp2(p0, p1, t0, t1);
    let a2 = lerp2(p1, p2, t1, t2);
    let a3 = lerp2(p2, p3, t2, t3);
    let b1 = lerp2(a1, a2, t0, t2);
    let b2 = lerp2(a2, a3, t1, t3);
    lerp2(b1, b2, t1, t2)
}

/// Samples a road centerline in XZ every `~step_m` meters according to `RoadStyle`.
pub fn sample_road_xz(points: &[[f32; 3]], style: RoadStyle, step_m: f32) -> Vec<[f32; 2]> {
    let clean = clean_control_points_xz(points);
    if clean.len() < 2 {
        return Vec::new();
    }
    let step = step_m.max(0.2);
    let mut out = Vec::new();

    if style == RoadStyle::Straight || clean.len() == 2 {
        out.push(clean[0]);
        for seg in clean.windows(2) {
            let a = seg[0];
            let b = seg[1];
            let len = dist_xz(a, b);
            let steps = (len / step).ceil().max(1.0) as usize;
            for s in 1..=steps {
                let t = s as f32 / steps as f32;
                out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
            }
        }
        return out;
    }

    let n = clean.len();
    for i in 0..(n - 1) {
        let p0 = if i == 0 {
            [2.0 * clean[0][0] - clean[1][0], 2.0 * clean[0][1] - clean[1][1]]
        } else {
            clean[i - 1]
        };
        let p1 = clean[i];
        let p2 = clean[i + 1];
        let p3 = if i + 2 < n {
            clean[i + 2]
        } else {
            [
                2.0 * clean[n - 1][0] - clean[n - 2][0],
                2.0 * clean[n - 1][1] - clean[n - 2][1],
            ]
        };
        let chord = dist_xz(p1, p2);
        let steps = (chord / step).ceil().max(2.0) as usize;
        let start_s = usize::from(i > 0);
        for s in start_s..=steps {
            let u = s as f32 / steps as f32;
            out.push(catmull_rom_centripetal_xz(p0, p1, p2, p3, u));
        }
    }
    out
}

#[inline]
fn smoothstep01(t: f32) -> f32 {
    let x = t.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Samples terrain height along `xz_samples`, smooths out small bumps so roads climb hills evenly,
/// and pins snapped start/end elevations seamlessly when meeting existing road nodes.
pub fn compute_smoothed_profile(
    xz_samples: &[[f32; 2]],
    pin_start_y: Option<f32>,
    pin_end_y: Option<f32>,
    mut terrain_y_at: impl FnMut(f32, f32) -> f32,
) -> Vec<f32> {
    let len = xz_samples.len();
    if len == 0 {
        return Vec::new();
    }

    let raw: Vec<f32> = xz_samples
        .iter()
        .map(|&[x, z]| terrain_y_at(x, z))
        .collect();

    const RADIUS: usize = 7;
    let mut smoothed = vec![0.0f32; len];
    for i in 0..len {
        let lo = i.saturating_sub(RADIUS);
        let hi = (i + RADIUS).min(len - 1);
        let mut sum = 0.0f32;
        let mut wsum = 0.0f32;
        for j in lo..=hi {
            let d = i.abs_diff(j);
            let w = (RADIUS + 1 - d) as f32;
            sum += raw[j] * w;
            wsum += w;
        }
        smoothed[i] = if wsum > 0.0 { sum / wsum } else { raw[i] };
    }

    if len == 1 {
        if let Some(sy) = pin_start_y.or(pin_end_y) {
            smoothed[0] = sy;
        }
        return smoothed;
    }

    let pin_span = 8usize.min(len - 1).max(1);
    for i in 0..len {
        let w_start = if pin_start_y.is_some() && i < pin_span {
            1.0 - smoothstep01(i as f32 / pin_span as f32)
        } else {
            0.0
        };
        let rev_i = len - 1 - i;
        let w_end = if pin_end_y.is_some() && rev_i < pin_span {
            1.0 - smoothstep01(rev_i as f32 / pin_span as f32)
        } else {
            0.0
        };
        let w_sum = w_start + w_end;
        if w_sum > 1e-5 {
            let sy = pin_start_y.unwrap_or(smoothed[i]);
            let ey = pin_end_y.unwrap_or(smoothed[i]);
            let base = smoothed[i];
            if w_sum >= 1.0 {
                smoothed[i] = (sy * w_start + ey * w_end) / w_sum;
            } else {
                let pin_target = (sy * w_start + ey * w_end) / w_sum;
                smoothed[i] = base + (pin_target - base) * w_sum;
            }
        }
    }

    smoothed
}

fn interpolate_control_heights(points: &[[f32; 3]], xz_samples: &[[f32; 2]]) -> Vec<f32> {
    if xz_samples.is_empty() {
        return Vec::new();
    }
    if points.is_empty() {
        return vec![0.0; xz_samples.len()];
    }
    let y0 = points[0][1];
    let y1 = points[points.len() - 1][1];
    let last = (xz_samples.len() - 1).max(1) as f32;
    (0..xz_samples.len())
        .map(|i| y0 + (y1 - y0) * (i as f32 / last))
        .collect()
}

fn miter_cross_section(t_in: [f32; 2], t_out: [f32; 2]) -> ([f32; 2], [f32; 2], f32) {
    let f_in = norm_xz(t_in);
    let f_out = norm_xz(t_out);
    let dot = f_in[0] * f_out[0] + f_in[1] * f_out[1];
    if dot < -0.70 {
        // Avoid extreme spikes on sharp hairpins; keep perpendicular to outgoing tangent.
        let r = right_from_forward_xz(f_out);
        return (f_out, r, 1.0);
    }
    let r_in = right_from_forward_xz(f_in);
    let r_out = right_from_forward_xz(f_out);
    let avg_r = [r_in[0] + r_out[0], r_in[1] + r_out[1]];
    let len = (avg_r[0] * avg_r[0] + avg_r[1] * avg_r[1]).sqrt();
    if len < 1e-4 {
        return (f_out, r_out, 1.0);
    }
    let miter_r = [avg_r[0] / len, avg_r[1] / len];
    let cos_half = (miter_r[0] * r_out[0] + miter_r[1] * r_out[1]).abs();
    let miter_scale = (1.0 / cos_half.max(0.42)).clamp(1.0, 2.4);
    let fwd = norm_xz([f_in[0] + f_out[0], f_in[1] + f_out[1]]);
    (fwd, miter_r, miter_scale)
}

/// Builds oriented, mitered centerline stations along `road`.
/// `start_away_dir` / `end_away_dir` are unit XZ tangents pointing AWAY from the shared endpoint
/// into an adjoining peer road on 2-way junctions or tile-border continuations.
pub fn build_road_stations(
    road: &RoadSpline,
    start_away_dir: Option<[f32; 2]>,
    end_away_dir: Option<[f32; 2]>,
) -> Vec<RoadStation> {
    let xz = sample_road_xz(&road.points, road.style, 0.5);
    let n = xz.len();
    if n < 2 {
        return Vec::new();
    }

    let heights = if road.profile_heights.len() == n {
        road.profile_heights.clone()
    } else {
        interpolate_control_heights(&road.points, &xz)
    };

    let mut arcs = vec![0.0f32; n];
    for i in 1..n {
        arcs[i] = arcs[i - 1] + dist_xz(xz[i - 1], xz[i]);
    }

    let mut stations = Vec::with_capacity(n);
    for i in 0..n {
        let seg_prev = if i > 0 {
            norm_xz([xz[i][0] - xz[i - 1][0], xz[i][1] - xz[i - 1][1]])
        } else {
            let own_out = norm_xz([xz[1][0] - xz[0][0], xz[1][1] - xz[0][1]]);
            match start_away_dir {
                Some(away) if away[0] * own_out[0] + away[1] * own_out[1] < 0.70 => {
                    norm_xz([-away[0], -away[1]])
                }
                _ => own_out,
            }
        };

        let seg_next = if i + 1 < n {
            norm_xz([xz[i + 1][0] - xz[i][0], xz[i + 1][1] - xz[i][1]])
        } else {
            let own_in = norm_xz([xz[n - 1][0] - xz[n - 2][0], xz[n - 1][1] - xz[n - 2][1]]);
            match end_away_dir {
                Some(away) if (-away[0]) * own_in[0] + (-away[1]) * own_in[1] < 0.70 => {
                    norm_xz(away)
                }
                _ => own_in,
            }
        };

        let (forward_xz, right_xz, miter_scale) = miter_cross_section(seg_prev, seg_next);
        stations.push(RoadStation {
            pos: [xz[i][0], heights[i], xz[i][1]],
            forward_xz,
            right_xz,
            miter_scale,
            arc_len: arcs[i],
        });
    }
    stations
}

/// Computes `(wx, wz, new_height)` terrain edits across integer grid vertices that level the ground
/// flat side-to-side under the roadbed (`width * 0.5 + 0.75`) and smoothly blend `shoulder` slopes
/// into surrounding terrain while following the longitudinal elevation profile over hills.
pub fn compute_road_terrain_edits(
    stations: &[RoadStation],
    width: f32,
    shoulder: f32,
    mut cur_height_at: impl FnMut(i32, i32) -> Option<f32>,
) -> Vec<(i32, i32, f32)> {
    if stations.len() < 2 {
        return Vec::new();
    }

    let half_w = (width * 0.5).max(0.5);
    let bed_half = half_w + 0.75;
    let shoulder_w = shoulder.max(0.5);
    let max_dist = bed_half + shoulder_w;
    let pad = max_dist.ceil() as i32;

    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let mut min_z = f32::MAX;
    let mut max_z = f32::MIN;
    for st in stations {
        min_x = min_x.min(st.pos[0]);
        max_x = max_x.max(st.pos[0]);
        min_z = min_z.min(st.pos[2]);
        max_z = max_z.max(st.pos[2]);
    }

    let gx_min = min_x.floor() as i32 - pad;
    let gx_max = max_x.ceil() as i32 + pad;
    let gz_min = min_z.floor() as i32 - pad;
    let gz_max = max_z.ceil() as i32 + pad;

    let mut seg_dists: Vec<(f32, f32)> = Vec::with_capacity(stations.len() - 1);
    let mut edits = Vec::new();

    for wz in gz_min..=gz_max {
        let pz = wz as f32;
        for wx in gx_min..=gx_max {
            let px = wx as f32;
            let Some(cur_h) = cur_height_at(wx, wz) else {
                continue;
            };

            seg_dists.clear();
            let mut d_min = f32::MAX;
            for seg in stations.windows(2) {
                let a = seg[0].pos;
                let b = seg[1].pos;
                let abx = b[0] - a[0];
                let abz = b[2] - a[2];
                let len2 = abx * abx + abz * abz;
                let t = if len2 > 1e-6 {
                    (((px - a[0]) * abx + (pz - a[2]) * abz) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let cx = a[0] + abx * t;
                let cz = a[2] + abz * t;
                let cy = a[1] + (b[1] - a[1]) * t;
                let d = ((px - cx).powi(2) + (pz - cz).powi(2)).sqrt();
                if d < d_min {
                    d_min = d;
                }
                if d <= max_dist + 1.2 {
                    seg_dists.push((d, cy));
                }
            }

            if d_min > max_dist || seg_dists.is_empty() {
                continue;
            }

            let mut w_sum = 0.0f32;
            let mut y_sum = 0.0f32;
            for &(d, cy) in &seg_dists {
                if d <= d_min + 1.2 {
                    let proximity = 1.0 - (d - d_min) / 1.2;
                    let w = (proximity * proximity) / (d * d + 0.25);
                    w_sum += w;
                    y_sum += cy * w;
                }
            }
            let y_target = if w_sum > 0.0 {
                y_sum / w_sum
            } else {
                seg_dists[0].1
            };

            let new_h = if d_min <= bed_half {
                y_target
            } else {
                let u = ((d_min - bed_half) / shoulder_w).clamp(0.0, 1.0);
                let s = smoothstep01(u);
                y_target * (1.0 - s) + cur_h * s
            };
            edits.push((wx, wz, new_h));
        }
    }

    edits
}

/// Closest XZ distance from point `p` to any sampled sub-segment of `road`.
pub fn distance_to_road_xz(road: &RoadSpline, px: f32, pz: f32) -> Option<(f32, f32)> {
    let stations = build_road_stations(road, None, None);
    if stations.len() < 2 {
        return None;
    }
    let mut best_d = f32::MAX;
    let mut best_y = 0.0f32;
    for seg in stations.windows(2) {
        let a = seg[0].pos;
        let b = seg[1].pos;
        let abx = b[0] - a[0];
        let abz = b[2] - a[2];
        let len2 = abx * abx + abz * abz;
        let t = if len2 > 1e-6 {
            (((px - a[0]) * abx + (pz - a[2]) * abz) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let cx = a[0] + abx * t;
        let cz = a[2] + abz * t;
        let d = ((px - cx).powi(2) + (pz - cz).powi(2)).sqrt();
        if d < best_d {
            best_d = d;
            best_y = a[1] + (b[1] - a[1]) * t;
        }
    }
    Some((best_d, best_y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_road_climbs_hill_and_levels_sides_flat() {
        let points = [[10.0, 0.0, 16.0], [34.0, 0.0, 16.0]];
        let xz = sample_road_xz(&points, RoadStyle::Straight, 0.5);
        assert!(xz.len() >= 48);
        assert!(xz.iter().all(|p| (p[1] - 16.0).abs() < 1e-5));

        // Sloped side hillside: rises along X toward 22.0 AND slopes cross-wise along Z.
        let hillside = |wx: f32, wz: f32| -> f32 {
            let hill = (-(wx - 22.0).powi(2) / 48.0).exp() * 6.0;
            let side_tilt = (wz - 16.0) * 0.4;
            hill + side_tilt
        };

        let profile = compute_smoothed_profile(&xz, None, None, hillside);
        let mid_y = profile[profile.len() / 2];
        assert!(mid_y > 4.0, "expected road profile to crest hill: {mid_y}");

        let road = RoadSpline {
            id: 1,
            style: RoadStyle::Straight,
            material: ROAD_MAT_COBBLESTONE,
            width: 4.0,
            shoulder: 3.0,
            points: points.to_vec(),
            profile_heights: profile,
        };
        let stations = build_road_stations(&road, None, None);
        let edits = compute_road_terrain_edits(&stations, 4.0, 3.0, |wx, wz| {
            Some(hillside(wx as f32, wz as f32))
        });

        let height_at = |x: i32, z: i32| {
            edits
                .iter()
                .find(|&&(ex, ez, _)| ex == x && ez == z)
                .map(|&(_, _, h)| h)
                .unwrap()
        };

        // Roadbed cross-section at X = 22 must be flat across Z = 14, 16, 18 despite initial side_tilt.
        let h_left = height_at(22, 14);
        let h_center = height_at(22, 16);
        let h_right = height_at(22, 18);
        assert!((h_left - h_center).abs() < 0.08, "left={h_left} vs center={h_center}");
        assert!((h_right - h_center).abs() < 0.08, "right={h_right} vs center={h_center}");
        assert!(h_center > 4.0);
    }

    #[test]
    fn curve_road_passes_through_control_points_and_pins_endpoints() {
        let points = [
            [8.0, 0.0, 8.0],
            [18.0, 0.0, 12.0],
            [24.0, 0.0, 24.0],
            [36.0, 0.0, 28.0],
        ];
        let xz = sample_road_xz(&points, RoadStyle::Curve, 0.5);
        assert!(xz.len() > 20);
        assert!(dist_xz(xz[0], [8.0, 8.0]) < 1e-4);
        assert!(dist_xz(*xz.last().unwrap(), [36.0, 28.0]) < 1e-4);

        let profile = compute_smoothed_profile(&xz, Some(2.5), Some(4.25), |x, _| x * 0.1);
        assert!((profile[0] - 2.5).abs() < 1e-5);
        assert!((*profile.last().unwrap() - 4.25).abs() < 1e-5);
    }
}
