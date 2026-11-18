use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use shared::character::{CharacterCatalog, CharacterDefinition};
use shared::entity::{EntityId, EntityVisual};
use shared::pathfinding::{WalkGrid, find_path, smooth_path};
use shared::protocol::{EntitySnapshot, ServerMessage};
use shared::world::{AreaKind, AreaMetadata, InsideKind, InsideSimData, TILE_SIZE, TileSimData};
use tokio::sync::mpsc;

use crate::db::CharacterSaveRecord;

use super::components::{
    CRYO_PRISON_AREA_ID, EntityLocation, InsideInstanceId, MovementComponent, PlayerComponent,
    VisualComponent, inside_instance_for,
};
use super::entity::{Entity, RemovedPlayer};
use super::glue::{SimCommand, SimEvent};

/// Configurable Outside simulation radius around each online player (`1` = `3×3` tiles = 9 tiles).
pub const OUTSIDE_SIM_RADIUS: i32 = 1;
pub const CRYO_PRISON_SPAWN_POS: [f32; 3] = [7.4, 0.0, 7.4];
pub const CRYO_PRISON_SPAWN_YAW: f32 = 1.50;
pub const PLAYER_MOVE_SPEED: f32 = 4.5;

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
    next_entity_id: u64,
    active_outside: HashMap<(String, i32, i32), TileSimData>,
    active_inside: HashMap<InsideInstanceId, InsideSimData>,
    entities: BTreeMap<EntityId, Entity>,
    player_entities: HashMap<u64, EntityId>,
    command_queue: VecDeque<SimCommand>,
    recent_events: Vec<SimEvent>,
}

impl SimWorld {
    pub fn new(world_dir: PathBuf) -> Self {
        let character_catalog = load_character_catalog(&world_dir);
        Self {
            world_dir,
            character_catalog,
            next_conn_id: 0,
            next_entity_id: 0,
            active_outside: HashMap::new(),
            active_inside: HashMap::new(),
            entities: BTreeMap::new(),
            player_entities: HashMap::new(),
            command_queue: VecDeque::new(),
            recent_events: Vec::new(),
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

    pub fn alloc_entity_id(&mut self) -> EntityId {
        self.next_entity_id += 1;
        EntityId::new(self.next_entity_id)
    }

    #[allow(dead_code)]
    pub fn entity(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(&id)
    }

    #[allow(dead_code)]
    pub fn entity_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        self.entities.get_mut(&id)
    }

    #[allow(dead_code)]
    pub fn player_entity_id(&self, account_id: u64) -> Option<EntityId> {
        self.player_entities.get(&account_id).copied()
    }

    #[allow(dead_code)]
    pub fn recent_events(&self) -> &[SimEvent] {
        &self.recent_events
    }

    pub fn enqueue_command(&mut self, cmd: SimCommand) {
        self.command_queue.push_back(cmd);
    }

    pub fn spawn_entity(&mut self, mut entity: Entity) -> EntityId {
        let id = entity.id;
        if let Some(player) = &entity.player {
            let account_id = player.account_id;
            if let Some(old_id) = self.player_entities.remove(&account_id) {
                if self.entities.remove(&old_id).is_some() {
                    self.recent_events
                        .push(SimEvent::EntityDespawned { entity_id: old_id });
                }
            }
            self.ensure_location_loaded(&entity.location, entity.transform.pos);
            self.player_entities.insert(account_id, id);
        }
        entity.transform.pos[1] = self.ground_elevation(&entity.location, entity.transform.pos);
        self.entities.insert(id, entity);
        self.recent_events.push(SimEvent::EntitySpawned { entity_id: id });
        id
    }

    pub fn despawn_entity(&mut self, entity_id: EntityId) -> Option<Entity> {
        let removed = self.entities.remove(&entity_id)?;
        if let Some(player) = &removed.player {
            if self.player_entities.get(&player.account_id) == Some(&entity_id) {
                self.player_entities.remove(&player.account_id);
            }
        }
        self.recent_events
            .push(SimEvent::EntityDespawned { entity_id });
        Some(removed)
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

    fn ensure_location_loaded(&mut self, location: &EntityLocation, pos: [f32; 3]) {
        match location {
            EntityLocation::Outside { area_id } => {
                let tx = (pos[0] / TILE_SIZE as f32).floor() as i32;
                let ty = (pos[2] / TILE_SIZE as f32).floor() as i32;
                for dy in -OUTSIDE_SIM_RADIUS..=OUTSIDE_SIM_RADIUS {
                    for dx in -OUTSIDE_SIM_RADIUS..=OUTSIDE_SIM_RADIUS {
                        self.ensure_outside_tile(area_id, tx + dx, ty + dy);
                    }
                }
            }
            EntityLocation::Inside(inst) => {
                self.ensure_inside_area(inst);
            }
        }
    }

    fn ground_elevation(&self, location: &EntityLocation, pos: [f32; 3]) -> f32 {
        match location {
            EntityLocation::Outside { area_id } => {
                let tx = (pos[0] / TILE_SIZE as f32).floor() as i32;
                let ty = (pos[2] / TILE_SIZE as f32).floor() as i32;
                if let Some(tile) = self.active_outside.get(&(area_id.clone(), tx, ty)) {
                    let lx = pos[0] - (tx * TILE_SIZE as i32) as f32;
                    let lz = pos[2] - (ty * TILE_SIZE as i32) as f32;
                    return tile.elevation_at(lx, lz);
                }
            }
            EntityLocation::Inside(inst) => {
                if let Some(sim) = self.active_inside.get(inst) {
                    return sim.elevation_at(pos[0], pos[2]);
                }
            }
        }
        pos[1]
    }

    fn is_entity_in_active_region(&self, entity: &Entity) -> bool {
        match &entity.location {
            EntityLocation::Outside { area_id } => {
                let tx = (entity.transform.pos[0] / TILE_SIZE as f32).floor() as i32;
                let ty = (entity.transform.pos[2] / TILE_SIZE as f32).floor() as i32;
                self.active_outside.contains_key(&(area_id.clone(), tx, ty))
            }
            EntityLocation::Inside(inst) => self.active_inside.contains_key(inst),
        }
    }

    fn visible_entities_for(&self, observer: &Entity) -> Vec<EntitySnapshot> {
        let mut out = Vec::new();
        match &observer.location {
            EntityLocation::Outside { area_id: obs_area } => {
                let obs_tx = (observer.transform.pos[0] / TILE_SIZE as f32).floor() as i32;
                let obs_ty = (observer.transform.pos[2] / TILE_SIZE as f32).floor() as i32;
                for candidate in self.entities.values() {
                    if let EntityLocation::Outside { area_id } = &candidate.location {
                        if area_id == obs_area {
                            let c_tx = (candidate.transform.pos[0] / TILE_SIZE as f32).floor() as i32;
                            let c_ty = (candidate.transform.pos[2] / TILE_SIZE as f32).floor() as i32;
                            if (c_tx - obs_tx).abs() <= OUTSIDE_SIM_RADIUS
                                && (c_ty - obs_ty).abs() <= OUTSIDE_SIM_RADIUS
                            {
                                if let Some(snap) = candidate.to_snapshot() {
                                    out.push(snap);
                                }
                            }
                        }
                    }
                }
            }
            EntityLocation::Inside(obs_inst) => {
                for candidate in self.entities.values() {
                    if candidate.location.inside_instance() == Some(obs_inst) {
                        if let Some(snap) = candidate.to_snapshot() {
                            out.push(snap);
                        }
                    }
                }
            }
        }
        out
    }

    fn compute_waypoints(
        &self,
        location: &EntityLocation,
        pos: [f32; 3],
        target_x: f32,
        target_z: f32,
    ) -> Option<VecDeque<[f32; 2]>> {
        match location {
            EntityLocation::Outside { area_id } => {
                let center_tx = (pos[0] / TILE_SIZE as f32).floor() as i32;
                let center_ty = (pos[2] / TILE_SIZE as f32).floor() as i32;
                let origin_wx = (center_tx - OUTSIDE_SIM_RADIUS) * TILE_SIZE as i32;
                let origin_wz = (center_ty - OUTSIDE_SIM_RADIUS) * TILE_SIZE as i32;
                let side = (OUTSIDE_SIM_RADIUS * 2 + 1) as usize * TILE_SIZE;
                let grid = OutsideWindowGrid {
                    tiles: &self.active_outside,
                    area_id,
                    origin_wx,
                    origin_wz,
                    side,
                };
                let start_raw = (
                    (pos[0].floor() as i32 - origin_wx).clamp(0, side as i32 - 1),
                    (pos[2].floor() as i32 - origin_wz).clamp(0, side as i32 - 1),
                );
                let start = find_nearest_walkable(&grid, start_raw, 2)?;
                let goal_raw = (
                    (target_x.floor() as i32 - origin_wx).clamp(0, side as i32 - 1),
                    (target_z.floor() as i32 - origin_wz).clamp(0, side as i32 - 1),
                );
                let goal = find_nearest_walkable(&grid, goal_raw, 2)?;
                let raw_path = find_path(&grid, start, goal)?;
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
                Some(build_waypoints(&smoothed, final_pt, origin_wx, origin_wz))
            }
            EntityLocation::Inside(inst) => {
                let sim = self.active_inside.get(inst)?;
                let w = sim.width as i32;
                let l = sim.length as i32;
                if w <= 0 || l <= 0 {
                    return None;
                }
                let start_raw = (
                    (pos[0].floor() as i32).clamp(0, w - 1),
                    (pos[2].floor() as i32).clamp(0, l - 1),
                );
                let start = find_nearest_walkable(sim, start_raw, 2)?;
                let goal_raw = (
                    (target_x.floor() as i32).clamp(0, w - 1),
                    (target_z.floor() as i32).clamp(0, l - 1),
                );
                let goal = find_nearest_walkable(sim, goal_raw, 2)?;
                let raw_path = find_path(sim, start, goal)?;
                let smoothed = smooth_path(sim, &raw_path);
                let final_pt = if goal == goal_raw {
                    [
                        target_x.clamp(0.25, w as f32 - 0.25),
                        target_z.clamp(0.25, l as f32 - 0.25),
                    ]
                } else {
                    [goal.0 as f32 + 0.5, goal.1 as f32 + 0.5]
                };
                Some(build_waypoints(&smoothed, final_pt, 0, 0))
            }
        }
    }

    pub fn process_commands(&mut self) {
        while let Some(cmd) = self.command_queue.pop_front() {
            match cmd {
                SimCommand::MoveTo {
                    entity_id,
                    target_x,
                    target_z,
                } => {
                    if !target_x.is_finite() || !target_z.is_finite() {
                        continue;
                    }
                    let Some((location, pos)) = self.entities.get(&entity_id).and_then(|e| {
                        e.movement
                            .as_ref()
                            .map(|_| (e.location.clone(), e.transform.pos))
                    }) else {
                        continue;
                    };
                    if let Some(waypoints) =
                        self.compute_waypoints(&location, pos, target_x, target_z)
                    {
                        if let Some(entity) = self.entities.get_mut(&entity_id) {
                            if let Some(movement) = &mut entity.movement {
                                movement.waypoints = waypoints;
                            }
                        }
                    }
                }
                SimCommand::StopMovement { entity_id } => {
                    if let Some(entity) = self.entities.get_mut(&entity_id) {
                        if let Some(movement) = &mut entity.movement {
                            movement.clear_path();
                        }
                    }
                }
                SimCommand::Teleport {
                    entity_id,
                    location,
                    mut pos,
                    yaw,
                } => {
                    let is_player = self
                        .entities
                        .get(&entity_id)
                        .is_some_and(|e| e.player.is_some());
                    if is_player {
                        self.ensure_location_loaded(&location, pos);
                    }
                    pos[1] = self.ground_elevation(&location, pos);
                    if let Some(entity) = self.entities.get_mut(&entity_id) {
                        entity.location = location;
                        entity.transform.pos = pos;
                        if let Some(new_yaw) = yaw {
                            entity.transform.yaw = new_yaw;
                        }
                        if let Some(movement) = &mut entity.movement {
                            movement.clear_path();
                        }
                    }
                }
                SimCommand::SetProperty {
                    entity_id,
                    key,
                    value,
                } => {
                    if let Some(entity) = self.entities.get_mut(&entity_id) {
                        entity.properties.set(key, value);
                    }
                }
                SimCommand::Despawn { entity_id } => {
                    self.despawn_entity(entity_id);
                }
            }
        }
    }

    pub fn move_entity_to(&mut self, entity_id: EntityId, target_x: f32, target_z: f32) {
        self.enqueue_command(SimCommand::MoveTo {
            entity_id,
            target_x,
            target_z,
        });
        self.process_commands();
    }

    pub fn spawn_player(
        &mut self,
        account_id: u64,
        saved: Option<CharacterSaveRecord>,
        tx_out: mpsc::UnboundedSender<ServerMessage>,
    ) -> (u64, ServerMessage, ServerMessage) {
        self.next_conn_id += 1;
        let conn_id = self.next_conn_id;

        let (area_id, pos, yaw, appearance_def, fresh_cryo_wake) = match saved {
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
        let location = if meta.kind == AreaKind::Outside {
            EntityLocation::Outside {
                area_id: area_id.clone(),
            }
        } else {
            EntityLocation::Inside(inside_instance_for(&area_id, account_id))
        };

        let appearance = appearance_def
            .to_compact(&self.character_catalog)
            .unwrap_or_default();

        let entity_id = self.alloc_entity_id();
        let entity = Entity::new(entity_id, location, pos, yaw)
            .with_visual(VisualComponent::Character {
                definition: appearance_def,
                compact: appearance.clone(),
            })
            .with_movement(MovementComponent::new(PLAYER_MOVE_SPEED))
            .with_player(PlayerComponent {
                account_id,
                conn_id,
                tx_out,
            });
        self.spawn_entity(entity);

        let snapped_pos = self
            .entities
            .get(&entity_id)
            .map(|e| e.transform.pos)
            .unwrap_or(pos);

        let enter_msg = ServerMessage::EnterArea {
            entity_id,
            area_id,
            area_name: meta.name,
            area_kind: meta.kind,
            dimensions: meta.dimensions,
            pos: snapped_pos,
            yaw,
            visual: EntityVisual::Character(appearance),
            fresh_cryo_wake,
        };
        let snap_entities = self
            .entities
            .get(&entity_id)
            .map(|observer| self.visible_entities_for(observer))
            .unwrap_or_default();
        let snap_msg = ServerMessage::WorldSnapshot {
            entities: snap_entities,
        };

        (conn_id, enter_msg, snap_msg)
    }

    pub fn remove_player(&mut self, account_id: u64, conn_id: u64) -> Option<RemovedPlayer> {
        let entity_id = self.player_entities.get(&account_id).copied()?;
        let matches_conn = self
            .entities
            .get(&entity_id)
            .and_then(|e| e.player.as_ref())
            .is_some_and(|p| p.conn_id == conn_id);
        if !matches_conn {
            return None;
        }
        let removed = self.despawn_entity(entity_id)?;
        let appearance_def = match removed.visual {
            Some(VisualComponent::Character { definition, .. }) => definition,
            _ => CharacterDefinition::cryo_awakening_default(),
        };
        Some(RemovedPlayer {
            entity_id: removed.id,
            account_id,
            area_id: removed.location.area_id().to_owned(),
            pos: removed.transform.pos,
            yaw: removed.transform.yaw,
            appearance_def,
        })
    }

    pub fn move_player_to(&mut self, account_id: u64, target_x: f32, target_z: f32) {
        if let Some(entity_id) = self.player_entities.get(&account_id).copied() {
            self.move_entity_to(entity_id, target_x, target_z);
        }
    }

    pub fn tick(&mut self) {
        self.recent_events.clear();
        self.process_commands();

        // Demand-driven 3×3 Outside tile hibernation & Inside instance lifecycle (anchored by player entities).
        let mut needed_outside: HashSet<(String, i32, i32)> = HashSet::new();
        let mut needed_inside: HashSet<InsideInstanceId> = HashSet::new();

        for entity in self.entities.values() {
            if entity.player.is_none() {
                continue;
            }
            match &entity.location {
                EntityLocation::Outside { area_id } => {
                    let tx = (entity.transform.pos[0] / TILE_SIZE as f32).floor() as i32;
                    let ty = (entity.transform.pos[2] / TILE_SIZE as f32).floor() as i32;
                    for dy in -OUTSIDE_SIM_RADIUS..=OUTSIDE_SIM_RADIUS {
                        for dx in -OUTSIDE_SIM_RADIUS..=OUTSIDE_SIM_RADIUS {
                            needed_outside.insert((area_id.clone(), tx + dx, ty + dy));
                        }
                    }
                }
                EntityLocation::Inside(inst) => {
                    needed_inside.insert(inst.clone());
                }
            }
        }

        self.active_outside.retain(|k, _| needed_outside.contains(k));
        self.active_inside.retain(|k, _| needed_inside.contains(k));

        // Despawn orphaned non-player entities living in solo instances that no longer have an owner online.
        let stale_solo_entities: Vec<EntityId> = self
            .entities
            .values()
            .filter(|e| {
                e.player.is_none()
                    && matches!(
                        &e.location,
                        EntityLocation::Inside(inst @ InsideInstanceId::Solo { .. })
                            if !needed_inside.contains(inst)
                    )
            })
            .map(|e| e.id)
            .collect();
        for id in stale_solo_entities {
            self.despawn_entity(id);
        }

        for (area_id, tx, ty) in needed_outside {
            self.ensure_outside_tile(&area_id, tx, ty);
        }
        for inst in needed_inside {
            self.ensure_inside_area(&inst);
        }

        // Advance movable entities in active regions along smoothed waypoints (10 Hz -> 0.1s per tick).
        let moving_ids: Vec<EntityId> = self
            .entities
            .values()
            .filter(|e| {
                self.is_entity_in_active_region(e)
                    && e.movement.as_ref().is_some_and(|m| m.is_moving())
            })
            .map(|e| e.id)
            .collect();

        for eid in moving_ids {
            let Some((location, orig_pos, mut pos, mut yaw, speed, mut waypoints)) =
                self.entities.get_mut(&eid).and_then(|e| {
                    let movement = e.movement.as_mut()?;
                    Some((
                        e.location.clone(),
                        e.transform.pos,
                        e.transform.pos,
                        e.transform.yaw,
                        movement.speed,
                        std::mem::take(&mut movement.waypoints),
                    ))
                })
            else {
                continue;
            };

            let mut remaining = speed * 0.1;
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
            pos[1] = self.ground_elevation(&location, pos);
            let arrived = waypoints.is_empty();

            if let Some(entity) = self.entities.get_mut(&eid) {
                entity.transform.pos = pos;
                entity.transform.yaw = yaw;
                if let Some(movement) = &mut entity.movement {
                    movement.waypoints = waypoints;
                }
            }

            if pos != orig_pos {
                self.recent_events.push(SimEvent::EntityMoved {
                    entity_id: eid,
                    from: orig_pos,
                    to: pos,
                });
            }
            if arrived {
                self.recent_events.push(SimEvent::EntityArrived {
                    entity_id: eid,
                    pos,
                });
            }
        }

        self.process_commands();

        if self.player_entities.is_empty() {
            return;
        }

        let outbound: Vec<(mpsc::UnboundedSender<ServerMessage>, Vec<EntitySnapshot>)> = self
            .entities
            .values()
            .filter_map(|observer| {
                let player = observer.player.as_ref()?;
                Some((player.tx_out.clone(), self.visible_entities_for(observer)))
            })
            .collect();

        for (tx_out, entities) in outbound {
            let _ = tx_out.send(ServerMessage::WorldSnapshot { entities });
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
    use shared::entity::{EntityCapabilities, PropertyValue};

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

        let p1_entity_id = match enter1 {
            ServerMessage::EnterArea {
                entity_id,
                area_id,
                pos,
                yaw,
                fresh_cryo_wake,
                dimensions,
                visual,
                ..
            } => {
                assert_eq!(area_id, "cryo_prison");
                assert_eq!(dimensions, Some((24, 20)));
                assert!((pos[0] - CRYO_PRISON_SPAWN_POS[0]).abs() < 1e-4);
                assert!((pos[2] - CRYO_PRISON_SPAWN_POS[2]).abs() < 1e-4);
                assert!((yaw - CRYO_PRISON_SPAWN_YAW).abs() < 1e-4);
                assert!(fresh_cryo_wake);
                assert!(matches!(visual, EntityVisual::Character(_)));
                entity_id
            }
            other => panic!("unexpected enter message: {other:?}"),
        };
        let p2_entity_id = match enter2 {
            ServerMessage::EnterArea {
                entity_id,
                fresh_cryo_wake: true,
                ..
            } => entity_id,
            other => panic!("unexpected enter2 message: {other:?}"),
        };
        assert_ne!(p1_entity_id, p2_entity_id);
        assert_eq!(sim.player_entity_id(101), Some(p1_entity_id));
        assert_eq!(sim.player_entity_id(202), Some(p2_entity_id));

        // Each player in cryo_prison must ONLY see themselves (Solo instance isolation).
        match snap1 {
            ServerMessage::WorldSnapshot { entities } => {
                assert_eq!(entities.len(), 1);
                assert_eq!(entities[0].entity_id, p1_entity_id);
                assert!(entities[0]
                    .capabilities
                    .contains(EntityCapabilities::PLAYER | EntityCapabilities::MOVABLE));
            }
            other => panic!("unexpected snap1: {other:?}"),
        }
        match snap2 {
            ServerMessage::WorldSnapshot { entities } => {
                assert_eq!(entities.len(), 1);
                assert_eq!(entities[0].entity_id, p2_entity_id);
            }
            other => panic!("unexpected snap2: {other:?}"),
        }

        // Request a walk across open cryo_prison floor from [7.4, 7.4] to [12.5, 9.5].
        sim.move_player_to(101, 12.5, 9.5);
        let mut saw_arrived = false;
        for _ in 0..25 {
            sim.tick();
            if sim.recent_events().iter().any(|ev| {
                matches!(ev, SimEvent::EntityArrived { entity_id, .. } if *entity_id == p1_entity_id)
            }) {
                saw_arrived = true;
            }
        }
        assert!(saw_arrived, "player entity should emit EntityArrived upon reaching destination");

        let mut latest_p1 = None;
        while let Ok(msg) = rx1.try_recv() {
            if let ServerMessage::WorldSnapshot { entities } = msg {
                assert_eq!(entities.len(), 1);
                assert_eq!(entities[0].entity_id, p1_entity_id);
                latest_p1 = Some(entities[0].clone());
            }
        }
        let p1 = latest_p1.expect("tick emitted WorldSnapshot");
        assert!((p1.pos[0] - 12.5).abs() < 0.05);
        assert!((p1.pos[2] - 9.5).abs() < 0.05);
        assert!(!p1.moving);

        // Player 202 remained stationary in their own Solo instance.
        let mut latest_p2 = None;
        while let Ok(msg) = rx2.try_recv() {
            if let ServerMessage::WorldSnapshot { entities } = msg {
                assert_eq!(entities.len(), 1);
                assert_eq!(entities[0].entity_id, p2_entity_id);
                latest_p2 = Some(entities[0].clone());
            }
        }
        let p2 = latest_p2.expect("p2 snapshot");
        assert!((p2.pos[0] - CRYO_PRISON_SPAWN_POS[0]).abs() < 1e-4);

        // Stale conn_id must not remove player 101; matching conn_id must remove player 101.
        assert!(sim.remove_player(101, conn1 + 999).is_none());
        let removed = sim.remove_player(101, conn1).expect("removes matching conn_id");
        assert_eq!(removed.entity_id, p1_entity_id);
        assert_eq!(removed.account_id, 101);
        assert_eq!(removed.area_id, "cryo_prison");
        assert_eq!(sim.player_entity_id(101), None);
    }

    #[test]
    fn non_player_moving_and_static_entities_share_world_and_snapshots() {
        let world_dir = workspace_world_dir();
        if !world_dir.join("areas").join("cryo_prison").join("sim.bin").exists() {
            return;
        }

        let mut sim = SimWorld::new(world_dir);
        let (tx1, mut rx1) = mpsc::unbounded_channel();
        let (tx2, mut rx2) = mpsc::unbounded_channel();

        let (conn1, enter1, _) = sim.spawn_player(101, None, tx1);
        let (_, _, _) = sim.spawn_player(202, None, tx2);

        let p1_id = match enter1 {
            ServerMessage::EnterArea { entity_id, .. } => entity_id,
            _ => panic!("expected EnterArea"),
        };
        let p1_location = EntityLocation::Inside(inside_instance_for(CRYO_PRISON_AREA_ID, 101));

        // Spawn a static interactable prop entity inside Player 101's solo instance.
        let console_id = sim.alloc_entity_id();
        let console = Entity::new(console_id, p1_location.clone(), [8.5, 0.0, 7.5], 0.0)
            .with_name("Cryo Terminal")
            .with_capabilities(EntityCapabilities::INTERACTABLE)
            .with_property("dialog_id", PropertyValue::String("cryo_wake".to_owned()))
            .with_visual(VisualComponent::Prop {
                asset_id: "local:cryo_terminal".to_owned(),
                scale: [1.0, 1.0, 1.0],
                cast_shadow: true,
            });
        sim.spawn_entity(console);

        // Spawn a moving NPC entity inside Player 101's solo instance.
        let drone_id = sim.alloc_entity_id();
        let drone = Entity::new(drone_id, p1_location, [7.5, 0.0, 7.5], 0.0)
            .with_name("Medical Drone")
            .with_capabilities(EntityCapabilities::INTERACTABLE | EntityCapabilities::QUEST_GIVER)
            .with_visual(VisualComponent::Character {
                definition: CharacterDefinition::cryo_awakening_default(),
                compact: Default::default(),
            })
            .with_movement(MovementComponent::new(4.0));
        sim.spawn_entity(drone);

        // Static entity safely ignores movement requests; moving NPC walks to target.
        sim.move_entity_to(console_id, 11.5, 9.5);
        sim.move_entity_to(drone_id, 11.5, 9.5);

        let mut drone_arrived = false;
        for _ in 0..25 {
            sim.tick();
            if sim.recent_events().iter().any(|ev| {
                matches!(ev, SimEvent::EntityArrived { entity_id, .. } if *entity_id == drone_id)
            }) {
                drone_arrived = true;
            }
        }
        assert!(drone_arrived, "moving NPC entity should reach destination");

        let console_entity = sim.entity(console_id).expect("console still exists");
        assert!((console_entity.transform.pos[0] - 8.5).abs() < 1e-4);
        assert!(!console_entity.is_moving());
        assert_eq!(
            console_entity.properties.get_str("dialog_id"),
            Some("cryo_wake")
        );

        let drone_entity = sim.entity(drone_id).expect("drone still exists");
        assert!((drone_entity.transform.pos[0] - 11.5).abs() < 0.05);
        assert!((drone_entity.transform.pos[2] - 9.5).abs() < 0.05);

        // Player 101 sees themselves + console + drone; Player 202 sees only themselves.
        let mut latest_p1_entities = Vec::new();
        while let Ok(msg) = rx1.try_recv() {
            if let ServerMessage::WorldSnapshot { entities } = msg {
                latest_p1_entities = entities;
            }
        }
        assert_eq!(latest_p1_entities.len(), 3);
        let p1_ids: Vec<EntityId> = latest_p1_entities.iter().map(|e| e.entity_id).collect();
        assert_eq!(p1_ids, vec![p1_id, console_id, drone_id]);

        let console_snap = latest_p1_entities
            .iter()
            .find(|e| e.entity_id == console_id)
            .unwrap();
        assert_eq!(console_snap.name.as_deref(), Some("Cryo Terminal"));
        assert!(console_snap.capabilities.contains(EntityCapabilities::INTERACTABLE));
        assert!(!console_snap.capabilities.contains(EntityCapabilities::MOVABLE));
        assert!(matches!(console_snap.visual, EntityVisual::Prop { .. }));

        let mut latest_p2_entities = Vec::new();
        while let Ok(msg) = rx2.try_recv() {
            if let ServerMessage::WorldSnapshot { entities } = msg {
                latest_p2_entities = entities;
            }
        }
        assert_eq!(latest_p2_entities.len(), 1);

        // Disconnecting Player 101 despawns orphaned solo-instance entities on the next tick.
        assert!(sim.remove_player(101, conn1).is_some());
        sim.tick();
        assert!(sim.entity(console_id).is_none());
        assert!(sim.entity(drone_id).is_none());
    }

    #[test]
    fn reconnecting_same_account_replaces_old_entity_and_ignores_stale_disconnect() {
        let world_dir = workspace_world_dir();
        if !world_dir.join("areas").join("cryo_prison").join("sim.bin").exists() {
            return;
        }

        let mut sim = SimWorld::new(world_dir);
        let (tx1, _rx1) = mpsc::unbounded_channel();
        let (tx2, _rx2) = mpsc::unbounded_channel();

        let (conn1, enter1, _) = sim.spawn_player(555, None, tx1);
        let old_id = match enter1 {
            ServerMessage::EnterArea { entity_id, .. } => entity_id,
            _ => panic!("expected EnterArea"),
        };

        let (conn2, enter2, snap2) = sim.spawn_player(555, None, tx2);
        let new_id = match enter2 {
            ServerMessage::EnterArea { entity_id, .. } => entity_id,
            _ => panic!("expected EnterArea"),
        };

        assert_ne!(old_id, new_id);
        assert!(sim.entity(old_id).is_none(), "old entity must be cleaned up on reconnect");
        assert!(sim.entity(new_id).is_some());
        assert_eq!(sim.player_entity_id(555), Some(new_id));
        match snap2 {
            ServerMessage::WorldSnapshot { entities } => {
                assert_eq!(entities.len(), 1);
                assert_eq!(entities[0].entity_id, new_id);
            }
            _ => panic!("expected WorldSnapshot"),
        }

        // Stale connection disconnect must not remove the replacement entity.
        assert!(sim.remove_player(555, conn1).is_none());
        assert!(sim.entity(new_id).is_some());

        // Matching connection disconnect removes the active entity.
        let removed = sim.remove_player(555, conn2).expect("removes active connection");
        assert_eq!(removed.entity_id, new_id);
        assert!(sim.entity(new_id).is_none());
    }
}
