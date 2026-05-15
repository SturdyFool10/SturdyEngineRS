use glam::{Mat4, Vec3};

use super::{camera::SceneCamera, lights::DirectionalLight};
use crate::push_constants;

/// GPU-layout mirror of the data read by `lit_fragment.slang` (forward path only).
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct LightingUniforms {
    dir_direction: [f32; 4],
    dir_color: [f32; 4],
    ambient: [f32; 4],
    camera_world_pos: [f32; 4],
}

impl LightingUniforms {
    pub(super) fn from_light_and_camera(light: &DirectionalLight, camera_world_pos: Vec3) -> Self {
        let dir = (-light.direction).normalize();
        Self {
            dir_direction: [dir.x, dir.y, dir.z, light.intensity],
            dir_color: [light.color.x, light.color.y, light.color.z, 0.0],
            ambient: [light.ambient.x, light.ambient.y, light.ambient.z, 0.0],
            camera_world_pos: [
                camera_world_pos.x,
                camera_world_pos.y,
                camera_world_pos.z,
                0.0,
            ],
        }
    }
}

/// Push constants sent to the vertex and fragment shaders for each camera draw call.
///
/// Shaders must declare `uniform CameraConstants cam` to receive these values.
/// `time` is the elapsed time in seconds since application start: available
/// in fragment shaders for procedural animation and UV scrolling.
#[push_constants]
pub struct CameraConstants {
    pub view_proj: [[f32; 4]; 4],
    pub previous_view_proj: [[f32; 4]; 4],
    /// Elapsed time in seconds (application start = 0). Use in material expressions.
    pub time: f32,
    pub _pad: [f32; 3],
}

impl CameraConstants {
    pub fn identity() -> Self {
        Self {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            previous_view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            time: 0.0,
            _pad: [0.0; 3],
        }
    }

    pub fn from_camera(camera: &SceneCamera) -> Self {
        Self {
            view_proj: camera.view_proj().to_cols_array_2d(),
            previous_view_proj: camera.previous_view_proj.to_cols_array_2d(),
            time: 0.0,
            _pad: [0.0; 3],
        }
    }

    pub fn with_time(mut self, time: f32) -> Self {
        self.time = time;
        self
    }
}
