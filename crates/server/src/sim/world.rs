use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use shared::character::{CharacterCatalog, CharacterDefinition};
use shared::pathfinding::{WalkGrid, find_path, smooth_path};
use shared::protocol::{PlayerSnapshot, ServerMessage};
use shared::world::{AreaKind, AreaMetadata, InsideKind, InsideSimData, TILE_SIZE, TileSimData};
use tokio::sync::mpsc;

use crate::db::CharacterSaveRecord;

use super::player::PlayerSimState;

/// Configurable Outside simulation radius around each online player (`1` = `3×3` tiles = 9 tiles).
pub const OUTSIDE_SIM_RADIUS: i32 = 1;
pub const CRYO_PRISON_AREA_ID: &str = "cryo_prison";
pub const CRYO_PRISON_SPAWN_POS: [f32; 3] = [7.4, 0.0, 7.4];
pub const CRYO_PRISON_SPAWN_YAW: f32 = 1.50;
pub const PLAYER_MOVE_SPEED: f32 = 4.5;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InsideInstanceId {
    Shared(String),
    Solo {
        area_id: String,
        owner_player_id: u64,
    },
}

impl InsideInstanceId {
    pub fn area_id(&self) -> &str {
        match self {
            Self::Shared(id) | Self::Solo { area_id: id, .. } => id,
        }
    }
}

pub fn inside_instance_for(area_id: &str, player_id: u64) -> InsideInstanceId {
    if area_id == CRYO_PRISON_AREA_ID {
        InsideInstanceId::Solo {
            area_id: area_id.to_owned(),
            owner_player_id: player_id,
        }
    } else {
        InsideInstanceId::Shared(area_id.to_owned())
    }
}

struct OutsideWindowGrid<'a> {
    tiles: &'a HashMap<(String, i32, i32), TileSimData>,
    area_id: &'a str,
    origin_wx: i32,
    origin_wz: i32,
    side: usize,
}

impl<'a> WalkGrid for OutsideWindowGrid<'a> {
    fn width(&self) -> usize {
        self.side
    }

    fn length(&self) -> usize {
        self.side
    }

    fn is_walkable(&self, x: i32, z: i32) -> bool {
        if x < 0 || z < 0 || x >= self.side as i32 || z >= self.side as i32 {
            return false;
        }
        let wx = self.origin_wx + x;
        let wz = self.origin_wz + z;
        let tx = wx.div_euclid(TILE_SIZE as i32);
        let ty = wz.div_euclid(TILE_SIZE as i32);
        let lx = wx.rem_euclid(TILE_SIZE as i32) as usize;
        let lz = wz.rem_euclid(TILE_SIZE as i32) as usize;
        self.tiles
            .get(&(self.area_id.to_owned(), tx, ty))
            .is_some_and(|tile| tile.walkable.get(lx, lz))
    }
}

fn find_nearest_walkable(grid: &dyn WalkGrid, center: (i32, i32), max_radius: i32) -> Option<(i32, i32)> {
    if grid.is_walkable(center.0, center.1) {
        return Some(center);
    }
    let mut best: Option<((i32, i32), i32)> = None;
    for r in 1..=max_radius {
        for dz in -r..=r {
            for dx in -r..=r {
                let cx = center.0 + dx;
                let cz = center.1 + dz;
                if !grid.is_walkable(cx, cz) {
                    continue;
                }
                let d2 = dx * dx + dz * dz;
                if best.is_none_or(|(_, best_d2)| d2 < best_d2) {
                    best = Some(((cx, cz), d2));
                }
            }
        }
        if let Some((cell, _)) = best {
            return Some(cell);
        }
    }
    None
}

fn load_character_catalog(world_dir: &Path) -> CharacterCatalog {
    let path = world_dir.join("characters").join("catalog.json");
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<CharacterCatalog>(&bytes).ok())
        .unwrap_or_default()
}

pub struct SimWorld {
    world_dir: PathBuf,
    character_catalog: CharacterCatalog,
    next_conn_id: u64,
    active_outside: HashMap<(String, i32, i32), TileSimData>,
    active_inside: HashMap<InsideInstanceId, InsideSimData>,
    players: HashMap<u64, PlayerSimState>,
}

impl SimWorld {
    pub fn new(world_dir: PathBuf) -> Self {
        let character_catalog = load_character_catalog(&world_dir);
        Self {
            world_dir,
            character_catalog,
            next_conn_id: 0,
            active_outside: HashMap::new(),
            active_inside: HashMap::new(),
            players: HashMap::new(),
        }
    }

    #[allow(dead_code)]
    pub fn hot_reload_outside_tile(&mut self, area_id: &str, tx: i32, ty: i32, sim: TileSimData) {
        self.active_outside.insert((area_id.to_owned(), tx, ty), sim);
    }

    #[allow(dead_code)]
    pub fn hot_reload_inside(&mut self, area_id: &str, sim: InsideSimData) {
        for (inst, active) in &mut self.active_inside {
            if inst.area_id() == area_id {
                *active = sim.clone();
            }
        }
    }

    fn load_area_meta(&self, area_id: &str) -> AreaMetadata {
        let path = self.world_dir.join("areas").join(area_id).join("area.bin");
        if let Ok(bytes) = fs::read(path) {
            if let Ok(meta) = postcard::from_bytes::<AreaMetadata>(&bytes) {
                return meta;
            }
        }
        if area_id == "main" {
            AreaMetadata {
                id: area_id.to_owned(),
                name: "Main".to_owned(),
                kind: AreaKind::Outside,
                dimensions: None,
            }
        } else {
            AreaMetadata {
                id: area_id.to_owned(),
                name: if area_id == CRYO_PRISON_AREA_ID {
                    "Cryo Prison".to_owned()
                } else {
                    area_id.to_owned()
                },
                kind: AreaKind::Inside(InsideKind::Ephemeral),
                dimensions: Some((24, 20)),
            }
        }
    }

    fn ensure_outside_tile(&mut self, area_id: &str, tx: i32, ty: i32) {
        let key = (area_id.to_owned(), tx, ty);
        if self.active_outside.contains_key(&key) {
            return;
        }
        let sim_path = self
            .world_dir
            .join("areas")
            .join(area_id)
            .join(format!("{tx},{ty}"))
            .join("sim.bin");
        if let Ok(bytes) = fs::read(sim_path) {
            if let Ok(sim) = postcard::from_bytes::<TileSimData>(&bytes) {
                self.active_outside.insert(key, sim);
            }
        }
    }

    fn ensure_inside_area(&mut self, inst: &InsideInstanceId) {
        if self.active_inside.contains_key(inst) {
            return;
        }
        let sim_path = self
            .world_dir
            .join("areas")
            .join(inst.area_id())
            .join("sim.bin");
        if let Ok(bytes) = fs::read(sim_path) {
            if let Ok(sim) = postcard::from_bytes::<InsideSimData>(&bytes) {
                self.active_inside.insert(inst.clone(), sim);
            }
        }
    }

    fn ground_elevation(&self, player_id: u64, area_id: &str, area_kind: AreaKind, pos: [f32; 3]) -> f32 {
        if area_kind == AreaKind::Outside {
            let tx = (pos[0] / TILE_SIZE as f32).floor() as i32;
            let ty = (pos[2] / TILE_SIZE as f32).floor() as i32;
            if let Some(tile) = self.active_outside.get(&(area_id.to_owned(), tx, ty)) {
                let lx = pos[0] - (tx * TILE_SIZE as i32) as f32;
                let lz = pos[2] - (ty * TILE_SIZE as i32) as f32;
                return tile.elevation_at(lx, lz);
            }
        } else {
            let inst = inside_instance_for(area_id, player_id);
            if let Some(sim) = self.active_inside.get(&inst) {
                return sim.elevation_at(pos[0], pos[2]);
            }
        }
        pos[1]
    }

    fn visible_players_for(&self, observer: &PlayerSimState) -> Vec<PlayerSnapshot> {
        let mut out = Vec::new();
        if observer.is_outside() {
            let obs_tx = (observer.pos[0] / TILE_SIZE as f32).floor() as i32;
            let obs_ty = (observer.pos[2] / TILE_SIZE as f32).floor() as i32;
            for p in self.players.values() {
                if p.is_outside() && p.area_id == observer.area_id {
                    let p_tx = (p.pos[0] / TILE_SIZE as f32).floor() as i32;
                    let p_ty = (p.pos[2] / TILE_SIZE as f32).floor() as i32;
                    if (p_tx - obs_tx).abs() <= OUTSIDE_SIM_RADIUS
                        && (p_ty - obs_ty).abs() <= OUTSIDE_SIM_RADIUS
                    {
                        out.push(PlayerSnapshot {
                            player_id: p.player_id,
                            pos: p.pos,
                            yaw: p.yaw,
                            moving: !p.path_waypoints.is_empty(),
                            appearance: p.appearance.clone(),
                        });
                    }
                }
            }
        } else {
            let obs_inst = inside_instance_for(&observer.area_id, observer.player_id);
            for p in self.players.values() {
                if !p.is_outside() && inside_instance_for(&p.area_id, p.player_id) == obs_inst {
                    out.push(PlayerSnapshot {
                        player_id: p.player_id,
                        pos: p.pos,
                        yaw: p.yaw,
                        moving: !p.path_waypoints.is_empty(),
                        appearance: p.appearance.clone(),
                    });
                }
            }
        }
        out.sort_by_key(|p| p.player_id);
        out
    }

    pub fn spawn_player(
        &mut self,
        player_id: u64,
        saved: Option<CharacterSaveRecord>,
        tx_out: mpsc::UnboundedSender<ServerMessage>,
    ) -> (u64, ServerMessage, ServerMessage) {
        self.next_conn_id += 1;
        let conn_id = self.next_conn_id;

        let (area_id, mut pos, yaw, appearance_def, fresh_cryo_wake) = match saved {
            Some(record) if record.area_id != CRYO_PRISON_AREA_ID => (
                record.area_id,
                record.pos,
                record.yaw,
                record.appearance,
                false,
            ),
            _ => (
                CRYO_PRISON_AREA_ID.to_owned(),
                CRYO_PRISON_SPAWN_POS,
                CRYO_PRISON_SPAWN_YAW,
                CharacterDefinition::cryo_awakening_default(),
                true,
            ),
        };

        let meta = self.load_area_meta(&area_id);
        if meta.kind == AreaKind::Outside {
            let tx = (pos[0] / TILE_SIZE as f32).floor() as i32;
            let ty = (pos[2] / TILE_SIZE as f32).floor() as i32;
            for dy in -OUTSIDE_SIM_RADIUS..=OUTSIDE_SIM_RADIUS {
                for dx in -OUTSIDE_SIM_RADIUS..=OUTSIDE_SIM_RADIUS {
                    self.ensure_outside_tile(&area_id, tx + dx, ty + dy);
                }
            }
        } else {
            let inst = inside_instance_for(&area_id, player_id);
            self.ensure_inside_area(&inst);
        }

        pos[1] = self.ground_elevation(player_id, &area_id, meta.kind, pos);

        let appearance = appearance_def
            .to_compact(&self.character_catalog)
            .unwrap_or_default();

        let state = PlayerSimState {
            player_id,
            conn_id,
            area_id: area_id.clone(),
            area_kind: meta.kind,
            pos,
            yaw,
            path_waypoints: VecDeque::new(),
            appearance_def,
            appearance: appearance.clone(),
            tx_out,
        };
        self.players.insert(player_id, state);

        let enter_msg = ServerMessage::EnterArea {
            area_id,
            area_name: meta.name,
            area_kind: meta.kind,
            dimensions: meta.dimensions,
            pos,
            yaw,
            appearance,
            fresh_cryo_wake,
        };
        let snap_players = self
            .players
            .get(&player_id)
            .map(|p| self.visible_players_for(p))
            .unwrap_or_default();
        let snap_msg = ServerMessage::WorldSnapshot {
            players: snap_players,
        };

        (conn_id, enter_msg, snap_msg)
    }

    pub fn remove_player(&mut self, player_id: u64, conn_id: u64) -> Option<PlayerSimState> {
        if self
            .players
            .get(&player_id)
            .is_some_and(|p| p.conn_id == conn_id)
        {
            self.players.remove(&player_id)
        } else {
            None
        }
    }

    pub fn move_player_to(&mut self, player_id: u64, target_x: f32, target_z: f32) {
        if !target_x.is_finite() || !target_z.is_finite() {
            return;
        }
        let Some((area_id, area_kind, pos)) = self
            .players
            .get(&player_id)
            .map(|p| (p.area_id.clone(), p.area_kind, p.pos))
        else {
            return;
        };

        let waypoints: VecDeque<[f32; 2]> = if area_kind == AreaKind::Outside {
            let center_tx = (pos[0] / TILE_SIZE as f32).floor() as i32;
            let center_ty = (pos[2] / TILE_SIZE as f32).floor() as i32;
            let origin_wx = (center_tx - OUTSIDE_SIM_RADIUS) * TILE_SIZE as i32;
            let origin_wz = (center_ty - OUTSIDE_SIM_RADIUS) * TILE_SIZE as i32;
            let side = (OUTSIDE_SIM_RADIUS * 2 + 1) as usize * TILE_SIZE;
            let grid = OutsideWindowGrid {
                tiles: &self.active_outside,
                area_id: &area_id,
                origin_wx,
                origin_wz,
                side,
            };
            let start_raw = (
                (pos[0].floor() as i32 - origin_wx).clamp(0, side as i32 - 1),
                (pos[2].floor() as i32 - origin_wz).clamp(0, side as i32 - 1),
            );
            let Some(start) = find_nearest_walkable(&grid, start_raw, 2) else {
                return;
            };
            let goal_raw = (
                (target_x.floor() as i32 - origin_wx).clamp(0, side as i32 - 1),
                (target_z.floor() as i32 - origin_wz).clamp(0, side as i32 - 1),
            );
            let Some(goal) = find_nearest_walkable(&grid, goal_raw, 2) else {
                return;
            };
            let Some(raw_path) = find_path(&grid, start, goal) else {
                return;
            };
            let smoothed = smooth_path(&grid, &raw_path);
            let final_pt = if goal == goal_raw {
                let min_x = (origin_wx + goal.0) as f32 + 0.15;
                let max_x = (origin_wx + goal.0) as f32 + 0.85;
                let min_z = (origin_wz + goal.1) as f32 + 0.15;
                let max_z = (origin_wz + goal.1) as f32 + 0.85;
                [target_x.clamp(min_x, max_x), target_z.clamp(min_z, max_z)]
            } else {
                [
                    (origin_wx + goal.0) as f32 + 0.5,
                    (origin_wz + goal.1) as f32 + 0.5,
                ]
            };
            build_waypoints(&smoothed, final_pt, origin_wx, origin_wz)
        } else {
            let inst = inside_instance_for(&area_id, player_id);
            let Some(sim) = self.active_inside.get(&inst) else {
                return;
            };
            let w = sim.width as i32;
            let l = sim.length as i32;
            if w <= 0 || l <= 0 {
                return;
            }
            let start_raw = (
                (pos[0].floor() as i32).clamp(0, w - 1),
                (pos[2].floor() as i32).clamp(0, l - 1),
            );
            let Some(start) = find_nearest_walkable(sim, start_raw, 2) else {
                return;
            };
            let goal_raw = (
                (target_x.floor() as i32).clamp(0, w - 1),
                (target_z.floor() as i32).clamp(0, l - 1),
            );
            let Some(goal) = find_nearest_walkable(sim, goal_raw, 2) else {
                return;
            };
            let Some(raw_path) = find_path(sim, start, goal) else {
                return;
            };
            let smoothed = smooth_path(sim, &raw_path);
            let final_pt = if goal == goal_raw {
                [
                    target_x.clamp(0.25, w as f32 - 0.25),
                    target_z.clamp(0.25, l as f32 - 0.25),
                ]
            } else {
                [goal.0 as f32 + 0.5, goal.1 as f32 + 0.5]
            };
            build_waypoints(&smoothed, final_pt, 0, 0)
        };

        if let Some(player) = self.players.get_mut(&player_id) {
            player.path_waypoints = waypoints;
        }
    }

    pub fn tick(&mut self) {
        // Demand-driven 3×3 Outside tile hibernation & Inside instance lifecycle.
        let mut needed_outside: HashSet<(String, i32, i32)> = HashSet::new();
        let mut needed_inside: HashSet<InsideInstanceId> = HashSet::new();

        for player in self.players.values() {
            if player.is_outside() {
                let tx = (player.pos[0] / TILE_SIZE as f32).floor() as i32;
                let ty = (player.pos[2] / TILE_SIZE as f32).floor() as i32;
                for dy in -OUTSIDE_SIM_RADIUS..=OUTSIDE_SIM_RADIUS {
                    for dx in -OUTSIDE_SIM_RADIUS..=OUTSIDE_SIM_RADIUS {
                        needed_outside.insert((player.area_id.clone(), tx + dx, ty + dy));
                    }
                }
            } else {
                needed_inside.insert(inside_instance_for(&player.area_id, player.player_id));
            }
        }

        self.active_outside.retain(|k, _| needed_outside.contains(k));
        self.active_inside.retain(|k, _| needed_inside.contains(k));

        for (area_id, tx, ty) in needed_outside {
            self.ensure_outside_tile(&area_id, tx, ty);
        }
        for inst in needed_inside {
            self.ensure_inside_area(&inst);
        }

        // Advance players along smoothed waypoints (10 Hz -> 0.1s per tick).
        let player_ids: Vec<u64> = self.players.keys().copied().collect();
        for pid in player_ids {
            let Some((area_id, area_kind, mut pos, mut yaw, mut waypoints)) =
                self.players.get_mut(&pid).map(|p| {
                    (
                        p.area_id.clone(),
                        p.area_kind,
                        p.pos,
                        p.yaw,
                        std::mem::take(&mut p.path_waypoints),
                    )
                })
            else {
                continue;
            };

            let mut remaining = PLAYER_MOVE_SPEED * 0.1;
            while remaining > 1e-4 {
                let Some(&next) = waypoints.front() else {
                    break;
                };
                let dx = next[0] - pos[0];
                let dz = next[1] - pos[2];
                let dist = (dx * dx + dz * dz).sqrt();
                if dist > 1e-4 {
                    yaw = dx.atan2(dz);
                }
                if dist <= remaining {
                    pos[0] = next[0];
                    pos[2] = next[1];
                    waypoints.pop_front();
                    remaining -= dist;
                } else {
                    let s = remaining / dist;
                    pos[0] += dx * s;
                    pos[2] += dz * s;
                    remaining = 0.0;
                }
            }
            pos[1] = self.ground_elevation(pid, &area_id, area_kind, pos);

            if let Some(p) = self.players.get_mut(&pid) {
                p.pos = pos;
                p.yaw = yaw;
                p.path_waypoints = waypoints;
            }
        }

        if self.players.is_empty() {
            return;
        }

        let outbound: Vec<(mpsc::UnboundedSender<ServerMessage>, Vec<PlayerSnapshot>)> = self
            .players
            .values()
            .map(|observer| (observer.tx_out.clone(), self.visible_players_for(observer)))
            .collect();

        for (tx_out, players) in outbound {
            let _ = tx_out.send(ServerMessage::WorldSnapshot { players });
        }
    }
}

fn build_waypoints(
    smoothed: &[(i32, i32)],
    final_pt: [f32; 2],
    origin_wx: i32,
    origin_wz: i32,
) -> VecDeque<[f32; 2]> {
    let mut out = VecDeque::new();
    if smoothed.len() <= 1 {
        out.push_back(final_pt);
        return out;
    }
    for &(cx, cz) in &smoothed[1..smoothed.len() - 1] {
        out.push_back([
            (origin_wx + cx) as f32 + 0.5,
            (origin_wz + cz) as f32 + 0.5,
        ]);
    }
    out.push_back(final_pt);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace_world_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("world")
    }

    #[test]
    fn cryo_prison_spawns_isolated_solo_instances_and_walks_path() {
        let world_dir = workspace_world_dir();
        if !world_dir.join("areas").join("cryo_prison").join("sim.bin").exists() {
            return;
        }

        let mut sim = SimWorld::new(world_dir);
        let (tx1, mut rx1) = mpsc::unbounded_channel();
        let (tx2, mut rx2) = mpsc::unbounded_channel();

        let (conn1, enter1, snap1) = sim.spawn_player(101, None, tx1);
        let (_conn2, enter2, snap2) = sim.spawn_player(202, None, tx2);

        match enter1 {
            ServerMessage::EnterArea {
                area_id,
                pos,
                yaw,
                fresh_cryo_wake,
                dimensions,
                ..
            } => {
                assert_eq!(area_id, "cryo_prison");
                assert_eq!(dimensions, Some((24, 20)));
                assert!((pos[0] - CRYO_PRISON_SPAWN_POS[0]).abs() < 1e-4);
                assert!((pos[2] - CRYO_PRISON_SPAWN_POS[2]).abs() < 1e-4);
                assert!((yaw - CRYO_PRISON_SPAWN_YAW).abs() < 1e-4);
                assert!(fresh_cryo_wake);
            }
            other => panic!("unexpected enter message: {other:?}"),
        }
        assert!(matches!(enter2, ServerMessage::EnterArea { fresh_cryo_wake: true, .. }));

        // Each player in cryo_prison must ONLY see themselves (Solo instance isolation).
        match snap1 {
            ServerMessage::WorldSnapshot { players } => {
                assert_eq!(players.len(), 1);
                assert_eq!(players[0].player_id, 101);
            }
            other => panic!("unexpected snap1: {other:?}"),
        }
        match snap2 {
            ServerMessage::WorldSnapshot { players } => {
                assert_eq!(players.len(), 1);
                assert_eq!(players[0].player_id, 202);
            }
            other => panic!("unexpected snap2: {other:?}"),
        }

        // Request a walk across open cryo_prison floor from [7.4, 7.4] to [12.5, 9.5].
        sim.move_player_to(101, 12.5, 9.5);
        for _ in 0..25 {
            sim.tick();
        }

        let mut latest_p1 = None;
        while let Ok(msg) = rx1.try_recv() {
            if let ServerMessage::WorldSnapshot { players } = msg {
                assert_eq!(players.len(), 1);
                assert_eq!(players[0].player_id, 101);
                latest_p1 = Some(players[0].clone());
            }
        }
        let p1 = latest_p1.expect("tick emitted WorldSnapshot");
        assert!((p1.pos[0] - 12.5).abs() < 0.05);
        assert!((p1.pos[2] - 9.5).abs() < 0.05);
        assert!(!p1.moving);

        // Player 202 remained stationary in their own Solo instance.
        let mut latest_p2 = None;
        while let Ok(msg) = rx2.try_recv() {
            if let ServerMessage::WorldSnapshot { players } = msg {
                assert_eq!(players.len(), 1);
                assert_eq!(players[0].player_id, 202);
                latest_p2 = Some(players[0].clone());
            }
        }
        let p2 = latest_p2.expect("p2 snapshot");
        assert!((p2.pos[0] - CRYO_PRISON_SPAWN_POS[0]).abs() < 1e-4);

        // Stale conn_id must not remove player 101; matching conn_id must remove player 101.
        assert!(sim.remove_player(101, conn1 + 999).is_none());
        let removed = sim.remove_player(101, conn1).expect("removes matching conn_id");
        assert_eq!(removed.area_id, "cryo_prison");
    }
}
