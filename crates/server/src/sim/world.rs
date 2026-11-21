use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use shared::character::{CharacterCatalog, CharacterDefinition};
use shared::entity::{
    EntityId, EntityTemplate, EntityVisual, InventoryConfig, InventoryEntry, PlacedEntity,
    VisualConfig,
};
use shared::item::{ItemCatalog, ItemDef, ItemEquipBinding};
use shared::pathfinding::{WalkGrid, find_path, smooth_path};
use shared::protocol::{EntitySnapshot, LootItemEntry, ServerMessage};
use shared::world::{
    AreaKind, AreaMetadata, InsideKind, InsideSimData, TILE_SIZE, TileSimData,
    prettify_asset_slug,
};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::db::CharacterSaveRecord;

use super::components::{
    CRYO_PRISON_AREA_ID, EntityLocation, InsideInstanceId, InventoryComponent, LootableComponent,
    MovementComponent, PlayerComponent, ScriptComponent, SpawnOrigin, TriggerComponent,
    VisualComponent, inside_instance_for,
};
use super::entity::{Entity, RemovedPlayer};
use super::glue::{SimCommand, SimEvent, TriggerWay};
use super::scripting::{ScriptTriggerInput, execute_trigger_script};

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

fn load_item_catalog(world_dir: &Path) -> ItemCatalog {
    let path = world_dir.join("items").join("catalog.json");
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<ItemCatalog>(&bytes).ok())
        .unwrap_or_default()
}

pub struct SimWorld {
    world_dir: PathBuf,
    character_catalog: CharacterCatalog,
    character_presets: HashMap<String, CharacterDefinition>,
    item_catalog: ItemCatalog,
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
        let item_catalog = load_item_catalog(&world_dir);
        let entity_templates = load_entity_templates(&world_dir);
        Self {
            world_dir,
            character_catalog,
            character_presets,
            item_catalog,
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
                pose_clip,
            }) = &mut entity.visual
            {
                *compact = definition
                    .to_compact_with_pose(catalog_ref, pose_clip.clone())
                    .unwrap_or_default();
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
            entity.script = resolved.script.map(ScriptComponent::new);
            entity.inventory = resolved
                .inventory
                .as_ref()
                .map(InventoryComponent::from_config);
            entity.lootable = resolved.lootable.map(LootableComponent::new);
            apply_equipped_inventory_to_visual(entity, &self.item_catalog, &self.character_catalog);
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
            VisualConfig::Character {
                preset_id,
                pose_clip,
            } => {
                let definition = self
                    .character_presets
                    .get(preset_id)
                    .cloned()
                    .unwrap_or_else(CharacterDefinition::cryo_awakening_default);
                let compact = definition
                    .to_compact_with_pose(&self.character_catalog, pose_clip.clone())
                    .unwrap_or_default();
                VisualComponent::Character {
                    definition,
                    compact,
                    pose_clip: pose_clip.clone(),
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
        if let Some(script) = resolved.script {
            entity.script = Some(ScriptComponent::new(script));
        }
        if let Some(inv) = &resolved.inventory {
            entity.inventory = Some(InventoryComponent::from_config(inv));
        }
        if let Some(loot) = resolved.lootable {
            entity.lootable = Some(LootableComponent::new(loot));
        }
        apply_equipped_inventory_to_visual(&mut entity, &self.item_catalog, &self.character_catalog);
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
        if self
            .entities
            .get(&target_id)
            .is_some_and(|t| t.lootable.is_some())
        {
            self.send_loot_window_to_player(actor_id, target_id);
        }
        self.run_trigger_script(target_id, actor_id, TriggerWay::Click);
    }

    fn send_loot_window_to_player(&self, actor_id: EntityId, target_id: EntityId) {
        let Some(tx_out) = self
            .entities
            .get(&actor_id)
            .and_then(|a| a.player.as_ref().map(|p| p.tx_out.clone()))
        else {
            return;
        };
        let Some(target) = self.entities.get(&target_id) else {
            return;
        };
        let Some(lootable) = &target.lootable else {
            return;
        };
        let title = lootable
            .config
            .title
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| target.name.clone())
            .unwrap_or_else(|| "Loot".to_owned());
        let description = lootable
            .config
            .description
            .clone()
            .unwrap_or_default();
        let items: Vec<LootItemEntry> = target
            .inventory
            .as_ref()
            .map(|inv| {
                inv.items
                    .iter()
                    .filter(|e| e.count > 0)
                    .map(|entry| {
                        let def = self.item_catalog.get(&entry.item_id);
                        LootItemEntry {
                            item_id: entry.item_id.clone(),
                            name: def
                                .map(|d| d.name.clone())
                                .unwrap_or_else(|| prettify_asset_slug(&entry.item_id)),
                            description: def.map(|d| d.description.clone()).unwrap_or_default(),
                            category: def.map(|d| d.category.clone()).unwrap_or_default(),
                            count: entry.count,
                            equipped: entry.equipped,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let _ = tx_out.send(ServerMessage::LootWindow {
            entity_id: target_id,
            title,
            description,
            items,
        });
    }

    fn broadcast_snapshots_for_entity_change(&self, changed_id: EntityId) {
        let Some(changed) = self.entities.get(&changed_id) else {
            return;
        };
        for observer in self.entities.values() {
            let Some(player) = observer.player.as_ref() else {
                continue;
            };
            if Self::entities_in_visible_range(observer, changed) {
                let _ = player.tx_out.send(ServerMessage::WorldSnapshot {
                    entities: self.visible_entities_for(observer),
                });
            }
        }
    }

    fn run_trigger_script(&mut self, trigger_id: EntityId, actor_id: EntityId, way: TriggerWay) {
        let Some((js_source, input)) = (|| {
            let trig_ent = self.entities.get(&trigger_id)?;
            let script = trig_ent.script.as_ref()?;
            let actor = self.entities.get(&actor_id)?;
            let js_source = script.config.effective_js().to_owned();
            let input = ScriptTriggerInput {
                trigger_entity_id: trigger_id,
                trigger_entity_name: trig_ent.name.clone().unwrap_or_else(|| format!("Entity {trigger_id}")),
                trigger_pos: trig_ent.transform.pos,
                trigger_props: trig_ent.properties.clone(),
                actor_entity_id: actor_id,
                actor_pos: actor.transform.pos,
                actor_props: actor.properties.clone(),
                actor_is_player: actor.player.is_some(),
                area_id: trig_ent.location.area_id().to_owned(),
                way,
            };
            Some((js_source, input))
        })() else {
            return;
        };

        match execute_trigger_script(&js_source, input) {
            Ok(out) => {
                for cmd in out.commands {
                    self.enqueue_command(cmd);
                }
            }
            Err(message) => {
                warn!("Trigger script error on entity {}: {}", trigger_id, message);
                self.recent_events.push(SimEvent::ScriptError {
                    entity_id: trigger_id,
                    message,
                });
            }
        }
    }

    fn remove_from_trigger_occupants(&mut self, entity_id: EntityId) {
        for entity in self.entities.values_mut() {
            if let Some(trig) = &mut entity.trigger {
                trig.occupants.remove(&entity_id);
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
                            let trig_cfg = t.effective_click_trigger()?;
                            Some((t.transform.pos, t.transform.yaw, trig_cfg))
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
                SimCommand::LootTake {
                    actor_id,
                    target_id,
                    item_id,
                } => {
                    self.execute_loot_take(actor_id, target_id, Some(&item_id));
                }
                SimCommand::LootTakeAll {
                    actor_id,
                    target_id,
                } => {
                    self.execute_loot_take(actor_id, target_id, None);
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
                    self.remove_from_trigger_occupants(entity_id);
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
                SimCommand::RemoveProperty { entity_id, key } => {
                    if let Some(entity) = self.entities.get_mut(&entity_id) {
                        entity.properties.remove(&key);
                    }
                }
                SimCommand::ShowPopup {
                    player_entity_id,
                    title,
                    body,
                    tone,
                } => {
                    if let Some(player) = self
                        .entities
                        .get(&player_entity_id)
                        .and_then(|e| e.player.as_ref())
                    {
                        let _ = player.tx_out.send(ServerMessage::Popup { title, body, tone });
                    }
                }
                SimCommand::TransferToArea {
                    entity_id,
                    area_id,
                    mut pos,
                    yaw,
                } => {
                    if !pos[0].is_finite() || !pos[2].is_finite() {
                        continue;
                    }
                    let Some((current_location, account_id)) =
                        self.entities.get(&entity_id).map(|e| {
                            (
                                e.location.clone(),
                                e.player.as_ref().map(|p| p.account_id).unwrap_or(0),
                            )
                        })
                    else {
                        continue;
                    };

                    let (target_area_id, target_location) = if area_id.is_empty() {
                        (current_location.area_id().to_owned(), current_location)
                    } else {
                        let area_exists = area_id == "main"
                            || area_id == CRYO_PRISON_AREA_ID
                            || self
                                .world_dir
                                .join("areas")
                                .join(&area_id)
                                .join("area.bin")
                                .exists();
                        if !area_exists {
                            warn!("TransferToArea ignored: unknown area '{area_id}'");
                            continue;
                        }
                        let meta = self.load_area_meta(&area_id);
                        let loc = if meta.kind == AreaKind::Outside {
                            EntityLocation::Outside {
                                area_id: area_id.clone(),
                            }
                        } else {
                            EntityLocation::Inside(inside_instance_for(&area_id, account_id))
                        };
                        (area_id, loc)
                    };

                    let same_location = target_location == current_location_of(&self.entities, entity_id);
                    self.remove_from_trigger_occupants(entity_id);

                    let is_player = self
                        .entities
                        .get(&entity_id)
                        .is_some_and(|e| e.player.is_some());
                    if is_player {
                        self.ensure_location_loaded(&target_location, pos);
                    }
                    pos[1] = self.ground_elevation(&target_location, pos);

                    let mut final_yaw = 0.0;
                    let mut tx_player: Option<mpsc::UnboundedSender<ServerMessage>> = None;
                    let mut visual_wire: Option<EntityVisual> = None;
                    if let Some(entity) = self.entities.get_mut(&entity_id) {
                        entity.location = target_location;
                        entity.transform.pos = pos;
                        if let Some(new_yaw) = yaw {
                            entity.transform.yaw = new_yaw;
                        }
                        final_yaw = entity.transform.yaw;
                        if let Some(movement) = &mut entity.movement {
                            movement.clear_path();
                        }
                        if let Some(player) = &mut entity.player {
                            player.pending_interaction = None;
                            tx_player = Some(player.tx_out.clone());
                        }
                        if let Some(vis) = &entity.visual {
                            visual_wire = Some(vis.to_wire());
                        }
                    }

                    if !same_location {
                        if let (Some(tx_out), Some(visual)) = (tx_player, visual_wire) {
                            let meta = self.load_area_meta(&target_area_id);
                            let enter_msg = ServerMessage::EnterArea {
                                entity_id,
                                area_id: target_area_id,
                                area_name: meta.name,
                                area_kind: meta.kind,
                                dimensions: meta.dimensions,
                                pos,
                                yaw: final_yaw,
                                visual,
                                fresh_cryo_wake: false,
                            };
                            let snap_entities = self
                                .entities
                                .get(&entity_id)
                                .map(|obs| self.visible_entities_for(obs))
                                .unwrap_or_default();
                            let _ = tx_out.send(enter_msg);
                            let _ = tx_out.send(ServerMessage::WorldSnapshot {
                                entities: snap_entities,
                            });
                        }
                    }
                }
                SimCommand::Despawn { entity_id } => {
                    self.remove_from_trigger_occupants(entity_id);
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

        let (area_id, pos, yaw, appearance_def, inventory_cfg, fresh_cryo_wake) = match saved {
            Some(record) if record.area_id != CRYO_PRISON_AREA_ID => (
                record.area_id,
                record.pos,
                record.yaw,
                record.appearance,
                record.inventory,
                false,
            ),
            _ => (
                CRYO_PRISON_AREA_ID.to_owned(),
                CRYO_PRISON_SPAWN_POS,
                CRYO_PRISON_SPAWN_YAW,
                CharacterDefinition::cryo_awakening_default(),
                InventoryConfig::default(),
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

        let compact = appearance_def
            .to_compact(&self.character_catalog)
            .unwrap_or_default();

        let entity_id = self.alloc_entity_id();
        let mut entity = Entity::new(entity_id, location, pos, yaw)
            .with_visual(VisualComponent::Character {
                definition: appearance_def,
                compact,
                pose_clip: None,
            })
            .with_movement(MovementComponent::new(PLAYER_MOVE_SPEED))
            .with_inventory(InventoryComponent::from_config(&inventory_cfg))
            .with_player(PlayerComponent {
                account_id,
                conn_id,
                tx_out,
                pending_interaction: None,
            });
        apply_equipped_inventory_to_visual(&mut entity, &self.item_catalog, &self.character_catalog);
        let wire_visual = entity
            .visual
            .as_ref()
            .map(VisualComponent::to_wire)
            .unwrap_or_else(|| EntityVisual::Character(Default::default()));
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
            visual: wire_visual,
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
        let inventory = removed
            .inventory
            .as_ref()
            .map(InventoryComponent::to_config)
            .unwrap_or_default();
        Some(RemovedPlayer {
            entity_id: removed.id,
            account_id,
            area_id: removed.location.area_id().to_owned(),
            pos: removed.transform.pos,
            yaw: removed.transform.yaw,
            appearance_def,
            inventory,
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

    pub fn loot_take_player_from(&mut self, account_id: u64, target_id: EntityId, item_id: String) {
        if let Some(actor_id) = self.player_entities.get(&account_id).copied() {
            self.enqueue_command(SimCommand::LootTake {
                actor_id,
                target_id,
                item_id,
            });
            self.process_commands();
        }
    }

    pub fn loot_take_all_player_from(&mut self, account_id: u64, target_id: EntityId) {
        if let Some(actor_id) = self.player_entities.get(&account_id).copied() {
            self.enqueue_command(SimCommand::LootTakeAll {
                actor_id,
                target_id,
            });
            self.process_commands();
        }
    }

    fn execute_loot_take(
        &mut self,
        actor_id: EntityId,
        target_id: EntityId,
        single_item_id: Option<&str>,
    ) {
        if actor_id == target_id {
            return;
        }
        let valid = (|| {
            let actor = self.entities.get(&actor_id)?;
            let target = self.entities.get(&target_id)?;
            if actor.player.is_none() || actor.inventory.is_none() {
                return None;
            }
            if target.lootable.is_none() || target.inventory.is_none() {
                return None;
            }
            if !Self::entities_in_visible_range(actor, target) {
                return None;
            }
            let trig_cfg = target.effective_click_trigger()?;
            let dist = trig_cfg.horizontal_distance_to_point(
                target.transform.pos,
                target.transform.yaw,
                actor.transform.pos,
            );
            (dist <= trig_cfg.interaction_range.max(0.1) + 0.75).then_some(())
        })()
        .is_some();
        if !valid {
            return;
        }

        let taken: Vec<InventoryEntry> = {
            let Some(target) = self.entities.get_mut(&target_id) else {
                return;
            };
            let Some(inv) = &mut target.inventory else {
                return;
            };
            let list = match single_item_id {
                Some(id) => inv.take_item(id).into_iter().collect(),
                None => inv.take_all(),
            };
            for entry in &list {
                if entry.equipped {
                    unequip_removed_item_from_visual(
                        target,
                        entry,
                        &self.item_catalog,
                        &self.character_catalog,
                    );
                }
            }
            list
        };

        if !taken.is_empty() {
            if let Some(actor) = self.entities.get_mut(&actor_id) {
                for entry in taken {
                    let item_def = self.item_catalog.get(&entry.item_id);
                    let max_stack = item_def.map(|d| d.max_stack).unwrap_or(1);
                    let equip_now = can_auto_equip_item(actor, item_def);
                    if let Some(inv) = &mut actor.inventory {
                        inv.give_item(&entry.item_id, entry.count, max_stack, equip_now);
                    }
                    if equip_now {
                        apply_equipped_inventory_to_visual(
                            actor,
                            &self.item_catalog,
                            &self.character_catalog,
                        );
                    }
                }
            }
            self.broadcast_snapshots_for_entity_change(target_id);
        }

        self.send_loot_window_to_player(actor_id, target_id);
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
                let trig_cfg = target.effective_click_trigger()?;
                let dist = trig_cfg.horizontal_distance_to_point(
                    target.transform.pos,
                    target.transform.yaw,
                    actor.transform.pos,
                );
                Some((
                    dist <= trig_cfg.interaction_range.max(0.1),
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
                self.run_trigger_script(trig_id, actor_entity_id, TriggerWay::WalkInto);
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

fn apply_equipped_inventory_to_visual(
    entity: &mut Entity,
    item_catalog: &ItemCatalog,
    char_catalog: &CharacterCatalog,
) {
    let Some(inv) = &entity.inventory else {
        return;
    };
    let Some(VisualComponent::Character {
        definition,
        compact,
        pose_clip,
    }) = &mut entity.visual
    else {
        return;
    };
    let mut changed = false;
    for entry in &inv.items {
        if !entry.equipped || entry.count == 0 {
            continue;
        }
        let Some(def) = item_catalog.get(&entry.item_id) else {
            continue;
        };
        let Some(binding) = &def.equip else {
            continue;
        };
        match binding {
            ItemEquipBinding::Skinned { slot, part_id } => {
                if definition.skinned_parts.get(slot) != Some(part_id) {
                    definition
                        .skinned_parts
                        .insert(slot.clone(), part_id.clone());
                    changed = true;
                }
            }
            ItemEquipBinding::Socket { socket_id, part_id } => {
                if definition.socket_parts.get(socket_id) != Some(part_id) {
                    definition
                        .socket_parts
                        .insert(socket_id.clone(), part_id.clone());
                    changed = true;
                }
            }
        }
    }
    if changed {
        *compact = definition
            .to_compact_with_pose(char_catalog, pose_clip.clone())
            .unwrap_or_default();
    }
}

fn unequip_removed_item_from_visual(
    entity: &mut Entity,
    removed: &InventoryEntry,
    item_catalog: &ItemCatalog,
    char_catalog: &CharacterCatalog,
) {
    if !removed.equipped {
        return;
    }
    let Some(binding) = item_catalog
        .get(&removed.item_id)
        .and_then(|d| d.equip.as_ref())
    else {
        return;
    };
    let still_occupied = entity.inventory.as_ref().is_some_and(|inv| {
        inv.items.iter().any(|other| {
            if !other.equipped || other.count == 0 {
                return false;
            }
            match (
                binding,
                item_catalog
                    .get(&other.item_id)
                    .and_then(|d| d.equip.as_ref()),
            ) {
                (
                    ItemEquipBinding::Skinned { slot: s1, .. },
                    Some(ItemEquipBinding::Skinned { slot: s2, .. }),
                ) => s1 == s2,
                (
                    ItemEquipBinding::Socket { socket_id: s1, .. },
                    Some(ItemEquipBinding::Socket { socket_id: s2, .. }),
                ) => s1 == s2,
                _ => false,
            }
        })
    });
    if still_occupied {
        return;
    }
    let Some(VisualComponent::Character {
        definition,
        compact,
        pose_clip,
    }) = &mut entity.visual
    else {
        return;
    };
    let changed = match binding {
        ItemEquipBinding::Skinned { slot, part_id } => {
            if definition.skinned_parts.get(slot) == Some(part_id) {
                definition.skinned_parts.remove(slot);
                true
            } else {
                false
            }
        }
        ItemEquipBinding::Socket { socket_id, part_id } => {
            if definition.socket_parts.get(socket_id) == Some(part_id) {
                definition.socket_parts.remove(socket_id);
                true
            } else {
                false
            }
        }
    };
    if changed {
        *compact = definition
            .to_compact_with_pose(char_catalog, pose_clip.clone())
            .unwrap_or_default();
    }
}

fn can_auto_equip_item(entity: &Entity, item_def: Option<&ItemDef>) -> bool {
    let Some(binding) = item_def.and_then(|d| d.equip.as_ref()) else {
        return false;
    };
    let Some(VisualComponent::Character { definition, .. }) = &entity.visual else {
        return false;
    };
    match binding {
        ItemEquipBinding::Skinned { slot, .. } => !definition.skinned_parts.contains_key(slot),
        ItemEquipBinding::Socket { socket_id, .. } => {
            !definition.socket_parts.contains_key(socket_id)
        }
    }
}

fn current_location_of(
    entities: &BTreeMap<EntityId, Entity>,
    entity_id: EntityId,
) -> EntityLocation {
    entities
        .get(&entity_id)
        .map(|e| e.location.clone())
        .unwrap_or_else(|| EntityLocation::Outside {
            area_id: String::new(),
        })
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
        PropertyValue, ScriptConfig, TriggerActivation, TriggerConfig,
    };
    use shared::protocol::PopupTone;
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

        // Each player in cryo_prison must ONLY see themselves as a player (Solo instance isolation).
        match snap1 {
            ServerMessage::WorldSnapshot { entities } => {
                let players: Vec<_> = entities
                    .iter()
                    .filter(|e| e.capabilities.contains(EntityCapabilities::PLAYER))
                    .collect();
                assert_eq!(players.len(), 1);
                assert_eq!(players[0].entity_id, p1_entity_id);
                assert!(players[0]
                    .capabilities
                    .contains(EntityCapabilities::PLAYER | EntityCapabilities::MOVABLE));
            }
            other => panic!("unexpected snap1: {other:?}"),
        }
        match snap2 {
            ServerMessage::WorldSnapshot { entities } => {
                let players: Vec<_> = entities
                    .iter()
                    .filter(|e| e.capabilities.contains(EntityCapabilities::PLAYER))
                    .collect();
                assert_eq!(players.len(), 1);
                assert_eq!(players[0].entity_id, p2_entity_id);
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
                assert!(!entities.iter().any(|e| e.entity_id == p2_entity_id));
                if let Some(p1_snap) = entities.iter().find(|e| e.entity_id == p1_entity_id) {
                    latest_p1 = Some(p1_snap.clone());
                }
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
                assert!(!entities.iter().any(|e| e.entity_id == p1_entity_id));
                if let Some(p2_snap) = entities.iter().find(|e| e.entity_id == p2_entity_id) {
                    latest_p2 = Some(p2_snap.clone());
                }
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
                pose_clip: None,
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

        // Player 101 sees themselves + console + drone (+ any authored solo instance entities); Player 202 sees none of Player 101's.
        let mut latest_p1_entities = Vec::new();
        while let Ok(msg) = rx1.try_recv() {
            if let ServerMessage::WorldSnapshot { entities } = msg {
                latest_p1_entities = entities;
            }
        }
        let p1_ids: Vec<EntityId> = latest_p1_entities.iter().map(|e| e.entity_id).collect();
        assert!(p1_ids.contains(&p1_id));
        assert!(p1_ids.contains(&console_id));
        assert!(p1_ids.contains(&drone_id));

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
        assert!(!latest_p2_entities
            .iter()
            .any(|e| e.entity_id == p1_id || e.entity_id == console_id || e.entity_id == drone_id));
        assert_eq!(
            latest_p2_entities
                .iter()
                .filter(|e| e.capabilities.contains(EntityCapabilities::PLAYER))
                .count(),
            1
        );

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
                let players: Vec<_> = entities
                    .iter()
                    .filter(|e| e.capabilities.contains(EntityCapabilities::PLAYER))
                    .collect();
                assert_eq!(players.len(), 1);
                assert_eq!(players[0].entity_id, new_id);
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
            script: None,
            inventory: None,
            lootable: None,
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
            inventory: InventoryConfig::default(),
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

    #[test]
    fn scripted_trigger_executes_props_popups_area_transfer_and_runtime_limits() {
        let world_dir = unique_temp_world("scripted_triggers");
        fs::create_dir_all(world_dir.join("entities").join("templates")).unwrap();
        fs::create_dir_all(world_dir.join("areas").join("dest_room")).unwrap();

        let dest_meta = AreaMetadata {
            id: "dest_room".to_owned(),
            name: "Dest Room".to_owned(),
            kind: AreaKind::Inside(InsideKind::Permanent),
            dimensions: Some((16, 16)),
        };
        fs::write(
            world_dir.join("areas/dest_room/area.bin"),
            postcard::to_allocvec(&dest_meta).unwrap(),
        )
        .unwrap();
        fs::write(
            world_dir.join("areas/dest_room/sim.bin"),
            postcard::to_allocvec(&InsideSimData::flat_default(16, 16)).unwrap(),
        )
        .unwrap();

        let mut props = PropertyBag::new();
        props.set("count", PropertyValue::Int(0));
        let mut portal_tpl = EntityTemplate {
            id: "portal_script".to_owned(),
            name: "Warp Console".to_owned(),
            properties: props,
            visual: None,
            movement: None,
            trigger: Some(TriggerConfig {
                size: [2.0, 2.0, 2.0],
                offset: [0.0, 0.0, 0.0],
                yaw_deg: 0.0,
                activation: TriggerActivation::Click,
                interaction_range: 3.0,
            }),
            script: Some(ScriptConfig {
                source: String::new(),
                compiled_js: r#"
                    function onTrigger(ctx) {
                        const next = (ctx.entity.getInt("count") || 0) + 1;
                        ctx.entity.setInt("count", next);
                        ctx.player.giveQuest("q_warp", "Enter Dest Room");
                        ctx.showPopup("Portal", "Warping (#" + next + ")", "info");
                        ctx.player.moveToArea("dest_room", 6.0, 6.0, 180);
                    }
                "#
                .to_owned(),
            }),
            inventory: None,
            lootable: None,
        };
        fs::write(
            world_dir.join("entities/templates/portal_script.json"),
            serde_json::to_vec(&portal_tpl).unwrap(),
        )
        .unwrap();

        let mut start_sim = InsideSimData::flat_default(20, 20);
        start_sim.entities.push(PlacedEntity {
            id: 1,
            template_id: "portal_script".to_owned(),
            position: [5.0, 0.0, 5.0],
            rotation_y: 0.0,
            overrides: EntityOverrides::default(),
        });
        fs::create_dir_all(world_dir.join("areas").join("start_room")).unwrap();
        let start_meta = AreaMetadata {
            id: "start_room".to_owned(),
            name: "Start Room".to_owned(),
            kind: AreaKind::Inside(InsideKind::Permanent),
            dimensions: Some((20, 20)),
        };
        fs::write(
            world_dir.join("areas/start_room/area.bin"),
            postcard::to_allocvec(&start_meta).unwrap(),
        )
        .unwrap();
        fs::write(
            world_dir.join("areas/start_room/sim.bin"),
            postcard::to_allocvec(&start_sim).unwrap(),
        )
        .unwrap();

        let mut sim = SimWorld::new(world_dir.clone());
        let (tx, mut rx) = mpsc::unbounded_channel();
        let saved = CharacterSaveRecord {
            account_id: 88,
            area_id: "start_room".to_owned(),
            pos: [5.0, 0.0, 5.0],
            yaw: 0.0,
            appearance: CharacterDefinition::cryo_awakening_default(),
            inventory: InventoryConfig::default(),
        };
        let (conn_id, enter_msg, snap_msg) = sim.spawn_player(88, Some(saved), tx);
        let player_eid = match enter_msg {
            ServerMessage::EnterArea { entity_id, .. } => entity_id,
            _ => panic!("expected EnterArea"),
        };
        let console_eid = match snap_msg {
            ServerMessage::WorldSnapshot { entities } => entities
                .into_iter()
                .find(|e| e.entity_id != player_eid)
                .expect("scripted trigger visible in snapshot")
                .entity_id,
            _ => panic!("expected WorldSnapshot"),
        };
        assert!(sim.entity(console_eid).unwrap().capabilities().contains(EntityCapabilities::SCRIPTED));

        sim.interact_player_with(88, console_eid);

        let console = sim.entity(console_eid).unwrap();
        assert_eq!(console.properties.get_int("count"), Some(1));

        let player = sim.entity(player_eid).unwrap();
        assert_eq!(player.location.area_id(), "dest_room");
        assert_eq!(player.properties.get_str("quest:q_warp"), Some("active"));

        let mut saw_popup = false;
        let mut saw_transfer = false;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                ServerMessage::Popup { title, body, tone } => {
                    assert_eq!(title, "Portal");
                    assert_eq!(body, "Warping (#1)");
                    assert_eq!(tone, PopupTone::Info);
                    saw_popup = true;
                }
                ServerMessage::EnterArea { area_id, .. } if area_id == "dest_room" => {
                    saw_transfer = true;
                }
                _ => {}
            }
        }
        assert!(saw_popup, "player should receive ServerMessage::Popup");
        assert!(saw_transfer, "player should receive EnterArea for dest_room");

        // Live-update template script to an infinite loop; must abort safely & emit ScriptError.
        portal_tpl.script = Some(ScriptConfig {
            source: String::new(),
            compiled_js: "function onTrigger(ctx) { while (true) {} }".to_owned(),
        });
        sim.upsert_entity_template(portal_tpl);

        // Move player back to start_room and interact again to hit the updated faulty script.
        sim.enqueue_command(SimCommand::TransferToArea {
            entity_id: player_eid,
            area_id: "start_room".to_owned(),
            pos: [5.0, 0.0, 5.0],
            yaw: None,
        });
        sim.process_commands();
        sim.interact_player_with(88, console_eid);
        assert!(
            sim.recent_events()
                .iter()
                .any(|e| matches!(e, SimEvent::ScriptError { entity_id, .. } if *entity_id == console_eid)),
            "faulty loop script should record SimEvent::ScriptError"
        );

        assert!(sim.remove_player(88, conn_id).is_some());
        let _ = fs::remove_dir_all(&world_dir);
    }

    #[test]
    fn cryo_prison_corpses_spawn_posed_and_loot_transfers_equip_and_strip_parts() {
        let world_dir = workspace_world_dir();
        if !world_dir.join("areas").join("cryo_prison").join("sim.bin").exists() {
            return;
        }

        let mut sim = SimWorld::new(world_dir);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (conn_id, enter_msg, snap_msg) = sim.spawn_player(901, None, tx);
        let player_eid = match enter_msg {
            ServerMessage::EnterArea { entity_id, .. } => entity_id,
            _ => panic!("expected EnterArea"),
        };

        let entities = match snap_msg {
            ServerMessage::WorldSnapshot { entities } => entities,
            _ => panic!("expected WorldSnapshot"),
        };
        let guard_snap = entities
            .iter()
            .find(|e| e.name.as_deref() == Some("Fallen Security Guard"))
            .expect("Fallen Security Guard spawned in cryo_prison");
        let prisoner_snap = entities
            .iter()
            .find(|e| e.name.as_deref() == Some("Fallen Prisoner"))
            .expect("Fallen Prisoner spawned in cryo_prison");

        assert!(guard_snap.capabilities.contains(EntityCapabilities::LOOTABLE));
        assert!(guard_snap.capabilities.contains(EntityCapabilities::INTERACTABLE));
        assert!(prisoner_snap.capabilities.contains(EntityCapabilities::LOOTABLE));

        match &guard_snap.visual {
            Some(EntityVisual::Character(app)) => {
                assert_eq!(app.pose_clip.as_deref(), Some("DeadSlumped"));
                assert!(!app.socket_part_indices.is_empty(), "guard has rifle equipped");
            }
            other => panic!("expected guard Character visual, got {other:?}"),
        }
        match &prisoner_snap.visual {
            Some(EntityVisual::Character(app)) => {
                assert_eq!(app.pose_clip.as_deref(), Some("DeadSupine"));
                assert!(app.skinned_part_indices.len() >= 3, "prisoner wears head, torso, legs");
            }
            other => panic!("expected prisoner Character visual, got {other:?}"),
        }

        let guard_eid = guard_snap.entity_id;
        let prisoner_eid = prisoner_snap.entity_id;

        // Player starts in cryo_awakening (bare torso/legs, no rifle) and out of range -> LootTake rejected.
        sim.loot_take_player_from(901, prisoner_eid, "cryo_jumpsuit".to_owned());
        assert!(
            sim.entity(player_eid)
                .unwrap()
                .inventory
                .as_ref()
                .unwrap()
                .items
                .is_empty(),
            "out-of-range loot take must be rejected"
        );

        // Click prisoner from afar -> walks into range and opens LootWindow!
        sim.interact_player_with(901, prisoner_eid);
        for _ in 0..80 {
            sim.tick();
        }
        let mut prisoner_loot_items = None;
        while let Ok(msg) = rx.try_recv() {
            if let ServerMessage::LootWindow {
                entity_id,
                title,
                items,
                ..
            } = msg
            {
                if entity_id == prisoner_eid {
                    assert_eq!(title, "Fallen Prisoner");
                    prisoner_loot_items = Some(items);
                }
            }
        }
        let prisoner_items = prisoner_loot_items.expect("clicking prisoner opens LootWindow upon arrival");
        assert_eq!(prisoner_items.len(), 2);

        // Take jumpsuit & pants individually -> prisoner stripped, player auto-equips both!
        sim.loot_take_player_from(901, prisoner_eid, "cryo_jumpsuit".to_owned());
        sim.loot_take_player_from(901, prisoner_eid, "cargo_pants".to_owned());

        let player_ent = sim.entity(player_eid).unwrap();
        assert_eq!(player_ent.inventory.as_ref().unwrap().items.len(), 2);
        assert!(player_ent.inventory.as_ref().unwrap().items.iter().all(|i| i.equipped));
        match &player_ent.visual {
            Some(VisualComponent::Character { definition, .. }) => {
                assert_eq!(
                    definition.skinned_parts.get("torso").map(String::as_str),
                    Some("torso_cryo_jumpsuit")
                );
                assert_eq!(
                    definition.skinned_parts.get("legs").map(String::as_str),
                    Some("legs_cargo_pants")
                );
            }
            _ => panic!("expected player character visual"),
        }
        let prisoner_ent = sim.entity(prisoner_eid).unwrap();
        assert!(prisoner_ent.inventory.as_ref().unwrap().items.is_empty());
        match &prisoner_ent.visual {
            Some(VisualComponent::Character {
                definition,
                compact,
                ..
            }) => {
                assert!(!definition.skinned_parts.contains_key("torso"));
                assert!(!definition.skinned_parts.contains_key("legs"));
                assert_eq!(compact.pose_clip.as_deref(), Some("DeadSupine"));
            }
            _ => panic!("expected prisoner character visual"),
        }

        // Now interact with Fallen Security Guard and Take All -> rifle moves to player's socket_hand_r,
        // guard loses socket_hand_r while keeping visor & armor!
        sim.interact_player_with(901, guard_eid);
        for _ in 0..40 {
            sim.tick();
        }
        sim.loot_take_all_player_from(901, guard_eid);

        let player_ent = sim.entity(player_eid).unwrap();
        match &player_ent.visual {
            Some(VisualComponent::Character { definition, .. }) => {
                assert_eq!(
                    definition.socket_parts.get("socket_hand_r").map(String::as_str),
                    Some("weapon_guard_rifle")
                );
            }
            _ => panic!("expected player character visual"),
        }
        let guard_ent = sim.entity(guard_eid).unwrap();
        assert!(guard_ent.inventory.as_ref().unwrap().items.is_empty());
        match &guard_ent.visual {
            Some(VisualComponent::Character {
                definition,
                compact,
                ..
            }) => {
                assert!(!definition.socket_parts.contains_key("socket_hand_r"));
                assert_eq!(
                    definition.skinned_parts.get("head").map(String::as_str),
                    Some("head_aug_visor")
                );
                assert_eq!(
                    definition.skinned_parts.get("torso").map(String::as_str),
                    Some("torso_fire_rig")
                );
                assert_eq!(compact.pose_clip.as_deref(), Some("DeadSlumped"));
            }
            _ => panic!("expected guard character visual"),
        }

        let mut saw_empty_guard_window = false;
        while let Ok(msg) = rx.try_recv() {
            if let ServerMessage::LootWindow {
                entity_id, items, ..
            } = msg
            {
                if entity_id == guard_eid && items.is_empty() {
                    saw_empty_guard_window = true;
                }
            }
        }
        assert!(saw_empty_guard_window, "LootTakeAll sends updated empty LootWindow");

        assert!(sim.remove_player(901, conn_id).is_some());
    }
}
