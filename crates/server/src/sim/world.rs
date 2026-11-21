use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use shared::character::{CharacterCatalog, CharacterDefinition};
use shared::entity::{EntityId, EntityTemplate, EntityVisual, PlacedEntity, VisualConfig};
use shared::pathfinding::{WalkGrid, find_path, smooth_path};
use shared::protocol::{EntitySnapshot, ServerMessage};
use shared::world::{AreaKind, AreaMetadata, InsideKind, InsideSimData, TILE_SIZE, TileSimData};
use tokio::sync::mpsc;
use tracing::info;

use crate::db::CharacterSaveRecord;

use super::components::{
    CRYO_PRISON_AREA_ID, EntityLocation, InsideInstanceId, MovementComponent, PlayerComponent,
    SpawnOrigin, TriggerComponent, VisualComponent, inside_instance_for,
};
use super::entity::{Entity, RemovedPlayer};
use super::glue::{SimCommand, SimEvent, TriggerWay};

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

fn load_character_presets(world_dir: &Path) -> HashMap<String, CharacterDefinition> {
    let dir = world_dir.join("characters").join("presets");
    let mut map = HashMap::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return map;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(def) = serde_json::from_slice::<CharacterDefinition>(&bytes) {
                map.insert(def.id.clone(), def);
            }
        }
    }
    map
}

fn load_entity_templates(world_dir: &Path) -> HashMap<String, EntityTemplate> {
    let dir = world_dir.join("entities").join("templates");
    let mut map = HashMap::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return map;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(tpl) = serde_json::from_slice::<EntityTemplate>(&bytes) {
                map.insert(tpl.id.clone(), tpl);
            }
        }
    }
    map
}

pub struct SimWorld {
    world_dir: PathBuf,
    character_catalog: CharacterCatalog,
    character_presets: HashMap<String, CharacterDefinition>,
    entity_templates: HashMap<String, EntityTemplate>,
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
        let character_presets = load_character_presets(&world_dir);
        let entity_templates = load_entity_templates(&world_dir);
        Self {
            world_dir,
            character_catalog,
            character_presets,
            entity_templates,
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

    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn hot_reload_outside_tile(&mut self, area_id: &str, tx: i32, ty: i32, sim: TileSimData) {
        let entities_to_spawn = sim.entities.clone();
        self.active_outside.insert((area_id.to_owned(), tx, ty), sim);
        let to_despawn: Vec<EntityId> = self
            .entities
            .values()
            .filter(|e| {
                matches!(
                    &e.spawn_origin,
                    Some(SpawnOrigin::OutsideTile { area_id: a, tx: etx, ty: ety, .. })
                        if a == area_id && *etx == tx && *ety == ty
                )
            })
            .map(|e| e.id)
            .collect();
        for id in to_despawn {
            self.despawn_entity(id);
        }
        let location = EntityLocation::Outside {
            area_id: area_id.to_owned(),
        };
        for placed in &entities_to_spawn {
            self.instantiate_placed_entity(
                location.clone(),
                SpawnOrigin::OutsideTile {
                    area_id: area_id.to_owned(),
                    tx,
                    ty,
                    placed_id: placed.id,
                },
                placed,
            );
        }
    }

    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn hot_reload_inside(&mut self, area_id: &str, sim: InsideSimData) {
        let instances: Vec<InsideInstanceId> = self
            .active_inside
            .keys()
            .filter(|inst| inst.area_id() == area_id)
            .cloned()
            .collect();
        for inst in instances {
            let entities_to_spawn = sim.entities.clone();
            self.active_inside.insert(inst.clone(), sim.clone());
            let to_despawn: Vec<EntityId> = self
                .entities
                .values()
                .filter(|e| {
                    matches!(
                        &e.spawn_origin,
                        Some(SpawnOrigin::InsideArea { instance, .. }) if instance == &inst
                    )
                })
                .map(|e| e.id)
                .collect();
            for id in to_despawn {
                self.despawn_entity(id);
            }
            let location = EntityLocation::Inside(inst.clone());
            for placed in &entities_to_spawn {
                self.instantiate_placed_entity(
                    location.clone(),
                    SpawnOrigin::InsideArea {
                        instance: inst.clone(),
                        placed_id: placed.id,
                    },
                    placed,
                );
            }
        }
    }

    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn reload_character_catalog(&mut self, catalog: CharacterCatalog) {
        self.character_catalog = catalog;
        let catalog_ref = &self.character_catalog;
        for entity in self.entities.values_mut() {
            if let Some(VisualComponent::Character {
                definition,
                compact,
            }) = &mut entity.visual
            {
                *compact = definition.to_compact(catalog_ref).unwrap_or_default();
            }
        }
        self.refresh_entities_for_template(None);
    }

    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn upsert_character_preset(&mut self, def: CharacterDefinition) {
        self.character_presets.insert(def.id.clone(), def);
        self.refresh_entities_for_template(None);
    }

    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn remove_character_preset(&mut self, preset_id: &str) {
        self.character_presets.remove(preset_id);
        self.refresh_entities_for_template(None);
    }

    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn upsert_entity_template(&mut self, template: EntityTemplate) {
        let id = template.id.clone();
        self.entity_templates.insert(id.clone(), template);
        self.refresh_entities_for_template(Some(&id));
    }

    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn remove_entity_template(&mut self, template_id: &str) {
        self.entity_templates.remove(template_id);
        self.refresh_entities_for_template(Some(template_id));
    }

    fn find_placed_source(&self, origin: &SpawnOrigin) -> Option<PlacedEntity> {
        let placed_id = origin.placed_id();
        match origin {
            SpawnOrigin::OutsideTile { area_id, tx, ty, .. } => self
                .active_outside
                .get(&(area_id.clone(), *tx, *ty))
                .and_then(|tile| tile.entities.iter().find(|p| p.id == placed_id))
                .cloned(),
            SpawnOrigin::InsideArea { instance, .. } => self
                .active_inside
                .get(instance)
                .and_then(|inside| inside.entities.iter().find(|p| p.id == placed_id))
                .cloned(),
        }
    }

    fn refresh_entities_for_template(&mut self, template_filter: Option<&str>) {
        let targets: Vec<(EntityId, PlacedEntity)> = self
            .entities
            .values()
            .filter_map(|e| {
                let origin = e.spawn_origin.as_ref()?;
                let placed = self.find_placed_source(origin)?;
                if template_filter.is_none_or(|id| id == placed.template_id) {
                    Some((e.id, placed))
                } else {
                    None
                }
            })
            .collect();

        for (eid, placed) in targets {
            let resolved = placed.resolve(self.entity_templates.get(&placed.template_id));
            let new_visual = resolved
                .visual
                .as_ref()
                .map(|v| self.build_visual_component(v));
            let Some(entity) = self.entities.get_mut(&eid) else {
                continue;
            };
            entity.name = Some(resolved.name);
            entity.properties = resolved.properties;
            entity.visual = new_visual;
            entity.movement = resolved.movement.map(|m| {
                let mut mov = entity
                    .movement
                    .take()
                    .unwrap_or_else(|| MovementComponent::new(m.speed));
                mov.speed = m.speed;
                mov
            });
            entity.trigger = resolved.trigger.map(|cfg| {
                let mut trig = entity
                    .trigger
                    .take()
                    .unwrap_or_else(|| TriggerComponent::new(cfg.clone()));
                trig.config = cfg;
                trig
            });
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
        if entity.spawn_origin.is_none() || entity.movement.is_some() {
            entity.transform.pos[1] =
                self.ground_elevation(&entity.location, entity.transform.pos);
        }
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

    fn build_visual_component(&self, cfg: &VisualConfig) -> VisualComponent {
        match cfg {
            VisualConfig::Character { preset_id } => {
                let definition = self
                    .character_presets
                    .get(preset_id)
                    .cloned()
                    .unwrap_or_else(CharacterDefinition::cryo_awakening_default);
                let compact = definition
                    .to_compact(&self.character_catalog)
                    .unwrap_or_default();
                VisualComponent::Character {
                    definition,
                    compact,
                }
            }
            VisualConfig::Prop {
                asset_id,
                scale,
                cast_shadow,
            } => VisualComponent::Prop {
                asset_id: asset_id.clone(),
                scale: *scale,
                cast_shadow: *cast_shadow,
            },
        }
    }

    fn instantiate_placed_entity(
        &mut self,
        location: EntityLocation,
        origin: SpawnOrigin,
        placed: &PlacedEntity,
    ) -> EntityId {
        let resolved = placed.resolve(self.entity_templates.get(&placed.template_id));
        let id = self.alloc_entity_id();
        let mut entity = Entity::new(id, location, placed.position, placed.rotation_y)
            .with_name(resolved.name)
            .with_spawn_origin(origin);
        entity.properties = resolved.properties;
        if let Some(vis) = &resolved.visual {
            entity.visual = Some(self.build_visual_component(vis));
        }
        if let Some(mov) = &resolved.movement {
            entity.movement = Some(MovementComponent::new(mov.speed));
        }
        if let Some(trig) = resolved.trigger {
            entity.trigger = Some(TriggerComponent::new(trig));
        }
        self.spawn_entity(entity)
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
            if let Ok(sim) = TileSimData::from_postcard_bytes(&bytes) {
                let entities_to_spawn = sim.entities.clone();
                self.active_outside.insert(key, sim);
                let location = EntityLocation::Outside {
                    area_id: area_id.to_owned(),
                };
                for placed in &entities_to_spawn {
                    self.instantiate_placed_entity(
                        location.clone(),
                        SpawnOrigin::OutsideTile {
                            area_id: area_id.to_owned(),
                            tx,
                            ty,
                            placed_id: placed.id,
                        },
                        placed,
                    );
                }
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
            if let Ok(sim) = InsideSimData::from_postcard_bytes(&bytes) {
                let entities_to_spawn = sim.entities.clone();
                self.active_inside.insert(inst.clone(), sim);
                let location = EntityLocation::Inside(inst.clone());
                for placed in &entities_to_spawn {
                    self.instantiate_placed_entity(
                        location.clone(),
                        SpawnOrigin::InsideArea {
                            instance: inst.clone(),
                            placed_id: placed.id,
                        },
                        placed,
                    );
                }
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

    fn entities_in_visible_range(a: &Entity, b: &Entity) -> bool {
        match (&a.location, &b.location) {
            (
                EntityLocation::Outside { area_id: area_a },
                EntityLocation::Outside { area_id: area_b },
            ) => {
                if area_a != area_b {
                    return false;
                }
                let a_tx = (a.transform.pos[0] / TILE_SIZE as f32).floor() as i32;
                let a_ty = (a.transform.pos[2] / TILE_SIZE as f32).floor() as i32;
                let b_tx = (b.transform.pos[0] / TILE_SIZE as f32).floor() as i32;
                let b_ty = (b.transform.pos[2] / TILE_SIZE as f32).floor() as i32;
                (b_tx - a_tx).abs() <= OUTSIDE_SIM_RADIUS
                    && (b_ty - a_ty).abs() <= OUTSIDE_SIM_RADIUS
            }
            (EntityLocation::Inside(inst_a), EntityLocation::Inside(inst_b)) => inst_a == inst_b,
            _ => false,
        }
    }

    fn visible_entities_for(&self, observer: &Entity) -> Vec<EntitySnapshot> {
        let mut out = Vec::new();
        for candidate in self.entities.values() {
            if Self::entities_in_visible_range(observer, candidate) {
                if let Some(snap) = candidate.to_snapshot() {
                    out.push(snap);
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

    fn fire_click_trigger(&mut self, actor_id: EntityId, target_id: EntityId, target_pos: [f32; 3]) {
        if let Some(actor) = self.entities.get_mut(&actor_id) {
            if let Some(mov) = &mut actor.movement {
                mov.clear_path();
            }
            if let Some(player) = &mut actor.player {
                player.pending_interaction = None;
            }
            let dx = target_pos[0] - actor.transform.pos[0];
            let dz = target_pos[2] - actor.transform.pos[2];
            if dx * dx + dz * dz > 1e-6 {
                actor.transform.yaw = dx.atan2(dz);
            }
        }
        info!(
            "Trigger fired (click): trigger_entity={} actor_entity={}",
            target_id, actor_id
        );
        self.recent_events.push(SimEvent::EntityTriggered {
            trigger_entity_id: target_id,
            actor_entity_id: actor_id,
            way: TriggerWay::Click,
        });
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
                SimCommand::Interact {
                    actor_id,
                    target_id,
                } => {
                    if actor_id == target_id {
                        continue;
                    }
                    let Some((actor_loc, actor_pos, can_move)) =
                        self.entities.get(&actor_id).and_then(|a| {
                            a.player
                                .as_ref()
                                .map(|_| (a.location.clone(), a.transform.pos, a.movement.is_some()))
                        })
                    else {
                        continue;
                    };
                    let Some((target_pos, target_yaw, trig_cfg)) =
                        self.entities.get(&target_id).and_then(|t| {
                            let actor = self.entities.get(&actor_id)?;
                            if !Self::entities_in_visible_range(actor, t) {
                                return None;
                            }
                            let trig = t.trigger.as_ref()?;
                            trig.config
                                .activation
                                .allows_click()
                                .then(|| (t.transform.pos, t.transform.yaw, trig.config.clone()))
                        })
                    else {
                        continue;
                    };

                    let dist = trig_cfg.horizontal_distance_to_point(
                        target_pos,
                        target_yaw,
                        actor_pos,
                    );
                    if dist <= trig_cfg.interaction_range.max(0.1) {
                        self.fire_click_trigger(actor_id, target_id, target_pos);
                    } else if can_move {
                        let waypoints = self.compute_waypoints(
                            &actor_loc,
                            actor_pos,
                            target_pos[0],
                            target_pos[2],
                        );
                        if let Some(actor) = self.entities.get_mut(&actor_id) {
                            if let Some(wps) = waypoints {
                                if let Some(mov) = &mut actor.movement {
                                    mov.waypoints = wps;
                                }
                                if let Some(player) = &mut actor.player {
                                    player.pending_interaction = Some(target_id);
                                }
                            } else if let Some(player) = &mut actor.player {
                                player.pending_interaction = None;
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
                        if let Some(player) = &mut entity.player {
                            player.pending_interaction = None;
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
                pending_interaction: None,
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
            if let Some(entity) = self.entities.get_mut(&entity_id) {
                if let Some(player) = &mut entity.player {
                    player.pending_interaction = None;
                }
            }
            self.move_entity_to(entity_id, target_x, target_z);
        }
    }

    pub fn interact_player_with(&mut self, account_id: u64, target_id: EntityId) {
        if let Some(actor_id) = self.player_entities.get(&account_id).copied() {
            self.enqueue_command(SimCommand::Interact {
                actor_id,
                target_id,
            });
            self.process_commands();
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

        // Despawn template entities from unloaded tiles/instances & orphaned solo-instance entities.
        let stale_entities: Vec<EntityId> = self
            .entities
            .values()
            .filter(|e| {
                if e.player.is_some() {
                    return false;
                }
                if let Some(origin) = &e.spawn_origin {
                    match origin {
                        SpawnOrigin::OutsideTile { area_id, tx, ty, .. } => {
                            if !self.active_outside.contains_key(&(area_id.clone(), *tx, *ty)) {
                                return true;
                            }
                        }
                        SpawnOrigin::InsideArea { instance, .. } => {
                            if !self.active_inside.contains_key(instance) {
                                return true;
                            }
                        }
                    }
                }
                matches!(
                    &e.location,
                    EntityLocation::Inside(inst @ InsideInstanceId::Solo { .. })
                        if !needed_inside.contains(inst)
                )
            })
            .map(|e| e.id)
            .collect();
        for id in stale_entities {
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

        // Resolve deferred click interactions once players walk into interaction range.
        let pending_actors: Vec<(EntityId, EntityId)> = self
            .entities
            .values()
            .filter_map(|e| Some((e.id, e.player.as_ref()?.pending_interaction?)))
            .collect();
        for (actor_id, target_id) in pending_actors {
            let outcome = (|| {
                let actor = self.entities.get(&actor_id)?;
                let target = self.entities.get(&target_id)?;
                if !Self::entities_in_visible_range(actor, target) {
                    return None;
                }
                let trig = target.trigger.as_ref()?;
                if !trig.config.activation.allows_click() {
                    return None;
                }
                let dist = trig.config.horizontal_distance_to_point(
                    target.transform.pos,
                    target.transform.yaw,
                    actor.transform.pos,
                );
                Some((
                    dist <= trig.config.interaction_range.max(0.1),
                    actor.is_moving(),
                    target.transform.pos,
                ))
            })();
            match outcome {
                Some((true, _, target_pos)) => {
                    self.fire_click_trigger(actor_id, target_id, target_pos);
                }
                Some((false, true, _)) => {}
                Some((false, false, _)) | None => {
                    if let Some(actor) = self.entities.get_mut(&actor_id) {
                        if let Some(player) = &mut actor.player {
                            player.pending_interaction = None;
                        }
                    }
                }
            }
        }

        // Evaluate Walk-Into trigger volumes against active players.
        let player_positions: Vec<(EntityId, EntityLocation, [f32; 3])> = self
            .entities
            .values()
            .filter(|e| e.player.is_some())
            .map(|e| (e.id, e.location.clone(), e.transform.pos))
            .collect();
        let walk_triggers: Vec<EntityId> = self
            .entities
            .values()
            .filter(|e| {
                self.is_entity_in_active_region(e)
                    && e.trigger
                        .as_ref()
                        .is_some_and(|t| t.config.activation.allows_walk_into())
            })
            .map(|e| e.id)
            .collect();

        for trig_id in walk_triggers {
            let Some((trig_loc, trig_pos, trig_yaw, trig_cfg)) =
                self.entities.get(&trig_id).and_then(|e| {
                    let trig = e.trigger.as_ref()?;
                    Some((
                        e.location.clone(),
                        e.transform.pos,
                        e.transform.yaw,
                        trig.config.clone(),
                    ))
                })
            else {
                continue;
            };

            let inside_now: BTreeSet<EntityId> = player_positions
                .iter()
                .filter(|(pid, ploc, ppos)| {
                    if *pid == trig_id {
                        return false;
                    }
                    let same_space = match (&trig_loc, ploc) {
                        (
                            EntityLocation::Outside { area_id: a },
                            EntityLocation::Outside { area_id: b },
                        ) => a == b,
                        (EntityLocation::Inside(a), EntityLocation::Inside(b)) => a == b,
                        _ => false,
                    };
                    same_space && trig_cfg.contains_point(trig_pos, trig_yaw, *ppos)
                })
                .map(|(pid, _, _)| *pid)
                .collect();

            let mut newly_entered = Vec::new();
            if let Some(trig_entity) = self.entities.get_mut(&trig_id) {
                if let Some(trig) = &mut trig_entity.trigger {
                    trig.occupants.retain(|pid| inside_now.contains(pid));
                    for pid in inside_now {
                        if trig.occupants.insert(pid) {
                            newly_entered.push(pid);
                        }
                    }
                }
            }

            for actor_entity_id in newly_entered {
                info!(
                    "Trigger fired (walk_into): trigger_entity={} actor_entity={}",
                    trig_id, actor_entity_id
                );
                self.recent_events.push(SimEvent::EntityTriggered {
                    trigger_entity_id: trig_id,
                    actor_entity_id,
                    way: TriggerWay::WalkInto,
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
    use shared::entity::{
        ComponentOverride, EntityCapabilities, EntityOverrides, MovementConfig, PropertyBag,
        PropertyValue, TriggerActivation, TriggerConfig,
    };
    use shared::world::InsideVisData;

    fn workspace_world_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("world")
    }

    fn unique_temp_world(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ff_world_{label}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("areas").join("test_room")).unwrap();
        let meta = AreaMetadata {
            id: "test_room".to_owned(),
            name: "Test Room".to_owned(),
            kind: AreaKind::Inside(InsideKind::Ephemeral),
            dimensions: Some((20, 20)),
        };
        fs::write(
            dir.join("areas").join("test_room").join("area.bin"),
            postcard::to_allocvec(&meta).unwrap(),
        )
        .unwrap();
        let vis = InsideVisData::flat_default(20, 20);
        fs::write(
            dir.join("areas").join("test_room").join("vis.bin"),
            postcard::to_allocvec(&vis).unwrap(),
        )
        .unwrap();
        dir
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
        assert!(matches!(
            console_snap.visual,
            Some(EntityVisual::Prop { .. })
        ));

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

    #[test]
    fn template_spawning_inherits_live_edits_with_overrides_and_fires_click_and_walk_triggers() {
        let world_dir = unique_temp_world("templates_and_triggers");

        let mut props = PropertyBag::new();
        props.set("role", PropertyValue::String("terminal".to_owned()));
        props.set("charges", PropertyValue::Int(3));

        let mut tpl = EntityTemplate {
            id: "portal_pad".to_owned(),
            name: "Portal Pad".to_owned(),
            properties: props,
            visual: None,
            movement: None,
            trigger: Some(TriggerConfig {
                size: [2.0, 2.0, 2.0],
                offset: [0.0, 0.0, 0.0],
                yaw_deg: 0.0,
                activation: TriggerActivation::Both,
                interaction_range: 1.5,
            }),
        };
        fs::create_dir_all(world_dir.join("entities").join("templates")).unwrap();
        fs::write(
            world_dir.join("entities/templates/portal_pad.json"),
            serde_json::to_vec(&tpl).unwrap(),
        )
        .unwrap();

        let mut inside_sim = InsideSimData::flat_default(20, 20);
        // Instance 1: pure inheritance (at [12.0, 0.0, 6.0])
        inside_sim.entities.push(PlacedEntity {
            id: 1,
            template_id: "portal_pad".to_owned(),
            position: [12.0, 0.0, 6.0],
            rotation_y: 0.0,
            overrides: EntityOverrides::default(),
        });
        // Instance 2: customized name + property + WalkInto-only trigger (at [8.0, 0.0, 12.0])
        let mut custom_ov = EntityOverrides::default();
        custom_ov.name = Some("Custom Pad".to_owned());
        custom_ov
            .properties
            .set("charges", PropertyValue::Int(9));
        custom_ov.trigger = ComponentOverride::Set(TriggerConfig {
            size: [2.0, 2.0, 2.0],
            offset: [0.0, 0.0, 0.0],
            yaw_deg: 0.0,
            activation: TriggerActivation::WalkInto,
            interaction_range: 1.5,
        });
        inside_sim.entities.push(PlacedEntity {
            id: 2,
            template_id: "portal_pad".to_owned(),
            position: [8.0, 0.0, 12.0],
            rotation_y: 0.0,
            overrides: custom_ov,
        });
        fs::write(
            world_dir.join("areas").join("test_room").join("sim.bin"),
            postcard::to_allocvec(&inside_sim).unwrap(),
        )
        .unwrap();

        let mut sim = SimWorld::new(world_dir.clone());
        let (tx, _rx) = mpsc::unbounded_channel();
        let saved = CharacterSaveRecord {
            account_id: 77,
            area_id: "test_room".to_owned(),
            pos: [4.0, 0.0, 6.0],
            yaw: 0.0,
            appearance: CharacterDefinition::cryo_awakening_default(),
        };
        let (conn_id, enter_msg, snap_msg) = sim.spawn_player(77, Some(saved), tx);
        let player_eid = match enter_msg {
            ServerMessage::EnterArea { entity_id, .. } => entity_id,
            _ => panic!("expected EnterArea"),
        };

        // Player + Instance 1 (clickable invisible trigger) appear in snapshot; Instance 2 is WalkInto+invisible so not in client snapshot.
        let snap_entities = match snap_msg {
            ServerMessage::WorldSnapshot { entities } => entities,
            _ => panic!("expected WorldSnapshot"),
        };
        assert_eq!(snap_entities.len(), 2);
        let pad1_eid = snap_entities
            .iter()
            .find(|e| e.entity_id != player_eid)
            .unwrap()
            .entity_id;

        let pad2_eid = sim
            .entities
            .values()
            .find(|e| e.name.as_deref() == Some("Custom Pad"))
            .expect("custom pad spawned")
            .id;

        // Edit the template live: name, property, and add MovementComponent!
        tpl.name = "Updated Pad".to_owned();
        tpl.properties.set("charges", PropertyValue::Int(5));
        tpl.movement = Some(MovementConfig { speed: 2.5 });
        sim.upsert_entity_template(tpl);

        let pad1 = sim.entity(pad1_eid).unwrap();
        assert_eq!(pad1.name.as_deref(), Some("Updated Pad"));
        assert_eq!(pad1.properties.get_int("charges"), Some(5));
        assert_eq!(pad1.movement.as_ref().map(|m| m.speed), Some(2.5));

        // Instance 2 kept its overridden name & charges while inheriting the new MovementComponent!
        let pad2 = sim.entity(pad2_eid).unwrap();
        assert_eq!(pad2.name.as_deref(), Some("Custom Pad"));
        assert_eq!(pad2.properties.get_int("charges"), Some(9));
        assert_eq!(pad2.movement.as_ref().map(|m| m.speed), Some(2.5));

        // Clicking pad1 from far away walks the player into interaction range and fires Click trigger!
        sim.interact_player_with(77, pad1_eid);
        let mut click_fired = false;
        for _ in 0..30 {
            sim.tick();
            if sim.recent_events().iter().any(|ev| {
                matches!(
                    ev,
                    SimEvent::EntityTriggered {
                        trigger_entity_id,
                        actor_entity_id,
                        way: TriggerWay::Click,
                    } if *trigger_entity_id == pad1_eid && *actor_entity_id == player_eid
                )
            }) {
                click_fired = true;
            }
        }
        assert!(click_fired, "click trigger should fire once player walks into range");

        // Walking through pad2 fires WalkInto trigger once on entry and again only after exit + re-entry!
        sim.move_player_to(77, 8.0, 12.0);
        let mut walk_count = 0usize;
        for _ in 0..25 {
            sim.tick();
            walk_count += sim
                .recent_events()
                .iter()
                .filter(|ev| {
                    matches!(
                        ev,
                        SimEvent::EntityTriggered {
                            trigger_entity_id,
                            actor_entity_id,
                            way: TriggerWay::WalkInto,
                        } if *trigger_entity_id == pad2_eid && *actor_entity_id == player_eid
                    )
                })
                .count();
        }
        assert_eq!(walk_count, 1, "walk-into trigger should fire once while standing inside");

        // Walk out, then walk back in -> fires a second time.
        sim.move_player_to(77, 3.0, 12.0);
        for _ in 0..20 {
            sim.tick();
        }
        sim.move_player_to(77, 8.0, 12.0);
        for _ in 0..20 {
            sim.tick();
            walk_count += sim
                .recent_events()
                .iter()
                .filter(|ev| {
                    matches!(
                        ev,
                        SimEvent::EntityTriggered {
                            trigger_entity_id,
                            actor_entity_id,
                            way: TriggerWay::WalkInto,
                        } if *trigger_entity_id == pad2_eid && *actor_entity_id == player_eid
                    )
                })
                .count();
        }
        assert_eq!(walk_count, 2, "re-entering walk-into trigger should fire again");

        assert!(sim.remove_player(77, conn_id).is_some());
        let _ = fs::remove_dir_all(&world_dir);
    }
}
