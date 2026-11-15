use crate::input::InputState;

use super::projection::Camera;

pub trait CameraController {
    fn update(&mut self, camera: &mut Camera, input: &InputState, dt: f32);
}
