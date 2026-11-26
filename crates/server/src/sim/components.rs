use std::collections::{BTreeSet, VecDeque};

use shared::character::{CharacterDefinition, CompactCharacterAppearance};
use shared::entity::{
    EntityId, EntityVisual, InventoryConfig, InventoryEntry, LootableConfig, ScriptConfig,
    TeleportTargetConfig, TriggerConfig,
};
use shared::item::{EquipSlot, ItemCatalog, ItemEquipSpec};
use shared::protocol::ServerMessage;
use tokio::sync::mpsc;

pub const CRYO_PRISON_AREA_ID: &str = "cryo_prison";
pub const BASE_BAG_SLOTS: u16 = 20;
pub const BASE_MAX_WEIGHT: f32 = 30.0;

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

/// Authoritative spatial membership for an entity (Outside grid area or specific Inside instance).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EntityLocation {
    Outside { area_id: String },
    Inside(InsideInstanceId),
}

impl EntityLocation {
    pub fn area_id(&self) -> &str {
        match self {
            Self::Outside { area_id } => area_id,
            Self::Inside(inst) => inst.area_id(),
        }
    }

    #[inline]
    #[allow(dead_code)]
    pub fn is_outside(&self) -> bool {
        matches!(self, Self::Outside { .. })
    }

    #[inline]
    #[allow(dead_code)]
    pub fn inside_instance(&self) -> Option<&InsideInstanceId> {
        match self {
            Self::Outside { .. } => None,
            Self::Inside(inst) => Some(inst),
        }
    }
}

/// Records the authored world-file placement that spawned a non-player entity so hot reloads stay synced.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SpawnOrigin {
    OutsideTile {
        area_id: String,
        tx: i32,
        ty: i32,
        placed_id: u64,
    },
    InsideArea {
        instance: InsideInstanceId,
        placed_id: u64,
    },
}

impl SpawnOrigin {
    pub fn placed_id(&self) -> u64 {
        match self {
            Self::OutsideTile { placed_id, .. } | Self::InsideArea { placed_id, .. } => *placed_id,
        }
    }
}

/// World-space position and Y-axis heading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformComponent {
    pub pos: [f32; 3],
    pub yaw: f32,
}

/// Visual presentation state replicated to clients via [`EntityVisual`].
#[derive(Debug, Clone)]
pub enum VisualComponent {
    Character {
        base_definition: CharacterDefinition,
        definition: CharacterDefinition,
        compact: CompactCharacterAppearance,
        pose_clip: Option<String>,
    },
    Prop {
        asset_id: String,
        scale: [f32; 3],
        cast_shadow: bool,
    },
}

impl VisualComponent {
    pub fn to_wire(&self) -> EntityVisual {
        match self {
            Self::Character { compact, .. } => EntityVisual::Character(compact.clone()),
            Self::Prop {
                asset_id,
                scale,
                cast_shadow,
            } => EntityVisual::Prop {
                asset_id: asset_id.clone(),
                scale: *scale,
                cast_shadow: *cast_shadow,
            },
        }
    }
}

/// Grid-pathfinding waypoint queue and movement speed for movable entities.
#[derive(Debug, Clone)]
pub struct MovementComponent {
    pub speed: f32,
    pub waypoints: VecDeque<[f32; 2]>,
}

impl MovementComponent {
    pub fn new(speed: f32) -> Self {
        Self {
            speed,
            waypoints: VecDeque::new(),
        }
    }

    #[inline]
    pub fn is_moving(&self) -> bool {
        !self.waypoints.is_empty()
    }

    pub fn clear_path(&mut self) {
        self.waypoints.clear();
    }
}

/// Oriented box volume that detects player click interactions and/or walk-into entry events.
#[derive(Debug, Clone)]
pub struct TriggerComponent {
    pub config: TriggerConfig,
    pub occupants: BTreeSet<EntityId>,
}

impl TriggerComponent {
    pub fn new(config: TriggerConfig) -> Self {
        Self {
            config,
            occupants: BTreeSet::new(),
        }
    }
}

/// Custom TypeScript/JS trigger handler attached to an entity.
#[derive(Debug, Clone)]
pub struct ScriptComponent {
    pub config: ScriptConfig,
}

impl ScriptComponent {
    pub fn new(config: ScriptConfig) -> Self {
        Self { config }
    }
}

/// Authoritative item storage attached to players, NPCs, corpses, and containers.
#[derive(Debug, Clone, Default)]
pub struct InventoryComponent {
    pub items: Vec<InventoryEntry>,
}

impl InventoryComponent {
    pub fn from_config(cfg: &InventoryConfig) -> Self {
        let items = cfg
            .items
            .iter()
            .filter(|e| !e.item_id.trim().is_empty() && e.count > 0)
            .cloned()
            .collect();
        Self { items }
    }

    pub fn to_config(&self) -> InventoryConfig {
        InventoryConfig {
            items: self
                .items
                .iter()
                .filter(|e| e.count > 0)
                .cloned()
                .collect(),
        }
    }

    pub fn take_item(&mut self, item_id: &str) -> Option<InventoryEntry> {
        let pos = self
            .items
            .iter()
            .position(|e| e.item_id == item_id && e.count > 0)?;
        Some(self.items.remove(pos))
    }

    pub fn take_all(&mut self) -> Vec<InventoryEntry> {
        self.items.drain(..).filter(|e| e.count > 0).collect()
    }

    pub fn give_item(&mut self, item_id: &str, count: u32, max_stack: u32, equip_now: bool) {
        if count == 0 || item_id.trim().is_empty() {
            return;
        }
        let stack_cap = max_stack.max(1);
        let mut remaining = count;
        if equip_now {
            self.items.push(InventoryEntry {
                item_id: item_id.to_owned(),
                count: 1,
                equipped: true,
                bag_slot: None,
            });
            remaining -= 1;
        }
        if remaining == 0 {
            return;
        }
        for existing in &mut self.items {
            if !existing.equipped && existing.item_id == item_id && existing.count < stack_cap {
                let add = remaining.min(stack_cap - existing.count);
                existing.count += add;
                remaining -= add;
                if remaining == 0 {
                    return;
                }
            }
        }
        let mut used: BTreeSet<u16> = self
            .items
            .iter()
            .filter(|e| !e.equipped && e.count > 0)
            .filter_map(|e| e.bag_slot)
            .collect();
        while remaining > 0 {
            let chunk = remaining.min(stack_cap);
            let slot = Self::next_free_slot_in_set(&used, u16::MAX);
            if let Some(s) = slot {
                used.insert(s);
            }
            self.items.push(InventoryEntry {
                item_id: item_id.to_owned(),
                count: chunk,
                equipped: false,
                bag_slot: slot,
            });
            remaining -= chunk;
        }
    }

    fn next_free_slot_in_set(used: &BTreeSet<u16>, limit: u16) -> Option<u16> {
        (0..limit).find(|s| !used.contains(s))
    }

    /// Total backpack slots available (`BASE_BAG_SLOTS` plus any equipped `bonus_bag_slots`).
    pub fn bag_capacity(&self, item_catalog: &ItemCatalog) -> u16 {
        let bonus: u32 = self
            .items
            .iter()
            .filter(|e| e.equipped && e.count > 0)
            .filter_map(|e| item_catalog.get(&e.item_id)?.equip.as_ref())
            .map(|s| s.bonus_bag_slots as u32)
            .sum();
        (BASE_BAG_SLOTS as u32 + bonus).min(u16::MAX as u32) as u16
    }

    /// Maximum carry weight in kilograms.
    pub fn max_weight(&self) -> f32 {
        BASE_MAX_WEIGHT
    }

    /// Total carried weight in kilograms across all equipped and unequipped entries.
    pub fn total_weight(&self, item_catalog: &ItemCatalog) -> f32 {
        self.items
            .iter()
            .filter(|e| e.count > 0)
            .map(|e| {
                item_catalog
                    .get(&e.item_id)
                    .map(|d| d.weight.max(0.0))
                    .unwrap_or(0.0)
                    * e.count as f32
            })
            .sum()
    }

    /// Returns the lowest unused bag slot index in `0..capacity`.
    pub fn first_free_bag_slot(&self, capacity: u16) -> Option<u16> {
        let used: BTreeSet<u16> = self
            .items
            .iter()
            .filter(|e| !e.equipped && e.count > 0)
            .filter_map(|e| e.bag_slot)
            .collect();
        Self::next_free_slot_in_set(&used, capacity)
    }

    /// Returns true if equipping `candidate` conflicts with any currently equipped item.
    pub fn has_equip_conflict(&self, candidate: &ItemEquipSpec, item_catalog: &ItemCatalog) -> bool {
        self.items.iter().any(|entry| {
            entry.equipped
                && entry.count > 0
                && item_catalog
                    .get(&entry.item_id)
                    .and_then(|d| d.equip.as_ref())
                    .is_some_and(|existing| candidate.conflicts_with(existing))
        })
    }

    /// Returns the equipped inventory entry currently occupying logical `slot`, if any.
    #[allow(dead_code)]
    pub fn equipped_in_slot(
        &self,
        slot: EquipSlot,
        item_catalog: &ItemCatalog,
    ) -> Option<&InventoryEntry> {
        self.items.iter().find(|entry| {
            entry.equipped
                && entry.count > 0
                && item_catalog
                    .get(&entry.item_id)
                    .and_then(|d| d.equip.as_ref())
                    .is_some_and(|spec| spec.occupies_slot(slot))
        })
    }

    /// Enforces equip validity, slot/two-handed uniqueness, and unique `bag_slot` assignment.
    pub fn normalize_layout(&mut self, item_catalog: &ItemCatalog) {
        self.normalize_equipped_only(item_catalog);
        let cap = self.bag_capacity(item_catalog);
        let mut used: BTreeSet<u16> = BTreeSet::new();

        for entry in &mut self.items {
            if entry.count == 0 {
                continue;
            }
            if entry.equipped {
                entry.bag_slot = None;
            } else if let Some(slot) = entry.bag_slot {
                if slot < cap && used.insert(slot) {
                    // Valid unique bag slot inside capacity.
                } else {
                    entry.bag_slot = None;
                }
            }
        }

        for entry in &mut self.items {
            if entry.count == 0 || entry.equipped || entry.bag_slot.is_some() {
                continue;
            }
            let assigned = Self::next_free_slot_in_set(&used, cap)
                .or_else(|| Self::next_free_slot_in_set(&used, u16::MAX));
            if let Some(s) = assigned {
                used.insert(s);
            }
            entry.bag_slot = assigned;
        }
    }

    fn normalize_equipped_only(&mut self, item_catalog: &ItemCatalog) {
        let mut active_specs: Vec<ItemEquipSpec> = Vec::new();
        let mut overflow: Vec<InventoryEntry> = Vec::new();
        for entry in &mut self.items {
            if entry.count == 0 || !entry.equipped {
                continue;
            }
            let Some(spec) = item_catalog
                .get(&entry.item_id)
                .and_then(|d| d.equip.as_ref())
            else {
                entry.equipped = false;
                continue;
            };
            if active_specs.iter().any(|existing| spec.conflicts_with(existing)) {
                entry.equipped = false;
            } else {
                if entry.count > 1 {
                    overflow.push(InventoryEntry {
                        item_id: entry.item_id.clone(),
                        count: entry.count - 1,
                        equipped: false,
                        bag_slot: None,
                    });
                    entry.count = 1;
                }
                entry.bag_slot = None;
                active_specs.push(spec.clone());
            }
        }
        for extra in overflow {
            let max_stack = item_catalog
                .get(&extra.item_id)
                .map(|d| d.max_stack)
                .unwrap_or(1);
            self.give_item(&extra.item_id, extra.count, max_stack, false);
        }
    }

    /// Checks whether `count` units of `item_id` can fit within weight and slot capacity.
    pub fn can_accept_item(
        &self,
        item_id: &str,
        count: u32,
        item_catalog: &ItemCatalog,
        equip_now: bool,
    ) -> Result<(), &'static str> {
        if count == 0 || item_id.trim().is_empty() {
            return Ok(());
        }
        let item_def = item_catalog.get(item_id);
        let unit_weight = item_def.map(|d| d.weight.max(0.0)).unwrap_or(0.0);
        if self.total_weight(item_catalog) + unit_weight * count as f32 > self.max_weight() + 1e-4 {
            return Err("Too heavy to carry.");
        }

        let mut remaining = count;
        let mut effective_cap = self.bag_capacity(item_catalog) as usize;
        if equip_now {
            remaining = remaining.saturating_sub(1);
            if let Some(spec) = item_def.and_then(|d| d.equip.as_ref()) {
                effective_cap += spec.bonus_bag_slots as usize;
            }
        }
        let max_stack = item_def.map(|d| d.max_stack).unwrap_or(1).max(1);
        for existing in &self.items {
            if remaining == 0 {
                break;
            }
            if !existing.equipped
                && existing.count > 0
                && existing.item_id == item_id
                && existing.count < max_stack
            {
                remaining = remaining.saturating_sub(max_stack - existing.count);
            }
        }
        let slots_needed = if remaining == 0 {
            0
        } else {
            remaining.div_ceil(max_stack) as usize
        };
        let used_slots = self
            .items
            .iter()
            .filter(|e| !e.equipped && e.count > 0)
            .count();
        if used_slots + slots_needed > effective_cap {
            return Err("Inventory is full.");
        }
        Ok(())
    }

    /// Moves, swaps, or merges an item stack between two bag slots.
    pub fn move_bag_slot(
        &mut self,
        from_slot: u16,
        to_slot: u16,
        item_catalog: &ItemCatalog,
    ) -> Result<(), &'static str> {
        let cap = self.bag_capacity(item_catalog);
        if from_slot == to_slot || from_slot >= cap || to_slot >= cap {
            return Err("Invalid bag slot.");
        }
        let from_idx = self
            .items
            .iter()
            .position(|e| !e.equipped && e.count > 0 && e.bag_slot == Some(from_slot))
            .ok_or("Source bag slot is empty.")?;
        let to_idx = self
            .items
            .iter()
            .position(|e| !e.equipped && e.count > 0 && e.bag_slot == Some(to_slot));

        let Some(ti) = to_idx else {
            self.items[from_idx].bag_slot = Some(to_slot);
            return Ok(());
        };

        if self.items[from_idx].item_id == self.items[ti].item_id {
            let max_stack = item_catalog
                .get(&self.items[ti].item_id)
                .map(|d| d.max_stack)
                .unwrap_or(1)
                .max(1);
            if self.items[ti].count < max_stack {
                let add = self.items[from_idx].count.min(max_stack - self.items[ti].count);
                self.items[ti].count += add;
                self.items[from_idx].count -= add;
                self.items.retain(|e| e.count > 0);
                return Ok(());
            }
        }

        self.items[from_idx].bag_slot = Some(to_slot);
        self.items[ti].bag_slot = Some(from_slot);
        Ok(())
    }

    /// Splits `split_count` units from `from_slot` into `to_slot` (empty or partial stack of same item).
    pub fn split_bag_stack(
        &mut self,
        from_slot: u16,
        to_slot: u16,
        split_count: u32,
        item_catalog: &ItemCatalog,
    ) -> Result<(), &'static str> {
        let cap = self.bag_capacity(item_catalog);
        if from_slot == to_slot || from_slot >= cap || to_slot >= cap || split_count == 0 {
            return Err("Invalid stack split.");
        }
        let from_idx = self
            .items
            .iter()
            .position(|e| !e.equipped && e.count > 0 && e.bag_slot == Some(from_slot))
            .ok_or("Source bag slot is empty.")?;
        if self.items[from_idx].count <= split_count {
            return Err("Not enough items in stack to split.");
        }
        let to_idx = self
            .items
            .iter()
            .position(|e| !e.equipped && e.count > 0 && e.bag_slot == Some(to_slot));

        match to_idx {
            None => {
                let item_id = self.items[from_idx].item_id.clone();
                self.items[from_idx].count -= split_count;
                self.items.push(InventoryEntry {
                    item_id,
                    count: split_count,
                    equipped: false,
                    bag_slot: Some(to_slot),
                });
                Ok(())
            }
            Some(ti) => {
                if self.items[from_idx].item_id != self.items[ti].item_id {
                    return Err("Target slot must be empty or hold the same item.");
                }
                let max_stack = item_catalog
                    .get(&self.items[ti].item_id)
                    .map(|d| d.max_stack)
                    .unwrap_or(1)
                    .max(1);
                if self.items[ti].count + split_count > max_stack {
                    return Err("Target stack does not have enough room.");
                }
                self.items[from_idx].count -= split_count;
                self.items[ti].count += split_count;
                Ok(())
            }
        }
    }

    fn relocate_overflow_bag_items(&mut self, capacity: u16) -> bool {
        let mut used: BTreeSet<u16> = self
            .items
            .iter()
            .filter(|e| !e.equipped && e.count > 0)
            .filter_map(|e| e.bag_slot.filter(|&s| s < capacity))
            .collect();
        for entry in &mut self.items {
            if entry.equipped || entry.count == 0 {
                continue;
            }
            if entry.bag_slot.is_some_and(|s| s < capacity) {
                continue;
            }
            let Some(free) = Self::next_free_slot_in_set(&used, capacity) else {
                return false;
            };
            used.insert(free);
            entry.bag_slot = Some(free);
        }
        true
    }

    fn place_unequipped_entry(
        &mut self,
        idx: usize,
        preferred_slot: Option<u16>,
        capacity: u16,
        item_catalog: &ItemCatalog,
    ) -> bool {
        let item_id = self.items[idx].item_id.clone();
        let max_stack = item_catalog
            .get(&item_id)
            .map(|d| d.max_stack)
            .unwrap_or(1)
            .max(1);

        if let Some(pref) = preferred_slot.filter(|&s| s < capacity) {
            let occupied = self
                .items
                .iter()
                .enumerate()
                .any(|(i, e)| i != idx && !e.equipped && e.count > 0 && e.bag_slot == Some(pref));
            if !occupied {
                self.items[idx].equipped = false;
                self.items[idx].bag_slot = Some(pref);
                return true;
            }
        }

        if self.items[idx].count == 1 && max_stack > 1 {
            if let Some(dest) = self.items.iter_mut().enumerate().find_map(|(i, e)| {
                (i != idx
                    && !e.equipped
                    && e.count > 0
                    && e.item_id == item_id
                    && e.count < max_stack
                    && e.bag_slot.is_some_and(|s| s < capacity))
                .then_some(e)
            }) {
                dest.count += 1;
                self.items[idx].count = 0;
                return true;
            }
        }

        if let Some(free) = self.first_free_bag_slot(capacity) {
            self.items[idx].equipped = false;
            self.items[idx].bag_slot = Some(free);
            true
        } else {
            false
        }
    }

    /// Equips an item from `from_slot`, validating `target_slot` and swapping displaced equipment transactionally.
    pub fn equip_from_bag(
        &mut self,
        from_slot: u16,
        target_slot: Option<EquipSlot>,
        item_catalog: &ItemCatalog,
    ) -> Result<(), &'static str> {
        let from_idx = self
            .items
            .iter()
            .position(|e| !e.equipped && e.count > 0 && e.bag_slot == Some(from_slot))
            .ok_or("Bag slot is empty.")?;
        let spec = item_catalog
            .get(&self.items[from_idx].item_id)
            .and_then(|d| d.equip.as_ref())
            .cloned()
            .ok_or("Item cannot be equipped.")?;
        if let Some(ts) = target_slot {
            if !spec.occupies_slot(ts) {
                return Err("Item does not fit that equipment slot.");
            }
        }

        let mut trial = self.clone();
        let displaced: Vec<usize> = trial
            .items
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                e.equipped
                    && e.count > 0
                    && item_catalog
                        .get(&e.item_id)
                        .and_then(|d| d.equip.as_ref())
                        .is_some_and(|other| spec.conflicts_with(other))
            })
            .map(|(i, _)| i)
            .collect();

        for &di in &displaced {
            trial.items[di].equipped = false;
            trial.items[di].bag_slot = None;
        }

        let freed_from_slot = if trial.items[from_idx].count > 1 {
            let item_id = trial.items[from_idx].item_id.clone();
            trial.items[from_idx].count -= 1;
            trial.items.push(InventoryEntry {
                item_id,
                count: 1,
                equipped: true,
                bag_slot: None,
            });
            None
        } else {
            trial.items[from_idx].equipped = true;
            trial.items[from_idx].bag_slot = None;
            Some(from_slot)
        };

        let new_cap = trial.bag_capacity(item_catalog);
        for (n, &di) in displaced.iter().enumerate() {
            let pref = if n == 0 { freed_from_slot } else { None };
            if !trial.place_unequipped_entry(di, pref, new_cap, item_catalog) {
                return Err("Not enough bag space to swap equipment.");
            }
        }
        trial.items.retain(|e| e.count > 0);
        if !trial.relocate_overflow_bag_items(new_cap) {
            return Err("Not enough bag space to swap equipment.");
        }
        *self = trial;
        Ok(())
    }

    /// Unequips whatever occupies `slot` into `target_bag_slot` (or first free slot), swapping if dropped on compatible gear.
    pub fn unequip_to_bag(
        &mut self,
        slot: EquipSlot,
        target_bag_slot: Option<u16>,
        item_catalog: &ItemCatalog,
    ) -> Result<(), &'static str> {
        let eq_idx = self
            .items
            .iter()
            .position(|e| {
                e.equipped
                    && e.count > 0
                    && item_catalog
                        .get(&e.item_id)
                        .and_then(|d| d.equip.as_ref())
                        .is_some_and(|spec| spec.occupies_slot(slot))
            })
            .ok_or("Equipment slot is empty.")?;

        if let Some(dest_slot) = target_bag_slot {
            if let Some(dest) = self
                .items
                .iter()
                .find(|e| !e.equipped && e.count > 0 && e.bag_slot == Some(dest_slot))
            {
                let same_stackable = dest.item_id == self.items[eq_idx].item_id
                    && item_catalog
                        .get(&dest.item_id)
                        .is_some_and(|d| dest.count < d.max_stack);
                let eq_spec = item_catalog
                    .get(&self.items[eq_idx].item_id)
                    .and_then(|d| d.equip.as_ref());
                let compatible_slot = eq_spec.and_then(|eqs| {
                    item_catalog
                        .get(&dest.item_id)
                        .and_then(|d| d.equip.as_ref())
                        .filter(|dests| eqs.conflicts_with(dests))
                        .map(|dests| dests.slot)
                });
                if !same_stackable && dest.item_id != self.items[eq_idx].item_id {
                    if let Some(target_eq_slot) = compatible_slot {
                        return self.equip_from_bag(dest_slot, Some(target_eq_slot), item_catalog);
                    }
                }
            }
        }

        let mut trial = self.clone();
        let bonus_removed = item_catalog
            .get(&trial.items[eq_idx].item_id)
            .and_then(|d| d.equip.as_ref())
            .map(|s| s.bonus_bag_slots)
            .unwrap_or(0);
        let new_cap = trial.bag_capacity(item_catalog).saturating_sub(bonus_removed);

        if let Some(dest_slot) = target_bag_slot {
            if dest_slot >= new_cap {
                return Err("Target bag slot is unavailable.");
            }
            if let Some(ti) = trial
                .items
                .iter()
                .position(|e| !e.equipped && e.count > 0 && e.bag_slot == Some(dest_slot))
            {
                let max_stack = item_catalog
                    .get(&trial.items[ti].item_id)
                    .map(|d| d.max_stack)
                    .unwrap_or(1)
                    .max(1);
                if trial.items[ti].item_id == trial.items[eq_idx].item_id
                    && trial.items[ti].count < max_stack
                {
                    trial.items[ti].count += 1;
                    trial.items.remove(eq_idx);
                } else {
                    return Err("Target bag slot is occupied.");
                }
            } else {
                trial.items[eq_idx].equipped = false;
                trial.items[eq_idx].bag_slot = Some(dest_slot);
            }
        } else if !trial.place_unequipped_entry(eq_idx, None, new_cap, item_catalog) {
            return Err("Not enough bag space to unequip.");
        }

        trial.items.retain(|e| e.count > 0);
        if !trial.relocate_overflow_bag_items(new_cap) {
            return Err("Not enough bag space to unequip.");
        }
        *self = trial;
        Ok(())
    }

    /// Equips the item at `index`, unequipping any conflicting slot (including two-handed MainHand/OffHand).
    #[allow(dead_code)]
    pub fn equip_entry(&mut self, index: usize, item_catalog: &ItemCatalog) -> bool {
        let Some(entry) = self.items.get(index) else {
            return false;
        };
        if entry.count == 0 || entry.equipped {
            return false;
        }
        let Some(spec) = item_catalog
            .get(&entry.item_id)
            .and_then(|d| d.equip.as_ref())
        else {
            return false;
        };

        for existing in &mut self.items {
            if !existing.equipped || existing.count == 0 {
                continue;
            }
            let conflicts = item_catalog
                .get(&existing.item_id)
                .and_then(|d| d.equip.as_ref())
                .is_some_and(|other| spec.conflicts_with(other));
            if conflicts {
                existing.equipped = false;
            }
        }

        if self.items[index].count > 1 {
            let item_id = self.items[index].item_id.clone();
            self.items[index].count -= 1;
            self.items.push(InventoryEntry {
                item_id,
                count: 1,
                equipped: true,
                bag_slot: None,
            });
        } else {
            self.items[index].equipped = true;
            self.items[index].bag_slot = None;
        }
        self.merge_unequipped_stacks(item_catalog);
        self.normalize_layout(item_catalog);
        true
    }

    /// Unequips whichever item currently occupies logical `slot` (including a two-handed MainHand weapon when `OffHand` is cleared).
    #[allow(dead_code)]
    pub fn unequip_slot(&mut self, slot: EquipSlot, item_catalog: &ItemCatalog) -> bool {
        let mut changed = false;
        for entry in &mut self.items {
            if !entry.equipped || entry.count == 0 {
                continue;
            }
            let occupies = item_catalog
                .get(&entry.item_id)
                .and_then(|d| d.equip.as_ref())
                .is_some_and(|spec| spec.occupies_slot(slot));
            if occupies {
                entry.equipped = false;
                changed = true;
            }
        }
        if changed {
            self.merge_unequipped_stacks(item_catalog);
            self.normalize_layout(item_catalog);
        }
        changed
    }

    #[allow(dead_code)]
    fn merge_unequipped_stacks(&mut self, item_catalog: &ItemCatalog) {
        let len = self.items.len();
        for i in 0..len {
            if self.items[i].equipped || self.items[i].count == 0 {
                continue;
            }
            let item_id = self.items[i].item_id.clone();
            let max_stack = item_catalog
                .get(&item_id)
                .map(|d| d.max_stack)
                .unwrap_or(1)
                .max(1);
            for j in 0..i {
                if self.items[i].count == 0 {
                    break;
                }
                if !self.items[j].equipped
                    && self.items[j].item_id == item_id
                    && self.items[j].count < max_stack
                {
                    let add = self.items[i].count.min(max_stack - self.items[j].count);
                    self.items[j].count += add;
                    self.items[i].count -= add;
                }
            }
        }
        self.items.retain(|e| e.count > 0);
    }
}

/// Click-to-loot behavior opening an interactive dialog over the entity's inventory.
#[derive(Debug, Clone, Default)]
pub struct LootableComponent {
    pub config: LootableConfig,
}

impl LootableComponent {
    pub fn new(config: LootableConfig) -> Self {
        Self { config }
    }
}

/// Marks an entity as a named arrival destination for `ctx.teleportToArea(areaId, targetName)`.
#[derive(Debug, Clone, Copy, Default)]
pub struct TeleportTargetComponent {
    #[allow(dead_code)]
    pub config: TeleportTargetConfig,
}

impl TeleportTargetComponent {
    pub fn new(config: TeleportTargetConfig) -> Self {
        Self { config }
    }
}

/// Network/session attachment that promotes an entity into a connected player observer.
#[derive(Debug, Clone)]
pub struct PlayerComponent {
    pub account_id: u64,
    pub conn_id: u64,
    pub tx_out: mpsc::UnboundedSender<ServerMessage>,
    pub pending_interaction: Option<EntityId>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::item::{ItemDef, VisualAttachment};

    fn test_catalog() -> ItemCatalog {
        ItemCatalog {
            items: vec![
                ItemDef {
                    id: "scrap_blade".to_owned(),
                    name: "Scrap Blade".to_owned(),
                    description: String::new(),
                    category: "Weapon".to_owned(),
                    max_stack: 1,
                    weight: 2.4,
                    preview_glb: None,
                    equip: Some(ItemEquipSpec {
                        slot: EquipSlot::MainHand,
                        two_handed: false,
                        visuals: vec![VisualAttachment::Socket {
                            socket_id: "socket_hand_r".to_owned(),
                            part_id: "weapon_scrap_blade".to_owned(),
                        }],
                        hides_categories: Vec::new(),
                        bonus_bag_slots: 0,
                    }),
                },
                ItemDef {
                    id: "guard_rifle".to_owned(),
                    name: "Guard Rifle".to_owned(),
                    description: String::new(),
                    category: "Weapon".to_owned(),
                    max_stack: 1,
                    weight: 3.6,
                    preview_glb: None,
                    equip: Some(ItemEquipSpec {
                        slot: EquipSlot::MainHand,
                        two_handed: true,
                        visuals: vec![VisualAttachment::Socket {
                            socket_id: "socket_hand_r".to_owned(),
                            part_id: "weapon_guard_rifle".to_owned(),
                        }],
                        hides_categories: Vec::new(),
                        bonus_bag_slots: 0,
                    }),
                },
                ItemDef {
                    id: "riot_shield".to_owned(),
                    name: "Riot Shield".to_owned(),
                    description: String::new(),
                    category: "Armor".to_owned(),
                    max_stack: 1,
                    weight: 4.0,
                    preview_glb: None,
                    equip: Some(ItemEquipSpec {
                        slot: EquipSlot::OffHand,
                        two_handed: false,
                        visuals: vec![VisualAttachment::Socket {
                            socket_id: "socket_hand_l".to_owned(),
                            part_id: "shield_riot".to_owned(),
                        }],
                        hides_categories: Vec::new(),
                        bonus_bag_slots: 0,
                    }),
                },
                ItemDef {
                    id: "throwing_knife".to_owned(),
                    name: "Throwing Knife".to_owned(),
                    description: String::new(),
                    category: "Weapon".to_owned(),
                    max_stack: 5,
                    weight: 0.3,
                    preview_glb: None,
                    equip: Some(ItemEquipSpec {
                        slot: EquipSlot::MainHand,
                        two_handed: false,
                        visuals: Vec::new(),
                        hides_categories: Vec::new(),
                        bonus_bag_slots: 0,
                    }),
                },
                ItemDef {
                    id: "salvage_rig_pack".to_owned(),
                    name: "Salvage Rig Pack".to_owned(),
                    description: String::new(),
                    category: "Back".to_owned(),
                    max_stack: 1,
                    weight: 1.5,
                    preview_glb: None,
                    equip: Some(ItemEquipSpec {
                        slot: EquipSlot::Back,
                        two_handed: false,
                        visuals: vec![VisualAttachment::Socket {
                            socket_id: "socket_back".to_owned(),
                            part_id: "pack_salvage_rig".to_owned(),
                        }],
                        hides_categories: Vec::new(),
                        bonus_bag_slots: 5,
                    }),
                },
                ItemDef {
                    id: "ration_pack".to_owned(),
                    name: "Ration Pack".to_owned(),
                    description: String::new(),
                    category: "Consumable".to_owned(),
                    max_stack: 10,
                    weight: 0.5,
                    preview_glb: None,
                    equip: None,
                },
                ItemDef {
                    id: "lead_ingot".to_owned(),
                    name: "Lead Ingot".to_owned(),
                    description: String::new(),
                    category: "Material".to_owned(),
                    max_stack: 10,
                    weight: 12.0,
                    preview_glb: None,
                    equip: None,
                },
            ],
        }
    }

    #[test]
    fn equip_entry_and_unequip_slot_enforce_two_handed_and_stack_merging() {
        let cat = test_catalog();
        let mut inv = InventoryComponent::default();
        inv.give_item("scrap_blade", 1, 1, false);
        inv.give_item("riot_shield", 1, 1, false);
        inv.give_item("guard_rifle", 1, 1, false);
        inv.give_item("throwing_knife", 3, 5, false);

        // One-handed weapon + off-hand shield coexist cleanly.
        assert!(inv.equip_entry(0, &cat));
        assert!(inv.equip_entry(1, &cat));
        assert_eq!(
            inv.equipped_in_slot(EquipSlot::MainHand, &cat)
                .map(|e| e.item_id.as_str()),
            Some("scrap_blade")
        );
        assert_eq!(
            inv.equipped_in_slot(EquipSlot::OffHand, &cat)
                .map(|e| e.item_id.as_str()),
            Some("riot_shield")
        );

        // Equipping two-handed rifle clears both MainHand and OffHand.
        assert!(inv.equip_entry(2, &cat));
        assert_eq!(
            inv.equipped_in_slot(EquipSlot::MainHand, &cat)
                .map(|e| e.item_id.as_str()),
            Some("guard_rifle")
        );
        assert_eq!(
            inv.equipped_in_slot(EquipSlot::OffHand, &cat)
                .map(|e| e.item_id.as_str()),
            Some("guard_rifle")
        );
        assert!(!inv.items[0].equipped);
        assert!(!inv.items[1].equipped);

        // Equipping OffHand shield explicitly unequips two-handed rifle.
        assert!(inv.equip_entry(1, &cat));
        assert!(inv.equipped_in_slot(EquipSlot::MainHand, &cat).is_none());
        assert_eq!(
            inv.equipped_in_slot(EquipSlot::OffHand, &cat)
                .map(|e| e.item_id.as_str()),
            Some("riot_shield")
        );

        // Equipping one item from a stack splits 1 equipped entry, then unequip_slot merges back.
        let knife_idx = inv
            .items
            .iter()
            .position(|e| e.item_id == "throwing_knife")
            .unwrap();
        assert!(inv.equip_entry(knife_idx, &cat));
        assert_eq!(inv.items[knife_idx].count, 2);
        assert!(!inv.items[knife_idx].equipped);
        assert_eq!(
            inv.equipped_in_slot(EquipSlot::MainHand, &cat)
                .map(|e| e.item_id.as_str()),
            Some("throwing_knife")
        );
        assert!(inv.unequip_slot(EquipSlot::MainHand, &cat));
        let knife_stacks: Vec<_> = inv
            .items
            .iter()
            .filter(|e| e.item_id == "throwing_knife")
            .collect();
        assert_eq!(knife_stacks.len(), 1);
        assert_eq!(knife_stacks[0].count, 3);
        assert!(!knife_stacks[0].equipped);
    }

    #[test]
    fn normalize_equipped_clears_invalid_and_conflicting_entries() {
        let cat = test_catalog();
        let mut inv = InventoryComponent::from_config(&InventoryConfig {
            items: vec![
                InventoryEntry {
                    item_id: "ration_pack".to_owned(),
                    count: 2,
                    equipped: true,
                    bag_slot: None,
                },
                InventoryEntry {
                    item_id: "guard_rifle".to_owned(),
                    count: 1,
                    equipped: true,
                    bag_slot: None,
                },
                InventoryEntry {
                    item_id: "riot_shield".to_owned(),
                    count: 1,
                    equipped: true,
                    bag_slot: None,
                },
                InventoryEntry {
                    item_id: "scrap_blade".to_owned(),
                    count: 1,
                    equipped: true,
                    bag_slot: None,
                },
            ],
        });
        inv.normalize_layout(&cat);

        assert!(!inv.items[0].equipped, "non-equippable item unequipped");
        assert_eq!(inv.items[0].bag_slot, Some(0));
        assert!(inv.items[1].equipped, "first two-handed weapon stays equipped");
        assert_eq!(inv.items[1].bag_slot, None);
        assert!(!inv.items[2].equipped, "conflicting off-hand cleared");
        assert_eq!(inv.items[2].bag_slot, Some(1));
        assert!(!inv.items[3].equipped, "conflicting main-hand cleared");
        assert_eq!(inv.items[3].bag_slot, Some(2));
    }

    #[test]
    fn bag_slots_reorder_swap_merge_and_split_stack() {
        let cat = test_catalog();
        let mut inv = InventoryComponent::default();
        inv.give_item("ration_pack", 6, 10, false); // slot 0
        inv.give_item("scrap_blade", 1, 1, false);  // slot 1

        // Split 2 rations from slot 0 into empty slot 4.
        inv.split_bag_stack(0, 4, 2, &cat).unwrap();
        assert_eq!(inv.items.iter().find(|e| e.bag_slot == Some(0)).unwrap().count, 4);
        assert_eq!(inv.items.iter().find(|e| e.bag_slot == Some(4)).unwrap().count, 2);

        // Swap slot 4 (rations) and slot 1 (blade).
        inv.move_bag_slot(4, 1, &cat).unwrap();
        assert_eq!(inv.items.iter().find(|e| e.bag_slot == Some(1)).unwrap().item_id, "ration_pack");
        assert_eq!(inv.items.iter().find(|e| e.bag_slot == Some(4)).unwrap().item_id, "scrap_blade");

        // Merge slot 1 (2 rations) back into slot 0 (4 rations) -> 6 rations at slot 0.
        inv.move_bag_slot(1, 0, &cat).unwrap();
        assert_eq!(inv.items.iter().find(|e| e.bag_slot == Some(0)).unwrap().count, 6);
        assert!(inv.items.iter().all(|e| e.bag_slot != Some(1)));

        // Move slot 4 to empty slot 9.
        inv.move_bag_slot(4, 9, &cat).unwrap();
        assert_eq!(inv.items.iter().find(|e| e.bag_slot == Some(9)).unwrap().item_id, "scrap_blade");

        // Reject invalid split.
        assert!(inv.split_bag_stack(0, 9, 2, &cat).is_err());
        assert!(inv.split_bag_stack(0, 2, 6, &cat).is_err());
    }

    #[test]
    fn equip_from_bag_swaps_displaced_and_two_handed_into_free_slots() {
        let cat = test_catalog();
        let mut inv = InventoryComponent::default();
        inv.give_item("scrap_blade", 1, 1, true);
        inv.give_item("riot_shield", 1, 1, true);
        inv.give_item("guard_rifle", 1, 1, false); // bag_slot 0

        // Reject mismatched target slot.
        assert!(inv.equip_from_bag(0, Some(EquipSlot::Body), &cat).is_err());

        // Equipping 2H rifle from slot 0 via OffHand slot succeeds (occupies_slot(OffHand) == true)
        // and places one displaced item at slot 0 + the second at slot 1.
        inv.equip_from_bag(0, Some(EquipSlot::OffHand), &cat).unwrap();
        assert_eq!(
            inv.equipped_in_slot(EquipSlot::MainHand, &cat).map(|e| e.item_id.as_str()),
            Some("guard_rifle")
        );
        let blade = inv.items.iter().find(|e| e.item_id == "scrap_blade").unwrap();
        let shield = inv.items.iter().find(|e| e.item_id == "riot_shield").unwrap();
        assert!(!blade.equipped);
        assert!(!shield.equipped);
        assert_eq!(blade.bag_slot, Some(0));
        assert_eq!(shield.bag_slot, Some(1));

        // Dragging equipped rifle from OffHand onto scrap_blade at slot 0 swaps them cleanly.
        inv.unequip_to_bag(EquipSlot::OffHand, Some(0), &cat).unwrap();
        assert_eq!(
            inv.equipped_in_slot(EquipSlot::MainHand, &cat).map(|e| e.item_id.as_str()),
            Some("scrap_blade")
        );
        assert_eq!(
            inv.items.iter().find(|e| e.item_id == "guard_rifle").unwrap().bag_slot,
            Some(0)
        );
    }

    #[test]
    fn unequip_to_bag_handles_backpack_capacity_shrink_and_full_bag_rejection() {
        let cat = test_catalog();
        let mut inv = InventoryComponent::default();
        inv.give_item("salvage_rig_pack", 1, 1, true);
        assert_eq!(inv.bag_capacity(&cat), 25);

        // Fill slots 0..19 (20 slots), leaving 20..24 free except put 1 item in slot 22.
        for _ in 0..18 {
            inv.give_item("scrap_blade", 1, 1, false); // slots 0..17
        }
        inv.give_item("ration_pack", 1, 10, false); // slot 18
        inv.give_item("riot_shield", 1, 1, false); // slot 19
        assert!(inv.first_free_bag_slot(25) == Some(20));

        // Move slot 19 to slot 22 so slot 19 is open (total 20 bag items + equipped pack = 21 items).
        inv.move_bag_slot(19, 22, &cat).unwrap();
        // Now unequipping the pack shrinks capacity 25 -> 20, needing 21 slots (fails cleanly!).
        assert!(inv.unequip_to_bag(EquipSlot::Back, None, &cat).is_err());
        assert_eq!(inv.bag_capacity(&cat), 25);

        // Free up 1 slot by taking the shield out, leaving 19 bag items.
        inv.take_item("riot_shield");
        // Unequipping pack now succeeds (20 items fit in 20 slots) and relocates nothing into >=20.
        inv.unequip_to_bag(EquipSlot::Back, None, &cat).unwrap();
        assert_eq!(inv.bag_capacity(&cat), 20);
        assert!(inv.items.iter().all(|e| !e.equipped && e.bag_slot.is_some_and(|s| s < 20)));
    }

    #[test]
    fn can_accept_item_enforces_weight_and_bag_capacity() {
        let cat = test_catalog();
        let mut inv = InventoryComponent::default();
        // 2 * 12.0 = 24.0 kg <= 30.0 kg -> ok; 3 * 12.0 = 36.0 kg > 30.0 kg -> too heavy.
        assert!(inv.can_accept_item("lead_ingot", 2, &cat, false).is_ok());
        inv.give_item("lead_ingot", 2, 10, false);
        assert_eq!(inv.total_weight(&cat), 24.0);
        assert!(inv.can_accept_item("lead_ingot", 1, &cat, false).is_err());
        assert!(inv.can_accept_item("guard_rifle", 1, &cat, true).is_ok());
        // Adding 15 ration packs (7.5 kg) exceeds 30.0 kg (24 + 7.5 = 31.5).
        assert!(inv.can_accept_item("ration_pack", 15, &cat, false).is_err());

        // Also test slot capacity limit with weightless/light items.
        let mut inv2 = InventoryComponent::default();
        for _ in 0..20 {
            inv2.give_item("throwing_knife", 1, 1, false); // max_stack 1 passed manually -> 20 slots full
        }
        assert!(inv2.can_accept_item("scrap_blade", 1, &cat, false).is_err());
        assert!(inv2.can_accept_item("salvage_rig_pack", 1, &cat, true).is_ok());
    }
}
