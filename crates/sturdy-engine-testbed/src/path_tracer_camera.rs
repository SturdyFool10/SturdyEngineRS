use bytemuck::{Pod, Zeroable};
use glam::Vec3;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct PathTracerCameraGpu {
    pub origin: [f32; 4],
    pub forward: [f32; 4],
    pub right: [f32; 4],
    pub up: [f32; 4],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PathTracerCameraInput {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
    pub yaw_left: bool,
    pub yaw_right: bool,
    pub pitch_up: bool,
    pub pitch_down: bool,
    pub fast: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct PathTracerCameraRig {
    position: Vec3,
    yaw: f32,
    pitch: f32,
    move_speed: f32,
    look_speed: f32,
    tan_half_fov_y: f32,
}

impl PathTracerCameraRig {
    pub fn outdoor_default() -> Self {
        let position = Vec3::new(0.0, 2.15, -8.8);
        let target = Vec3::new(0.0, 0.95, 3.8);
        let forward = (target - position).normalize_or_zero();
        Self {
            position,
            yaw: forward.x.atan2(forward.z),
            pitch: forward.y.asin(),
            move_speed: 3.2,
            look_speed: 1.45,
            tan_half_fov_y: 0.58,
        }
    }

    pub fn apply_input(&mut self, input: PathTracerCameraInput, dt_seconds: f32) -> bool {
        let dt = dt_seconds.clamp(0.0, 0.1);
        if dt <= 0.0 {
            return false;
        }

        let yaw_axis = axis(input.yaw_right, input.yaw_left);
        let pitch_axis = axis(input.pitch_up, input.pitch_down);
        let mut rotated = false;
        if yaw_axis != 0.0 || pitch_axis != 0.0 {
            self.yaw += yaw_axis * self.look_speed * dt;
            self.pitch = (self.pitch + pitch_axis * self.look_speed * dt).clamp(-1.35, 1.35);
            rotated = true;
        }

        let forward_axis = axis(input.forward, input.backward);
        let strafe_axis = axis(input.right, input.left);
        let vertical_axis = axis(input.up, input.down);
        let mut movement = Vec3::ZERO;
        if forward_axis != 0.0 || strafe_axis != 0.0 || vertical_axis != 0.0 {
            movement += self.horizontal_forward() * forward_axis;
            movement += self.horizontal_right() * strafe_axis;
            movement += Vec3::Y * vertical_axis;
        }

        let mut translated = false;
        if movement.length_squared() > 1e-6 {
            let speed = self.move_speed * if input.fast { 3.0 } else { 1.0 };
            self.position += movement.normalize() * speed * dt;
            self.position.y = self.position.y.max(0.08);
            translated = true;
        }

        rotated || translated
    }

    pub fn gpu_data(self) -> PathTracerCameraGpu {
        let forward = self.forward();
        let right = normalize_or(Vec3::Y.cross(forward), Vec3::X);
        let up = normalize_or(forward.cross(right), Vec3::Y);
        PathTracerCameraGpu {
            origin: [
                self.position.x,
                self.position.y,
                self.position.z,
                self.tan_half_fov_y,
            ],
            forward: [forward.x, forward.y, forward.z, 0.0],
            right: [right.x, right.y, right.z, 0.0],
            up: [up.x, up.y, up.z, 0.0],
        }
    }

    pub fn position(self) -> [f32; 3] {
        self.position.to_array()
    }

    fn forward(self) -> Vec3 {
        let cos_pitch = self.pitch.cos();
        let forward = Vec3::new(
            self.yaw.sin() * cos_pitch,
            self.pitch.sin(),
            self.yaw.cos() * cos_pitch,
        );
        normalize_or(forward, Vec3::Z)
    }

    fn horizontal_forward(self) -> Vec3 {
        normalize_or(Vec3::new(self.yaw.sin(), 0.0, self.yaw.cos()), Vec3::Z)
    }

    fn horizontal_right(self) -> Vec3 {
        normalize_or(Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin()), Vec3::X)
    }
}

fn axis(positive: bool, negative: bool) -> f32 {
    positive as i32 as f32 - negative as i32 as f32
}

fn normalize_or(value: Vec3, fallback: Vec3) -> Vec3 {
    if value.length_squared() > 1e-8 {
        value.normalize()
    } else {
        fallback
    }
}
