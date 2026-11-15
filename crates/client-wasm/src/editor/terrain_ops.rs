use glam::Vec3;
use shared::world::{AreaKind, TILE_SIZE, TILE_VERTICES, TileVisData};

use super::state::EditorState;

impl EditorState {
    pub(super) fn raycast_terrain(&self, orig: Vec3, dir: Vec3) -> Option<Vec3> {
        let mut t = 0.5f32;
        let step = 0.75f32;
        let mut prev_t: Option<f32> = None;
        let mut prev_above = false;

        while t <= 320.0 {
            let p = orig + dir * t;
            if let Some(h) = self.terrain_height_at(p.x, p.z) {
                let above = p.y >= h;
                if let Some(pt) = prev_t {
                    if prev_above && !above {
                        let mut lo = pt;
                        let mut hi = t;
                        for _ in 0..8 {
                            let mid = 0.5 * (lo + hi);
                            let mp = orig + dir * mid;
                            if let Some(mh) = self.terrain_height_at(mp.x, mp.z) {
                                if mp.y >= mh {
                                    lo = mid;
                                } else {
                                    hi = mid;
                                }
                            }
                        }
                        let hit = orig + dir * (0.5 * (lo + hi));
                        let hy = self.terrain_height_at(hit.x, hit.z).unwrap_or(hit.y);
                        return Some(Vec3::new(hit.x, hy, hit.z));
                    }
                }
                prev_t = Some(t);
                prev_above = above;
            }
            t += step;
        }
        None
    }

    pub fn terrain_height_at(&self, wx: f32, wz: f32) -> Option<f32> {
        if self.active_area.kind == AreaKind::Outside {
            let tx = (wx / TILE_SIZE as f32).floor() as i32;
            let ty = (wz / TILE_SIZE as f32).floor() as i32;
            let tile = self.outside_tiles.get(&(tx, ty))?;
            let lx = (wx - (tx * TILE_SIZE as i32) as f32).clamp(0.0, TILE_SIZE as f32 - 0.0001);
            let lz = (wz - (ty * TILE_SIZE as i32) as f32).clamp(0.0, TILE_SIZE as f32 - 0.0001);
            Some(bilerp_height(&tile.vis.heights, TILE_VERTICES, lx, lz))
        } else if let Some(inside) = &self.inside_area {
            let w = inside.vis.width as f32;
            let l = inside.vis.length as f32;
            if wx < 0.0 || wz < 0.0 || wx >= w || wz >= l {
                return None;
            }
            Some(bilerp_height(
                &inside.vis.heights,
                inside.vis.width as usize + 1,
                wx,
                wz,
            ))
        } else {
            None
        }
    }

    pub(super) fn sample_vertex_height(&self, wx: i32, wz: i32) -> Option<f32> {
        if self.active_area.kind == AreaKind::Outside {
            // Check canonical tile owning cell/vertex first, then fall back to border neighbor
            let tx = wx.div_euclid(TILE_SIZE as i32);
            let ty = wz.div_euclid(TILE_SIZE as i32);
            let vx = wx.rem_euclid(TILE_SIZE as i32) as usize;
            let vz = wz.rem_euclid(TILE_SIZE as i32) as usize;
            if let Some(tile) = self.outside_tiles.get(&(tx, ty)) {
                return Some(tile.vis.heights[vz * TILE_VERTICES + vx]);
            }
            // Handle right/bottom boundary vertices (vx == 64 or vz == 64)
            let alt_tx = if vx == 0 { tx - 1 } else { tx };
            let alt_ty = if vz == 0 { ty - 1 } else { ty };
            for (cand_tx, cand_ty) in [(alt_tx, ty), (tx, alt_ty), (alt_tx, alt_ty)] {
                if let Some(tile) = self.outside_tiles.get(&(cand_tx, cand_ty)) {
                    let avx = (wx - cand_tx * TILE_SIZE as i32) as usize;
                    let avz = (wz - cand_ty * TILE_SIZE as i32) as usize;
                    if avx < TILE_VERTICES && avz < TILE_VERTICES {
                        return Some(tile.vis.heights[avz * TILE_VERTICES + avx]);
                    }
                }
            }
            None
        } else if let Some(inside) = &self.inside_area {
            let w = inside.vis.width as i32;
            let l = inside.vis.length as i32;
            if wx < 0 || wz < 0 || wx > w || wz > l {
                return None;
            }
            Some(inside.vis.heights[wz as usize * (w as usize + 1) + wx as usize])
        } else {
            None
        }
    }

    pub(super) fn copy_loaded_borders_into(&self, tx: i32, ty: i32, vis: &mut TileVisData) {
        let ox = tx * TILE_SIZE as i32;
        let oz = ty * TILE_SIZE as i32;
        for vz in 0..TILE_VERTICES {
            for vx in 0..TILE_VERTICES {
                if vx == 0 || vx == TILE_SIZE || vz == 0 || vz == TILE_SIZE {
                    if let Some(h) = self.sample_vertex_height(ox + vx as i32, oz + vz as i32) {
                        vis.heights[vz * TILE_VERTICES + vx] = h;
                    }
                }
            }
        }
    }

    pub(super) fn refresh_outside_tile_meshes(&self, gl: &glow::Context, tiles: &[(i32, i32)]) {
        for &(tx, ty) in tiles {
            let Some(tile) = self.outside_tiles.get(&(tx, ty)) else {
                continue;
            };
            let ox = tx * TILE_SIZE as i32;
            let oz = ty * TILE_SIZE as i32;
            let mut normals = Vec::with_capacity(TILE_VERTICES * TILE_VERTICES);

            for vz in 0..TILE_VERTICES {
                for vx in 0..TILE_VERTICES {
                    let wx = ox + vx as i32;
                    let wz = oz + vz as i32;
                    let hc = tile.vis.heights[vz * TILE_VERTICES + vx];
                    let hl = self.sample_vertex_height(wx - 1, wz).unwrap_or(hc);
                    let hr = self.sample_vertex_height(wx + 1, wz).unwrap_or(hc);
                    let hd = self.sample_vertex_height(wx, wz - 1).unwrap_or(hc);
                    let hu = self.sample_vertex_height(wx, wz + 1).unwrap_or(hc);
                    let n = Vec3::new(hl - hr, 2.0, hd - hu).normalize_or_zero();
                    normals.push(n.to_array());
                }
            }
            tile.gpu
                .upload_vertices(gl, ox as f32, oz as f32, &tile.vis.heights, &normals);
        }
    }

    pub(super) fn refresh_inside_mesh(&self, gl: &glow::Context) {
        let Some(inside) = &self.inside_area else {
            return;
        };
        let w = inside.vis.width as usize;
        let l = inside.vis.length as usize;
        let vw = w + 1;
        let vh = l + 1;
        let mut normals = Vec::with_capacity(vw * vh);
        for vz in 0..vh {
            for vx in 0..vw {
                let hc = inside.vis.heights[vz * vw + vx];
                let hl = if vx > 0 { inside.vis.heights[vz * vw + (vx - 1)] } else { hc };
                let hr = if vx < w { inside.vis.heights[vz * vw + (vx + 1)] } else { hc };
                let hd = if vz > 0 { inside.vis.heights[(vz - 1) * vw + vx] } else { hc };
                let hu = if vz < l { inside.vis.heights[(vz + 1) * vw + vx] } else { hc };
                let n = Vec3::new(hl - hr, 2.0, hd - hu).normalize_or_zero();
                normals.push(n.to_array());
            }
        }
        inside
            .gpu
            .upload_vertices(gl, 0.0, 0.0, &inside.vis.heights, &normals);
    }
}

fn bilerp_height(heights: &[f32], stride: usize, lx: f32, lz: f32) -> f32 {
    let ix = lx.floor() as usize;
    let iz = lz.floor() as usize;
    let fx = lx - ix as f32;
    let fz = lz - iz as f32;
    let i00 = iz * stride + ix;
    let h00 = heights.get(i00).copied().unwrap_or(0.0);
    let h10 = heights.get(i00 + 1).copied().unwrap_or(h00);
    let h01 = heights.get(i00 + stride).copied().unwrap_or(h00);
    let h11 = heights.get(i00 + stride + 1).copied().unwrap_or(h00);
    let h0 = h00 + (h10 - h00) * fx;
    let h1 = h01 + (h11 - h01) * fx;
    h0 + (h1 - h0) * fz
}
