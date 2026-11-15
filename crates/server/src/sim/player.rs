#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PlayerSimState {
    pub player_id: u64,
    pub area_id: String,
    pub is_outside: bool,
    pub pos: [f32; 3],
}
