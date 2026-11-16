use std::collections::HashSet;

use glam::Vec3;
use shared::world::{
    AreaKind, MAX_ROAD_MATERIALS, RoadSpline, RoadStyle, TILE_SIZE, TILE_VERTICES,
    build_road_stations, compute_road_terrain_edits, compute_smoothed_profile, distance_to_road_xz,
    sample_road_xz,
};

use crate::camera::Camera;
use crate::input::InputState;
use crate::render::RoadMeshGpu;

use super::state::EditorState;
use super::types::{EditorTool, RoadDraft, RoadSubMode};

const NODE_SNAP_RADIUS: f32 = 1.6;
const BORDER_SNAP_RADIUS: f32 = 0.85;
const NODE_MATCH_EPS: f32 = 0.05;
const NODE_PICK_RADIUS: f32 = 1.25;

#[derive(Clone, Copy)]
struct RectBounds {
    min_x: f32,
    max_x: f32,
    min_z: f32,
    max_z: f32,
}

impl RectBounds {
    fn from_tile(tx: i32, ty: i32) -> Self {
        let ox = (tx * TILE_SIZE as i32) as f32;
        let oz = (ty * TILE_SIZE as i32) as f32;
        Self {
            min_x: ox,
            max_x: ox + TILE_SIZE as f32,
            min_z: oz,
            max_z: oz + TILE_SIZE as f32,
        }
    }

    fn contains_xz(self, x: f32, z: f32) -> bool {
        const EPS: f32 = 1e-3;
        x >= self.min_x - EPS
            && x <= self.max_x + EPS
            && z >= self.min_z - EPS
            && z <= self.max_z + EPS
    }

    fn clamp_xz(self, x: f32, z: f32) -> [f32; 2] {
        [x.clamp(self.min_x, self.max_x), z.clamp(self.min_z, self.max_z)]
    }

    fn intersect(self, other: Self) -> Option<Self> {
        let min_x = self.min_x.max(other.min_x);
        let max_x = self.max_x.min(other.max_x);
        let min_z = self.min_z.max(other.min_z);
        let max_z = self.max_z.min(other.max_z);
        if min_x <= max_x + 1e-4 && min_z <= max_z + 1e-4 {
            Some(Self {
                min_x,
                max_x: max_x.max(min_x),
                min_z,
                max_z: max_z.max(min_z),
            })
        } else {
            None
        }
    }
}

fn snap_to_tile_border(x: f32, z: f32, bounds: Option<RectBounds>) -> ([f32; 2], bool) {
    let tile_f = TILE_SIZE as f32;
    let nearest_x = (x / tile_f).round() * tile_f;
    let nearest_z = (z / tile_f).round() * tile_f;

    let mut sx = x;
    let mut sz = z;
    let mut snapped = false;

    if (x - nearest_x).abs() <= BORDER_SNAP_RADIUS
        && bounds.is_none_or(|b| nearest_x >= b.min_x - 1e-3 && nearest_x <= b.max_x + 1e-3)
    {
        sx = nearest_x;
        snapped = true;
    }
    if (z - nearest_z).abs() <= BORDER_SNAP_RADIUS
        && bounds.is_none_or(|b| nearest_z >= b.min_z - 1e-3 && nearest_z <= b.max_z + 1e-3)
    {
        sz = nearest_z;
        snapped = true;
    }
    ([sx, sz], snapped)
}

#[inline]
fn dist_xz(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = b[0] - a[0];
    let dz = b[2] - a[2];
    (dx * dx + dz * dz).sqrt()
}

fn road_endpoint_away_dir(road: &RoadSpline, at_start: bool) -> Option<[f32; 2]> {
    let xz = sample_road_xz(&road.points, road.style, 0.5);
    if xz.len() < 2 {
        return None;
    }
    let (a, b) = if at_start {
        (xz[0], xz[1])
    } else {
        (xz[xz.len() - 1], xz[xz.len() - 2])
    };
    let dx = b[0] - a[0];
    let dz = b[1] - a[1];
    let len = (dx * dx + dz * dz).sqrt();
    if len > 1e-5 {
        Some([dx / len, dz / len])
    } else {
        None
    }
}

impl EditorState {
    pub(super) fn find_selected_road(&self) -> Option<&RoadSpline> {
        let id = self.selected_road_id?;
        if self.active_area.kind == AreaKind::Outside {
            self.outside_tiles
                .values()
                .flat_map(|t| t.vis.roads.iter())
                .find(|r| r.id == id)
        } else {
            self.inside_area
                .as_ref()
                .and_then(|ins| ins.vis.roads.iter().find(|r| r.id == id))
        }
    }

    fn all_loaded_roads(&self) -> Vec<(Option<(i32, i32)>, &RoadSpline)> {
        if self.active_area.kind == AreaKind::Outside {
            let mut out = Vec::new();
            for (&coord, tile) in &self.outside_tiles {
                for r in &tile.vis.roads {
                    out.push((Some(coord), r));
                }
            }
            out
        } else if let Some(ins) = &self.inside_area {
            ins.vis.roads.iter().map(|r| (None, r)).collect()
        } else {
            Vec::new()
        }
    }

    fn find_road_owner_tile(&self, road_id: u64) -> Option<(i32, i32)> {
        if self.active_area.kind != AreaKind::Outside {
            return None;
        }
        self.outside_tiles
            .iter()
            .find(|(_, tile)| tile.vis.roads.iter().any(|r| r.id == road_id))
            .map(|(&coord, _)| coord)
    }

    fn find_owner_tile_for_points(&self, points: &[[f32; 3]], locked: Option<(i32, i32)>) -> Option<(i32, i32)> {
        if let Some(t) = locked {
            if self.outside_tiles.contains_key(&t) {
                return Some(t);
            }
        }
        if points.is_empty() {
            return None;
        }
        let mid_x = points.iter().map(|p| p[0]).sum::<f32>() / points.len() as f32;
        let mid_z = points.iter().map(|p| p[2]).sum::<f32>() / points.len() as f32;
        let mut best: Option<(f32, (i32, i32))> = None;
        for (&(tx, ty), _) in &self.outside_tiles {
            let b = RectBounds::from_tile(tx, ty);
            if points.iter().all(|p| b.contains_xz(p[0], p[2])) {
                let cx = (tx * TILE_SIZE as i32) as f32 + TILE_SIZE as f32 * 0.5;
                let cz = (ty * TILE_SIZE as i32) as f32 + TILE_SIZE as f32 * 0.5;
                let d2 = (cx - mid_x).powi(2) + (cz - mid_z).powi(2);
                if best.is_none_or(|(bd, _)| d2 < bd) {
                    best = Some((d2, (tx, ty)));
                }
            }
        }
        best.map(|(_, t)| t)
    }

    fn find_snap_node(
        &self,
        raw_x: f32,
        raw_z: f32,
        bounds: Option<RectBounds>,
        excluded: &[(u64, usize)],
    ) -> Option<[f32; 3]> {
        let mut best_d = NODE_SNAP_RADIUS;
        let mut best_pos: Option<[f32; 3]> = None;

        for (_, road) in self.all_loaded_roads() {
            for (idx, &pt) in road.points.iter().enumerate() {
                if excluded.iter().any(|&(eid, eidx)| eid == road.id && eidx == idx) {
                    continue;
                }
                if let Some(b) = bounds {
                    if !b.contains_xz(pt[0], pt[2]) {
                        continue;
                    }
                } else if self.active_area.kind == AreaKind::Outside {
                    let shares_tile = self.outside_tiles.keys().any(|&(tx, ty)| {
                        let b = RectBounds::from_tile(tx, ty);
                        b.contains_xz(raw_x, raw_z) && b.contains_xz(pt[0], pt[2])
                    });
                    if !shares_tile {
                        continue;
                    }
                }
                let dx = pt[0] - raw_x;
                let dz = pt[2] - raw_z;
                let d = (dx * dx + dz * dz).sqrt();
                if d < best_d {
                    best_d = d;
                    best_pos = Some(pt);
                }
            }
        }
        best_pos
    }

    fn find_existing_node_y_excluding(&self, pos: [f32; 3], exclude_road_id: Option<u64>) -> Option<f32> {
        let mut best_d = NODE_MATCH_EPS;
        let mut best_y: Option<f32> = None;
        for (_, road) in self.all_loaded_roads() {
            if Some(road.id) == exclude_road_id {
                continue;
            }
            for &pt in &road.points {
                let d = dist_xz(pt, pos);
                if d < best_d {
                    best_d = d;
                    best_y = Some(pt[1]);
                }
            }
        }
        best_y
    }

    fn compute_endpoint_joins(&self, road: &RoadSpline) -> (Option<[f32; 2]>, Option<[f32; 2]>, u8) {
        if road.points.len() < 2 {
            return (None, None, 0);
        }
        let all = self.all_loaded_roads();
        let p_start = road.points[0];
        let p_end = road.points[road.points.len() - 1];

        let mut start_matches: Vec<[f32; 2]> = Vec::new();
        let mut end_matches: Vec<[f32; 2]> = Vec::new();
        let mut overlaps_interior = false;

        for &(_, other) in &all {
            if other.id == road.id || other.points.len() < 2 {
                continue;
            }
            let o_start = other.points[0];
            let o_end = other.points[other.points.len() - 1];

            let matched_start = if dist_xz(p_start, o_start) <= NODE_MATCH_EPS {
                if let Some(dir) = road_endpoint_away_dir(other, true) {
                    start_matches.push(dir);
                }
                true
            } else if dist_xz(p_start, o_end) <= NODE_MATCH_EPS {
                if let Some(dir) = road_endpoint_away_dir(other, false) {
                    start_matches.push(dir);
                }
                true
            } else {
                false
            };

            let matched_end = if dist_xz(p_end, o_start) <= NODE_MATCH_EPS {
                if let Some(dir) = road_endpoint_away_dir(other, true) {
                    end_matches.push(dir);
                }
                true
            } else if dist_xz(p_end, o_end) <= NODE_MATCH_EPS {
                if let Some(dir) = road_endpoint_away_dir(other, false) {
                    end_matches.push(dir);
                }
                true
            } else {
                false
            };

            if !matched_start {
                if let Some((d, _)) = distance_to_road_xz(other, p_start[0], p_start[2]) {
                    if d <= other.width * 0.5 + 0.15 {
                        overlaps_interior = true;
                    }
                }
            }
            if !matched_end {
                if let Some((d, _)) = distance_to_road_xz(other, p_end[0], p_end[2]) {
                    if d <= other.width * 0.5 + 0.15 {
                        overlaps_interior = true;
                    }
                }
            }
        }

        let start_away = if start_matches.len() == 1 && !overlaps_interior {
            Some(start_matches[0])
        } else {
            None
        };
        let end_away = if end_matches.len() == 1 && !overlaps_interior {
            Some(end_matches[0])
        } else {
            None
        };
        let multi_junction =
            start_matches.len() >= 2 || end_matches.len() >= 2 || overlaps_interior;
        let tier = if multi_junction {
            ((road.id % 11) as u8) + 1
        } else {
            0
        };
        (start_away, end_away, tier)
    }

    pub(super) fn rebuild_outside_road_meshes_near(&mut self, gl: &glow::Context, tx: i32, ty: i32) {
        let mut coords = Vec::new();
        for dy in -1..=1 {
            for dx in -1..=1 {
                let k = (tx + dx, ty + dy);
                if self.outside_tiles.contains_key(&k) {
                    coords.push(k);
                }
            }
        }
        for coord in coords {
            let Some(tile) = self.outside_tiles.get(&coord) else {
                continue;
            };
            let mut built = Vec::with_capacity(tile.vis.roads.len());
            for road in &tile.vis.roads {
                let (s_dir, e_dir, tier) = self.compute_endpoint_joins(road);
                let is_soft = self.is_road_material_soft(road.material);
                if let Ok(mesh) = RoadMeshGpu::from_road(gl, road, s_dir, e_dir, tier, is_soft) {
                    built.push((road.id, mesh));
                }
            }
            if let Some(tile_mut) = self.outside_tiles.get_mut(&coord) {
                for mesh in tile_mut.road_meshes.drain().map(|(_, m)| m) {
                    mesh.destroy(gl);
                }
                for (id, mesh) in built {
                    tile_mut.road_meshes.insert(id, mesh);
                }
            } else {
                for (_, mesh) in built {
                    mesh.destroy(gl);
                }
            }
        }
    }

    pub(super) fn rebuild_inside_road_meshes(&mut self, gl: &glow::Context) {
        let Some(inside) = &self.inside_area else {
            return;
        };
        let mut built = Vec::with_capacity(inside.vis.roads.len());
        for road in &inside.vis.roads {
            let (s_dir, e_dir, tier) = self.compute_endpoint_joins(road);
            let is_soft = self.is_road_material_soft(road.material);
            if let Ok(mesh) = RoadMeshGpu::from_road(gl, road, s_dir, e_dir, tier, is_soft) {
                built.push((road.id, mesh));
            }
        }
        if let Some(inside_mut) = &mut self.inside_area {
            for mesh in inside_mut.road_meshes.drain().map(|(_, m)| m) {
                mesh.destroy(gl);
            }
            for (id, mesh) in built {
                inside_mut.road_meshes.insert(id, mesh);
            }
        } else {
            for (_, mesh) in built {
                mesh.destroy(gl);
            }
        }
    }

    pub(super) fn rebuild_all_active_road_meshes(&mut self, gl: &glow::Context) {
        if self.active_area.kind == AreaKind::Outside {
            let coords: Vec<(i32, i32)> = self.outside_tiles.keys().copied().collect();
            for coord in coords {
                let Some(tile) = self.outside_tiles.get(&coord) else {
                    continue;
                };
                let mut built = Vec::with_capacity(tile.vis.roads.len());
                for road in &tile.vis.roads {
                    let (s_dir, e_dir, tier) = self.compute_endpoint_joins(road);
                    let is_soft = self.is_road_material_soft(road.material);
                    if let Ok(mesh) = RoadMeshGpu::from_road(gl, road, s_dir, e_dir, tier, is_soft) {
                        built.push((road.id, mesh));
                    }
                }
                if let Some(tile_mut) = self.outside_tiles.get_mut(&coord) {
                    for mesh in tile_mut.road_meshes.drain().map(|(_, m)| m) {
                        mesh.destroy(gl);
                    }
                    for (id, mesh) in built {
                        tile_mut.road_meshes.insert(id, mesh);
                    }
                }
            }
        } else {
            self.rebuild_inside_road_meshes(gl);
        }
    }

    fn recompute_road_profile(&self, road: &mut RoadSpline) {
        if road.points.len() < 2 {
            road.profile_heights.clear();
            return;
        }
        let xz = sample_road_xz(&road.points, road.style, 0.5);
        let pin_start = self.find_existing_node_y_excluding(road.points[0], Some(road.id));
        let pin_end = self
            .find_existing_node_y_excluding(road.points[road.points.len() - 1], Some(road.id));
        let fallback = road.points[0][1];
        let profile = compute_smoothed_profile(&xz, pin_start, pin_end, |x, z| {
            self.terrain_height_at(x, z).unwrap_or(fallback)
        });
        if let (Some(&sy), Some(&ey)) = (profile.first(), profile.last()) {
            road.points[0][1] = sy;
            let last = road.points.len() - 1;
            road.points[last][1] = ey;
        }
        // Keep interior node gizmo elevation aligned to nearest sampled road station.
        for pt in road.points.iter_mut().skip(1) {
            if let Some((idx, _)) = xz
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    let da = (a[0] - pt[0]).powi(2) + (a[1] - pt[2]).powi(2);
                    let db = (b[0] - pt[0]).powi(2) + (b[1] - pt[2]).powi(2);
                    da.total_cmp(&db)
                })
            {
                if let Some(&py) = profile.get(idx) {
                    pt[1] = py;
                }
            }
        }
        road.profile_heights = profile;
    }

    fn apply_terrain_leveling_for_roads(&mut self, gl: &glow::Context, road_ids: &[u64]) {
        let mut all_edits: Vec<(i32, i32, f32)> = Vec::new();
        for &rid in road_ids {
            let Some(road) = self
                .all_loaded_roads()
                .into_iter()
                .find(|(_, r)| r.id == rid)
                .map(|(_, r)| r.clone())
            else {
                continue;
            };
            let (s_dir, e_dir, _) = self.compute_endpoint_joins(&road);
            let stations = build_road_stations(&road, s_dir, e_dir);
            let edits = compute_road_terrain_edits(&stations, road.width, road.shoulder, |wx, wz| {
                self.sample_vertex_height(wx, wz)
            });
            all_edits.extend(edits);
        }

        if all_edits.is_empty() {
            return;
        }

        if self.active_area.kind == AreaKind::Outside {
            let mut touched_tiles: HashSet<(i32, i32)> = HashSet::new();
            for (wx, wz, h) in all_edits {
                for (&(tx, ty), tile) in &mut self.outside_tiles {
                    let ox = tx * TILE_SIZE as i32;
                    let oz = ty * TILE_SIZE as i32;
                    if wx >= ox
                        && wx <= ox + TILE_SIZE as i32
                        && wz >= oz
                        && wz <= oz + TILE_SIZE as i32
                    {
                        let vx = (wx - ox) as usize;
                        let vz = (wz - oz) as usize;
                        tile.vis.heights[vz * TILE_VERTICES + vx] = h;
                        tile.dirty = true;
                        touched_tiles.insert((tx, ty));
                    }
                }
            }
            if !touched_tiles.is_empty() {
                let mut refresh = HashSet::new();
                for &(tx, ty) in &touched_tiles {
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let k = (tx + dx, ty + dy);
                            if self.outside_tiles.contains_key(&k) {
                                refresh.insert(k);
                            }
                        }
                    }
                }
                let refresh_vec: Vec<_> = refresh.into_iter().collect();
                self.refresh_outside_tile_meshes(gl, &refresh_vec);
            }
        } else if let Some(inside) = &mut self.inside_area {
            let w = inside.vis.width as i32;
            let l = inside.vis.length as i32;
            let vw = w as usize + 1;
            let mut changed = false;
            for (wx, wz, h) in all_edits {
                if wx >= 0 && wx <= w && wz >= 0 && wz <= l {
                    inside.vis.heights[wz as usize * vw + wx as usize] = h;
                    changed = true;
                }
            }
            if changed {
                inside.dirty = true;
                self.refresh_inside_mesh(gl);
            }
        }
    }

    fn resolve_place_cursor_point(&self, raw: Vec3) -> ([f32; 3], bool) {
        let candidate_bounds = match self.active_area.kind {
            AreaKind::Outside => {
                if let Some(draft) = &self.road_draft {
                    if let Some((tx, ty)) = draft.locked_tile {
                        Some(RectBounds::from_tile(tx, ty))
                    } else if let Some(&p0) = draft.points.first() {
                        // p0 sits on a shared boundary: pick loaded tile touching p0 closest to cursor
                        self.outside_tiles
                            .keys()
                            .filter_map(|&(tx, ty)| {
                                let b = RectBounds::from_tile(tx, ty);
                                b.contains_xz(p0[0], p0[2]).then_some((tx, ty, b))
                            })
                            .min_by(|&(ax, ay, _), &(bx, by, _)| {
                                let acx = (ax * TILE_SIZE as i32) as f32 + 32.0;
                                let acz = (ay * TILE_SIZE as i32) as f32 + 32.0;
                                let bcx = (bx * TILE_SIZE as i32) as f32 + 32.0;
                                let bcz = (by * TILE_SIZE as i32) as f32 + 32.0;
                                let da = (acx - raw.x).powi(2) + (acz - raw.z).powi(2);
                                let db = (bcx - raw.x).powi(2) + (bcz - raw.z).powi(2);
                                da.total_cmp(&db)
                            })
                            .map(|(_, _, b)| b)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            AreaKind::Inside(_) => self.inside_area.as_ref().map(|ins| RectBounds {
                min_x: 0.0,
                max_x: ins.vis.width as f32,
                min_z: 0.0,
                max_z: ins.vis.length as f32,
            }),
        };

        if let Some(snapped) = self.find_snap_node(raw.x, raw.z, candidate_bounds, &[]) {
            return (snapped, true);
        }

        let [cx, cz] = if let Some(b) = candidate_bounds {
            b.clamp_xz(raw.x, raw.z)
        } else {
            [raw.x, raw.z]
        };
        let ([bx, bz], border_snapped) = if self.active_area.kind == AreaKind::Outside {
            snap_to_tile_border(cx, cz, candidate_bounds)
        } else {
            ([cx, cz], false)
        };
        let cy = self.terrain_height_at(bx, bz).unwrap_or(raw.y);
        ([bx, cy, bz], border_snapped)
    }

    pub(super) fn update_road_hover_preview(&mut self, gl: &glow::Context) {
        if self.tool != EditorTool::Road || self.road_submode != RoadSubMode::Place {
            self.hover_road_point = None;
            return;
        }
        let Some(raw) = self.hover_world else {
            self.hover_road_point = None;
            return;
        };
        let (pt, snapped) = self.resolve_place_cursor_point(raw);
        self.hover_road_point = Some((pt, snapped));

        let style = self.road_style;
        let material = self.road_material;
        let width = self.road_width;
        let shoulder = self.road_shoulder;

        let preview_points_opt = self.road_draft.as_ref().map(|d| {
            let mut pts = d.points.clone();
            if pts.is_empty() || dist_xz(*pts.last().unwrap(), pt) >= 0.25 {
                pts.push(pt);
            }
            pts
        });

        let Some(preview_points) = preview_points_opt else {
            return;
        };

        let key = (pt, preview_points.len(), style, material, width);
        let needs_rebuild = self
            .road_draft
            .as_ref()
            .is_some_and(|d| d.preview_key.as_ref() != Some(&key));
        if !needs_rebuild {
            return;
        }

        let new_gpu = if preview_points.len() >= 2 {
            let xz = sample_road_xz(&preview_points, style, 0.5);
            let pin_start = self.find_existing_node_y_excluding(preview_points[0], None);
            let pin_end =
                self.find_existing_node_y_excluding(preview_points[preview_points.len() - 1], None);
            let fallback = preview_points[0][1];
            let profile = compute_smoothed_profile(&xz, pin_start, pin_end, |x, z| {
                self.terrain_height_at(x, z).unwrap_or(fallback)
            });
            let ghost = RoadSpline {
                id: 0,
                style,
                material,
                width,
                shoulder,
                points: preview_points,
                profile_heights: profile,
            };
            RoadMeshGpu::from_road(
                gl,
                &ghost,
                None,
                None,
                2,
                self.is_road_material_soft(ghost.material),
            )
            .ok()
        } else {
            None
        };

        if let Some(draft) = &mut self.road_draft {
            if let Some(old) = draft.preview_gpu.take() {
                old.destroy(gl);
            }
            draft.preview_gpu = new_gpu;
            draft.preview_key = Some(key);
        }
    }

    pub(super) fn handle_road_hotkeys(&mut self, gl: &glow::Context, input: &InputState, now_ms: f64) {
        if self.road_submode == RoadSubMode::Place {
            if input.is_key_just_pressed("Escape") {
                self.cancel_road_draft(gl);
                return;
            }
            if input.is_key_just_pressed("Backspace") {
                if let Some(draft) = &mut self.road_draft {
                    draft.points.pop();
                    draft.preview_key = None;
                    if draft.points.len() <= 1 {
                        draft.locked_tile = None;
                    }
                    if draft.points.is_empty() {
                        self.cancel_road_draft(gl);
                        return;
                    }
                }
                self.update_road_hover_preview(gl);
                return;
            }
            if input.is_key_just_pressed("Enter") || input.is_key_just_pressed("NumpadEnter") {
                self.finish_road_draft(gl, now_ms);
            }
        } else if self.road_submode == RoadSubMode::Edit
            && (input.is_key_just_pressed("Delete") || input.is_key_just_pressed("Backspace"))
        {
            self.delete_selected_road(gl, now_ms);
        }
    }

    pub(super) fn handle_road_tool(
        &mut self,
        gl: &glow::Context,
        camera: &Camera,
        input: &InputState,
        now_ms: f64,
    ) {
        match self.road_submode {
            RoadSubMode::Place => {
                self.update_road_hover_preview(gl);
                if input.right_just_pressed {
                    self.finish_road_draft(gl, now_ms);
                    return;
                }
                if input.left_just_pressed {
                    let Some((pt, _)) = self.hover_road_point else {
                        return;
                    };
                    if let Some(draft) = &mut self.road_draft {
                        if draft.points.len() >= 2 && dist_xz(*draft.points.last().unwrap(), pt) < 0.25 {
                            self.finish_road_draft(gl, now_ms);
                            return;
                        }
                        if draft.points.is_empty() || dist_xz(*draft.points.last().unwrap(), pt) >= 0.25 {
                            draft.points.push(pt);
                            draft.preview_key = None;
                        }
                    } else {
                        self.road_draft = Some(RoadDraft {
                            points: vec![pt],
                            locked_tile: None,
                            preview_gpu: None,
                            preview_key: None,
                        });
                    }

                    // Lock to owning tile as soon as points uniquely identify one tile (interior point).
                    if self.active_area.kind == AreaKind::Outside {
                        if let Some(draft) = &mut self.road_draft {
                            if draft.locked_tile.is_none() {
                                let matching: Vec<(i32, i32)> = self
                                    .outside_tiles
                                    .keys()
                                    .copied()
                                    .filter(|&(tx, ty)| {
                                        let b = RectBounds::from_tile(tx, ty);
                                        draft.points.iter().all(|p| b.contains_xz(p[0], p[2]))
                                    })
                                    .collect();
                                if matching.len() == 1 {
                                    draft.locked_tile = Some(matching[0]);
                                }
                            }
                        }
                    }
                    self.update_road_hover_preview(gl);
                }
            }
            RoadSubMode::Edit => {
                self.hover_road_point = None;
                let ray_hit = camera
                    .screen_ray(
                        input.mouse_x,
                        input.mouse_y,
                        input.canvas_width,
                        input.canvas_height,
                    )
                    .and_then(|(orig, dir)| self.raycast_terrain(orig, dir))
                    .or(self.hover_world);

                if input.left_just_pressed {
                    if let Some(hit) = ray_hit {
                        self.pick_road_or_node(hit);
                    }
                }

                if input.left_mouse_down && self.dragging_road_node {
                    if let Some(hit) = ray_hit {
                        self.drag_selected_road_node(gl, hit);
                    }
                }
            }
        }
    }

    fn pick_road_or_node(&mut self, hit: Vec3) {
        self.dragging_road_node = false;
        self.road_drag_moved = false;
        self.welded_drag_peers.clear();

        // Priority 1: control point on currently selected road
        if let Some(sel_id) = self.selected_road_id {
            if let Some(road) = self.find_selected_road() {
                if let Some((idx, pos)) = road
                    .points
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        let da = (a[0] - hit.x).powi(2) + (a[2] - hit.z).powi(2);
                        let db = (b[0] - hit.x).powi(2) + (b[2] - hit.z).powi(2);
                        da.total_cmp(&db)
                    })
                    .filter(|(_, p)| ((p[0] - hit.x).powi(2) + (p[2] - hit.z).powi(2)).sqrt() <= NODE_PICK_RADIUS)
                {
                    let pos = *pos;
                    let n_len = road.points.len();
                    self.selected_road_node = Some(idx);
                    self.dragging_road_node = true;
                    if idx == 0 || idx + 1 == n_len {
                        let peers = self.collect_welded_peers(sel_id, pos);
                        self.welded_drag_peers = peers;
                    }
                    return;
                }
            }
        }

        // Priority 2: any road node within pick radius
        let mut best_node: Option<(f32, u64, usize, [f32; 3], bool)> = None;
        for (_, road) in self.all_loaded_roads() {
            for (idx, &p) in road.points.iter().enumerate() {
                let d = ((p[0] - hit.x).powi(2) + (p[2] - hit.z).powi(2)).sqrt();
                if d <= NODE_PICK_RADIUS && best_node.is_none_or(|(bd, ..)| d < bd) {
                    let is_end = idx == 0 || idx + 1 == road.points.len();
                    best_node = Some((d, road.id, idx, p, is_end));
                }
            }
        }
        if let Some((_, rid, nidx, pos, is_end)) = best_node {
            self.selected_road_id = Some(rid);
            self.selected_road_node = Some(nidx);
            self.dragging_road_node = true;
            if is_end {
                self.welded_drag_peers = self.collect_welded_peers(rid, pos);
            }
            return;
        }

        // Priority 3: closest road ribbon body
        let mut best_road: Option<(f32, u64)> = None;
        for (_, road) in self.all_loaded_roads() {
            if let Some((d, _)) = distance_to_road_xz(road, hit.x, hit.z) {
                if d <= road.width * 0.5 + 0.35 && best_road.is_none_or(|(bd, _)| d < bd) {
                    best_road = Some((d, road.id));
                }
            }
        }
        if let Some((_, rid)) = best_road {
            self.selected_road_id = Some(rid);
            self.selected_road_node = None;
        } else {
            self.selected_road_id = None;
            self.selected_road_node = None;
        }
    }

    fn collect_welded_peers(&self, primary_id: u64, pos: [f32; 3]) -> Vec<(Option<(i32, i32)>, u64, usize)> {
        let mut peers = Vec::new();
        for (tile_coord, road) in self.all_loaded_roads() {
            if road.id == primary_id || road.points.len() < 2 {
                continue;
            }
            if dist_xz(road.points[0], pos) <= NODE_MATCH_EPS {
                peers.push((tile_coord, road.id, 0));
            }
            let last = road.points.len() - 1;
            if dist_xz(road.points[last], pos) <= NODE_MATCH_EPS {
                peers.push((tile_coord, road.id, last));
            }
        }
        peers
    }

    fn drag_selected_road_node(&mut self, gl: &glow::Context, hit: Vec3) {
        let (Some(rid), Some(nidx)) = (self.selected_road_id, self.selected_road_node) else {
            return;
        };

        // Intersect allowed bounds of the primary road's tile and all welded peer tiles so
        // cross-tile border nodes stay strictly on the shared border without tearing apart.
        let mut bounds: Option<RectBounds> = match self.active_area.kind {
            AreaKind::Outside => self.find_road_owner_tile(rid).map(|(tx, ty)| RectBounds::from_tile(tx, ty)),
            AreaKind::Inside(_) => self.inside_area.as_ref().map(|ins| RectBounds {
                min_x: 0.0,
                max_x: ins.vis.width as f32,
                min_z: 0.0,
                max_z: ins.vis.length as f32,
            }),
        };
        for &(peer_tile, _, _) in &self.welded_drag_peers {
            if let Some((tx, ty)) = peer_tile {
                if let Some(cur) = bounds {
                    bounds = cur.intersect(RectBounds::from_tile(tx, ty)).or(bounds);
                }
            }
        }

        let mut excluded: Vec<(u64, usize)> = vec![(rid, nidx)];
        for &(_, pid, pnidx) in &self.welded_drag_peers {
            excluded.push((pid, pnidx));
        }

        let target_pt = if let Some(snapped) = self.find_snap_node(hit.x, hit.z, bounds, &excluded) {
            snapped
        } else {
            let [cx, cz] = bounds.map_or([hit.x, hit.z], |b| b.clamp_xz(hit.x, hit.z));
            let [cx, cz] = if self.active_area.kind == AreaKind::Outside {
                snap_to_tile_border(cx, cz, bounds).0
            } else {
                [cx, cz]
            };
            let cy = self.terrain_height_at(cx, cz).unwrap_or(hit.y);
            [cx, cy, cz]
        };

        let mut targets: Vec<(Option<(i32, i32)>, u64, usize)> =
            vec![(self.find_road_owner_tile(rid), rid, nidx)];
        targets.extend(self.welded_drag_peers.iter().copied());

        let mut changed_ids = Vec::new();
        for (tile_coord, tid, tidx) in targets {
            let mut moved = false;
            if let Some(tx_ty) = tile_coord {
                if let Some(tile) = self.outside_tiles.get_mut(&tx_ty) {
                    if let Some(road) = tile.vis.roads.iter_mut().find(|r| r.id == tid) {
                        if let Some(pt) = road.points.get_mut(tidx) {
                            if ((pt[0] - target_pt[0]).powi(2) + (pt[2] - target_pt[2]).powi(2)).sqrt() > 0.005 {
                                *pt = target_pt;
                                moved = true;
                            }
                        }
                    }
                }
            } else if let Some(ins) = &mut self.inside_area {
                if let Some(road) = ins.vis.roads.iter_mut().find(|r| r.id == tid) {
                    if let Some(pt) = road.points.get_mut(tidx) {
                        if ((pt[0] - target_pt[0]).powi(2) + (pt[2] - target_pt[2]).powi(2)).sqrt() > 0.005 {
                            *pt = target_pt;
                            moved = true;
                        }
                    }
                }
            }
            if moved {
                changed_ids.push(tid);
                self.road_drag_moved = true;
            }
        }

        if changed_ids.is_empty() {
            return;
        }

        for cid in changed_ids {
            self.recompute_road_by_id(cid);
        }
        self.rebuild_all_active_road_meshes(gl);
    }

    fn recompute_road_by_id(&mut self, road_id: u64) {
        if self.active_area.kind == AreaKind::Outside {
            let Some(coord) = self.find_road_owner_tile(road_id) else {
                return;
            };
            let Some(mut road) = self
                .outside_tiles
                .get(&coord)
                .and_then(|t| t.vis.roads.iter().find(|r| r.id == road_id).cloned())
            else {
                return;
            };
            self.recompute_road_profile(&mut road);
            if let Some(tile) = self.outside_tiles.get_mut(&coord) {
                if let Some(dst) = tile.vis.roads.iter_mut().find(|r| r.id == road_id) {
                    *dst = road;
                    tile.dirty = true;
                }
            }
        } else if let Some(mut road) = self
            .inside_area
            .as_ref()
            .and_then(|i| i.vis.roads.iter().find(|r| r.id == road_id).cloned())
        {
            self.recompute_road_profile(&mut road);
            if let Some(ins) = &mut self.inside_area {
                if let Some(dst) = ins.vis.roads.iter_mut().find(|r| r.id == road_id) {
                    *dst = road;
                    ins.dirty = true;
                }
            }
        }
    }

    pub(super) fn finish_road_node_drag(&mut self, gl: &glow::Context, now_ms: f64) {
        let was_moved = self.road_drag_moved;
        self.dragging_road_node = false;
        self.road_drag_moved = false;
        let peers = std::mem::take(&mut self.welded_drag_peers);
        if !was_moved {
            return;
        }
        let mut ids = Vec::new();
        if let Some(rid) = self.selected_road_id {
            ids.push(rid);
        }
        for &(_, pid, _) in &peers {
            if !ids.contains(&pid) {
                ids.push(pid);
            }
        }
        for &id in &ids {
            self.recompute_road_by_id(id);
        }
        self.apply_terrain_leveling_for_roads(gl, &ids);
        self.rebuild_all_active_road_meshes(gl);
        self.mark_unsaved(now_ms);
    }

    pub fn set_road_submode(&mut self, gl: &glow::Context, submode: RoadSubMode) {
        if submode != RoadSubMode::Place {
            self.cancel_road_draft(gl);
        }
        self.road_submode = submode;
        self.dragging_road_node = false;
        self.road_drag_moved = false;
        self.welded_drag_peers.clear();
    }

    pub fn set_road_style(&mut self, gl: &glow::Context, style: RoadStyle, now_ms: f64) {
        self.road_style = style;
        if let Some(draft) = &mut self.road_draft {
            draft.preview_key = None;
        }
        if self.road_submode == RoadSubMode::Edit {
            if let Some(rid) = self.selected_road_id {
                self.mutate_selected_road(gl, rid, now_ms, true, |r| {
                    r.style = style;
                });
            }
        } else {
            self.update_road_hover_preview(gl);
        }
    }

    pub fn set_road_material(&mut self, gl: &glow::Context, material: u8, now_ms: f64) {
        let mat = material
            .min(self.max_road_material)
            .min(MAX_ROAD_MATERIALS as u8 - 1);
        self.road_material = mat;
        if let Some(draft) = &mut self.road_draft {
            draft.preview_key = None;
        }
        if self.road_submode == RoadSubMode::Edit {
            if let Some(rid) = self.selected_road_id {
                // Changing texture preserves existing road elevation / terrain poke-through.
                self.mutate_selected_road(gl, rid, now_ms, false, |r| {
                    r.material = mat;
                });
            }
        } else {
            self.update_road_hover_preview(gl);
        }
    }

    pub fn set_road_width(&mut self, gl: &glow::Context, width: f32, now_ms: f64) {
        let w = width.clamp(1.5, 14.0);
        self.road_width = w;
        if let Some(draft) = &mut self.road_draft {
            draft.preview_key = None;
        }
        if self.road_submode == RoadSubMode::Edit {
            if let Some(rid) = self.selected_road_id {
                self.mutate_selected_road(gl, rid, now_ms, true, |r| {
                    r.width = w;
                });
            }
        } else {
            self.update_road_hover_preview(gl);
        }
    }

    pub fn set_road_shoulder(&mut self, gl: &glow::Context, shoulder: f32, now_ms: f64) {
        let s = shoulder.clamp(0.5, 12.0);
        self.road_shoulder = s;
        if self.road_submode == RoadSubMode::Edit {
            if let Some(rid) = self.selected_road_id {
                self.mutate_selected_road(gl, rid, now_ms, true, |r| {
                    r.shoulder = s;
                });
            }
        }
    }

    pub fn finish_road_draft(&mut self, gl: &glow::Context, now_ms: f64) {
        let Some(mut draft) = self.road_draft.take() else {
            return;
        };
        if let Some(gpu) = draft.preview_gpu.take() {
            gpu.destroy(gl);
        }
        if draft.points.len() < 2 {
            self.hover_road_point = None;
            return;
        }

        let owner_tile = if self.active_area.kind == AreaKind::Outside {
            let Some(t) = self.find_owner_tile_for_points(&draft.points, draft.locked_tile) else {
                return;
            };
            Some(t)
        } else {
            None
        };

        let id = self.next_road_id;
        self.next_road_id += 1;
        let mut road = RoadSpline {
            id,
            style: self.road_style,
            material: self.road_material,
            width: self.road_width,
            shoulder: self.road_shoulder,
            points: draft.points,
            profile_heights: Vec::new(),
        };
        self.recompute_road_profile(&mut road);

        if let Some(coord) = owner_tile {
            if let Some(tile) = self.outside_tiles.get_mut(&coord) {
                tile.vis.roads.push(road);
                tile.dirty = true;
            }
        } else if let Some(ins) = &mut self.inside_area {
            ins.vis.roads.push(road);
            ins.dirty = true;
        }

        self.apply_terrain_leveling_for_roads(gl, &[id]);
        self.rebuild_all_active_road_meshes(gl);
        self.selected_road_id = Some(id);
        self.selected_road_node = None;
        self.mark_unsaved(now_ms);
    }

    pub fn cancel_road_draft(&mut self, gl: &glow::Context) {
        if let Some(mut draft) = self.road_draft.take() {
            if let Some(gpu) = draft.preview_gpu.take() {
                gpu.destroy(gl);
            }
        }
        self.hover_road_point = None;
    }

    pub fn relevel_selected_road(&mut self, gl: &glow::Context, now_ms: f64) {
        let Some(rid) = self.selected_road_id else {
            return;
        };
        self.mutate_selected_road(gl, rid, now_ms, true, |_| {});
    }

    pub fn delete_selected_road_node(&mut self, gl: &glow::Context, now_ms: f64) {
        let (Some(rid), Some(nidx)) = (self.selected_road_id, self.selected_road_node) else {
            return;
        };
        let mut deleted = false;
        if self.active_area.kind == AreaKind::Outside {
            if let Some(coord) = self.find_road_owner_tile(rid) {
                if let Some(tile) = self.outside_tiles.get_mut(&coord) {
                    if let Some(road) = tile.vis.roads.iter_mut().find(|r| r.id == rid) {
                        if road.points.len() > 2 && nidx < road.points.len() {
                            road.points.remove(nidx);
                            tile.dirty = true;
                            deleted = true;
                        }
                    }
                }
            }
        } else if let Some(ins) = &mut self.inside_area {
            if let Some(road) = ins.vis.roads.iter_mut().find(|r| r.id == rid) {
                if road.points.len() > 2 && nidx < road.points.len() {
                    road.points.remove(nidx);
                    ins.dirty = true;
                    deleted = true;
                }
            }
        }

        if deleted {
            self.selected_road_node = None;
            self.recompute_road_by_id(rid);
            self.apply_terrain_leveling_for_roads(gl, &[rid]);
            self.rebuild_all_active_road_meshes(gl);
            self.mark_unsaved(now_ms);
        }
    }

    pub fn delete_selected_road(&mut self, gl: &glow::Context, now_ms: f64) {
        let Some(rid) = self.selected_road_id else {
            return;
        };
        let mut removed = false;
        if self.active_area.kind == AreaKind::Outside {
            if let Some(coord) = self.find_road_owner_tile(rid) {
                if let Some(tile) = self.outside_tiles.get_mut(&coord) {
                    let before = tile.vis.roads.len();
                    tile.vis.roads.retain(|r| r.id != rid);
                    if tile.vis.roads.len() != before {
                        if let Some(mesh) = tile.road_meshes.remove(&rid) {
                            mesh.destroy(gl);
                        }
                        tile.dirty = true;
                        removed = true;
                    }
                }
            }
        } else if let Some(ins) = &mut self.inside_area {
            let before = ins.vis.roads.len();
            ins.vis.roads.retain(|r| r.id != rid);
            if ins.vis.roads.len() != before {
                if let Some(mesh) = ins.road_meshes.remove(&rid) {
                    mesh.destroy(gl);
                }
                ins.dirty = true;
                removed = true;
            }
        }

        if removed {
            self.selected_road_id = None;
            self.selected_road_node = None;
            self.dragging_road_node = false;
            self.welded_drag_peers.clear();
            self.rebuild_all_active_road_meshes(gl);
            self.mark_unsaved(now_ms);
        }
    }

    fn mutate_selected_road(
        &mut self,
        gl: &glow::Context,
        road_id: u64,
        now_ms: f64,
        relevel: bool,
        mut f: impl FnMut(&mut RoadSpline),
    ) {
        let mut changed = false;
        if self.active_area.kind == AreaKind::Outside {
            if let Some(coord) = self.find_road_owner_tile(road_id) {
                if let Some(tile) = self.outside_tiles.get_mut(&coord) {
                    if let Some(road) = tile.vis.roads.iter_mut().find(|r| r.id == road_id) {
                        f(road);
                        tile.dirty = true;
                        changed = true;
                    }
                }
            }
        } else if let Some(ins) = &mut self.inside_area {
            if let Some(road) = ins.vis.roads.iter_mut().find(|r| r.id == road_id) {
                f(road);
                ins.dirty = true;
                changed = true;
            }
        }

        if changed {
            if relevel {
                self.recompute_road_by_id(road_id);
                self.apply_terrain_leveling_for_roads(gl, &[road_id]);
            }
            self.rebuild_all_active_road_meshes(gl);
            self.mark_unsaved(now_ms);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_to_tile_border_snaps_within_radius_and_respects_bounds() {
        let tile0 = Some(RectBounds::from_tile(0, 0));

        assert_eq!(snap_to_tile_border(63.35, 32.0, tile0), ([64.0, 32.0], true));
        assert_eq!(snap_to_tile_border(64.6, 32.0, None), ([64.0, 32.0], true));
        assert_eq!(snap_to_tile_border(50.0, 32.0, tile0), ([50.0, 32.0], false));

        // 128.0 border is outside tile (0, 0)'s [0..=64] bounds and must not snap.
        assert_eq!(snap_to_tile_border(127.6, 32.0, tile0), ([127.6, 32.0], false));
    }
}
