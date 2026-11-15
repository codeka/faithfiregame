use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use shared::world::{InsideSimData, TileSimData};

/// Configurable Outside simulation radius around each online player (`1` = `3×3` tiles = 9 tiles).
pub const OUTSIDE_SIM_RADIUS: i32 = 1;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PlayerSimState {
    pub player_id: u64,
    pub area_id: String,
    pub is_outside: bool,
    pub pos: [f32; 3],
}

pub struct SimWorld {
    world_dir: PathBuf,
    active_outside: HashMap<(String, i32, i32), TileSimData>,
    active_inside: HashMap<String, InsideSimData>,
    players: HashMap<u64, PlayerSimState>,
}

impl SimWorld {
    pub fn new(world_dir: PathBuf) -> Self {
        Self {
            world_dir,
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
        self.active_inside.insert(area_id.to_owned(), sim);
    }

    pub fn tick(&mut self) {
        // Demand-driven 3×3 Outside tile hibernation & Inside instance lifecycle.
        let mut needed_outside: HashSet<(String, i32, i32)> = HashSet::new();
        let mut needed_inside: HashSet<String> = HashSet::new();

        for player in self.players.values() {
            if player.is_outside {
                let tx = (player.pos[0] / 64.0).floor() as i32;
                let ty = (player.pos[2] / 64.0).floor() as i32;
                for dy in -OUTSIDE_SIM_RADIUS..=OUTSIDE_SIM_RADIUS {
                    for dx in -OUTSIDE_SIM_RADIUS..=OUTSIDE_SIM_RADIUS {
                        needed_outside.insert((player.area_id.clone(), tx + dx, ty + dy));
                    }
                }
            } else {
                needed_inside.insert(player.area_id.clone());
            }
        }

        self.active_outside.retain(|k, _| needed_outside.contains(k));
        self.active_inside.retain(|k, _| needed_inside.contains(k));

        for key in needed_outside {
            if !self.active_outside.contains_key(&key) {
                let sim_path = self
                    .world_dir
                    .join("areas")
                    .join(&key.0)
                    .join(format!("{},{}", key.1, key.2))
                    .join("sim.bin");
                if let Ok(bytes) = fs::read(sim_path) {
                    if let Ok(sim) = postcard::from_bytes::<TileSimData>(&bytes) {
                        self.active_outside.insert(key, sim);
                    }
                }
            }
        }
    }
}
