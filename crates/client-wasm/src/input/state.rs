use std::collections::HashSet;

#[derive(Debug, Default, Clone)]
pub struct InputState {
    pub keys: HashSet<String>,
    pub middle_mouse_down: bool,
    pub left_mouse_down: bool,
    pub left_just_pressed: bool,
    pub left_just_released: bool,
    pub mouse_x: f32,
    pub mouse_y: f32,
    pub mouse_delta: (f32, f32),
    pub scroll_delta: f32,
    pub canvas_width: f32,
    pub canvas_height: f32,
}

impl InputState {
    pub fn end_frame(&mut self) {
        self.mouse_delta = (0.0, 0.0);
        self.scroll_delta = 0.0;
        self.left_just_pressed = false;
        self.left_just_released = false;
    }

    pub fn is_key_down(&self, code: &str) -> bool {
        self.keys.contains(code)
    }
}
