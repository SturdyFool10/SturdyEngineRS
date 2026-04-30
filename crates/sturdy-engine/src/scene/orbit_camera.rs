use glam::{Mat4, Vec3};

/// Spherical-coordinate camera that orbits a fixed target point.
///
/// Suitable for model viewers, level editors, and any scene where you want to
/// inspect geometry from all angles with mouse drag + scroll wheel.
///
/// # Usage
/// ```ignore
/// let mut cam = OrbitCamera::new(Vec3::ZERO, 5.0);
///
/// // In pointer_moved / pointer_button:
/// if primary_held { cam.on_drag(dx, dy, 0.005); }
///
/// // In pointer_scroll:
/// cam.on_scroll(delta_y, 0.1);
///
/// // In render:
/// let view = cam.view_matrix();
/// let proj = cam.projection_matrix(width as f32 / height as f32);
/// ```
pub struct OrbitCamera {
    /// The point the camera orbits around.
    pub target: Vec3,
    /// Distance from target to eye.
    pub distance: f32,
    /// Horizontal rotation in radians (yaw around world Y).
    pub yaw: f32,
    /// Vertical rotation in radians (pitch above the XZ plane, clamped).
    pub pitch: f32,
    /// Vertical field of view in radians.
    pub fov_y: f32,
    /// Near clip plane distance.
    pub near: f32,
    /// Far clip plane distance.
    pub far: f32,
    /// Minimum allowed distance (prevents clipping through target).
    pub min_distance: f32,
    /// Maximum allowed distance.
    pub max_distance: f32,
}

impl OrbitCamera {
    /// Create an orbit camera looking at `target` from `distance` units away.
    ///
    /// Default pitch is 20° above horizontal, yaw is 45°, fov is 60°.
    pub fn new(target: Vec3, distance: f32) -> Self {
        Self {
            target,
            distance,
            yaw: std::f32::consts::FRAC_PI_4,
            pitch: 0.35,
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.1,
            far: 1000.0,
            min_distance: 0.1,
            max_distance: f32::MAX,
        }
    }

    /// Current eye position in world space.
    pub fn position(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        self.target + Vec3::new(cp * cy, sp, cp * sy) * self.distance
    }

    /// View matrix (world → camera space).
    pub fn view_matrix(&self) -> Mat4 {
        Mat4::look_at_rh(self.position(), self.target, Vec3::Y)
    }

    /// Perspective projection matrix for the given aspect ratio (width / height).
    pub fn projection_matrix(&self, aspect: f32) -> Mat4 {
        Mat4::perspective_rh(self.fov_y, aspect.max(f32::EPSILON), self.near, self.far)
    }

    /// Rotate the orbit by a pointer drag delta.
    ///
    /// `dx` and `dy` are cursor deltas in logical pixels. `sensitivity` scales
    /// them to radians — `0.005` is a reasonable default.
    pub fn on_drag(&mut self, dx: f32, dy: f32, sensitivity: f32) {
        self.yaw += dx * sensitivity;
        self.pitch = (self.pitch - dy * sensitivity)
            .clamp(-std::f32::consts::FRAC_PI_2 + 0.01, std::f32::consts::FRAC_PI_2 - 0.01);
    }

    /// Zoom by adjusting distance.
    ///
    /// `delta` is in logical scroll units (positive = zoom in). `sensitivity`
    /// scales the effect — `0.1` works well for trackpad, `1.0` for wheel clicks.
    pub fn on_scroll(&mut self, delta: f32, sensitivity: f32) {
        self.distance = (self.distance * (1.0 - delta * sensitivity))
            .clamp(self.min_distance, self.max_distance);
    }
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self::new(Vec3::ZERO, 5.0)
    }
}
