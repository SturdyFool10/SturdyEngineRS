use glam::Vec3;

/// A directional light illuminating the entire scene.
///
/// Set via [`Scene::directional_light`](super::Scene::directional_light) before
/// drawing. The default is a warm sunlight from above-right with a dark-grey
/// ambient.
#[derive(Clone, Debug)]
pub struct DirectionalLight {
    /// World-space direction the light shines **toward** (normalized before upload).
    pub direction: Vec3,
    /// Diffuse + specular colour of the light.
    pub color: Vec3,
    /// Multiplier applied to both diffuse and specular contributions.
    pub intensity: f32,
    /// Constant ambient term added to every fragment regardless of normal.
    pub ambient: Vec3,
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self {
            direction: Vec3::new(0.45, -0.75, -0.55).normalize(),
            color: Vec3::new(1.0, 0.95, 0.88),
            intensity: 1.0,
            ambient: Vec3::new(0.08, 0.08, 0.10),
        }
    }
}

/// An omnidirectional point light with physically correct inverse-square falloff.
///
/// Add to a scene via `scene.point_lights.push(PointLight { ... })`.
/// Changes take effect from the next `DeferredPass::draw()` call.
#[derive(Clone, Debug)]
pub struct PointLight {
    /// World-space position.
    pub position: Vec3,
    /// Linear RGB colour (linear, not sRGB).
    pub color: Vec3,
    /// Luminous intensity in candela (or a relative scale when `intensity` < 1000).
    pub intensity: f32,
    /// Maximum influence radius in world units. The light is clamped to zero at this distance.
    pub range: f32,
}

impl Default for PointLight {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            color: Vec3::ONE,
            intensity: 100.0,
            range: 10.0,
        }
    }
}

/// A cone-shaped spot light with inner (full intensity) and outer (zero intensity) angles.
///
/// Add to a scene via `scene.spot_lights.push(SpotLight { ... })`.
#[derive(Clone, Debug)]
pub struct SpotLight {
    /// World-space position of the light source.
    pub position: Vec3,
    /// World-space direction the spot **shines toward** (normalized before upload).
    pub direction: Vec3,
    /// Linear RGB colour.
    pub color: Vec3,
    /// Luminous intensity in candela (or a relative scale).
    pub intensity: f32,
    /// Maximum influence radius in world units.
    pub range: f32,
    /// Inner cone half-angle in radians: full intensity inside this cone.
    pub inner_angle: f32,
    /// Outer cone half-angle in radians: zero intensity outside this cone.
    pub outer_angle: f32,
}

impl Default for SpotLight {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            direction: Vec3::new(0.0, -1.0, 0.0),
            color: Vec3::ONE,
            intensity: 500.0,
            range: 20.0,
            inner_angle: 0.35,
            outer_angle: 0.52,
        }
    }
}

/// A rectangular area light. Illuminates via polygon solid-angle diffuse +
/// representative-point specular (same BVH as point/spot lights).
///
/// `right` and `up` define the local axes; the light surface spans
/// `[-half_extents.x, +half_extents.x] x [-half_extents.y, +half_extents.y]`.
#[derive(Clone, Debug)]
pub struct RectLight {
    /// World-space center.
    pub position: Vec3,
    /// Local X axis (right direction), unit vector.
    pub right: Vec3,
    /// Local Y axis (up direction), unit vector.
    pub up: Vec3,
    /// [half_width, half_height]: half-dimensions along `right` and `up`.
    pub half_extents: [f32; 2],
    /// Emitted radiance (linear RGB).
    pub color: Vec3,
    /// Intensity multiplier. Default 1.0.
    pub intensity: f32,
    /// Maximum influence distance in world units.
    pub range: f32,
    /// If true, illuminates from both sides of the rectangle.
    pub two_sided: bool,
}

impl Default for RectLight {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            right: Vec3::X,
            up: Vec3::Y,
            half_extents: [0.5, 0.5],
            color: Vec3::ONE,
            intensity: 1.0,
            range: 10.0,
            two_sided: false,
        }
    }
}

/// A spherical area light (emissive sphere, e.g. bare bulb, sun disc).
///
/// `radius` is the source radius (controls apparent size and specular highlight
/// softness). `range` is the maximum influence distance.
#[derive(Clone, Debug)]
pub struct SphereLight {
    pub position: Vec3,
    /// Emissive radius of the source (not the influence radius). Default 0.1.
    pub radius: f32,
    pub color: Vec3,
    pub intensity: f32,
    /// Maximum influence distance.
    pub range: f32,
}

impl Default for SphereLight {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            radius: 0.1,
            color: Vec3::ONE,
            intensity: 100.0,
            range: 10.0,
        }
    }
}

/// A disc (circular) area light, e.g. a recessed ceiling downlight.
#[derive(Clone, Debug)]
pub struct DiskLight {
    pub position: Vec3,
    /// Outward normal of the disc surface.
    pub normal: Vec3,
    /// Disc radius.
    pub radius: f32,
    pub color: Vec3,
    pub intensity: f32,
    pub range: f32,
    pub two_sided: bool,
}

impl Default for DiskLight {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            normal: Vec3::NEG_Y,
            radius: 0.3,
            color: Vec3::ONE,
            intensity: 500.0,
            range: 10.0,
            two_sided: false,
        }
    }
}

// Matches GpuLightData in deferred_lighting.slang exactly (96 bytes = 6 x float4).
//
// Layout (backward-compatible: bytes 0-63 unchanged from the old 64-byte struct):
//   0-15:  color_intensity   rgb x intensity
//   16-31: position_range    world pos + influence range
//   32-47: direction_kind    dir/normal xyz + kind (float: 0=dir,1=pt,2=spot,3=rect,4=sphere,5=disk)
//   48-63: misc              [cos_inner, cos_outer, area_radius, two_sided 0/1]
//   64-79: right_half_w      rect: right.xyz + half_width;  others: zero
//   80-95: up_half_h         rect: up.xyz + half_height;    others: zero
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GpuLightData {
    color_intensity: [f32; 4],
    position_range: [f32; 4],
    direction_kind: [f32; 4],
    misc: [f32; 4],
    right_half_w: [f32; 4],
    up_half_h: [f32; 4],
}

impl GpuLightData {
    pub(crate) fn from_directional(light: &DirectionalLight) -> Self {
        let toward = (-light.direction).normalize();
        let lum = light.intensity;
        Self {
            color_intensity: [
                light.color.x * lum,
                light.color.y * lum,
                light.color.z * lum,
                1.0,
            ],
            position_range: [toward.x, toward.y, toward.z, 0.0],
            direction_kind: [0.0, 0.0, 0.0, 0.0],
            misc: [1.0, 1.0, 0.0, 0.0],
            right_half_w: [0.0; 4],
            up_half_h: [0.0; 4],
        }
    }

    pub(crate) fn from_point(light: &PointLight) -> Self {
        let lum = light.intensity;
        Self {
            color_intensity: [
                light.color.x * lum,
                light.color.y * lum,
                light.color.z * lum,
                1.0,
            ],
            position_range: [
                light.position.x,
                light.position.y,
                light.position.z,
                light.range,
            ],
            direction_kind: [0.0, 0.0, 0.0, 1.0],
            misc: [1.0, 1.0, 0.0, 0.0],
            right_half_w: [0.0; 4],
            up_half_h: [0.0; 4],
        }
    }

    pub(crate) fn from_spot(light: &SpotLight) -> Self {
        let lum = light.intensity;
        let dir = light.direction.normalize();
        Self {
            color_intensity: [
                light.color.x * lum,
                light.color.y * lum,
                light.color.z * lum,
                1.0,
            ],
            position_range: [
                light.position.x,
                light.position.y,
                light.position.z,
                light.range,
            ],
            direction_kind: [dir.x, dir.y, dir.z, 2.0],
            misc: [light.inner_angle.cos(), light.outer_angle.cos(), 0.0, 0.0],
            right_half_w: [0.0; 4],
            up_half_h: [0.0; 4],
        }
    }

    pub(crate) fn from_rect(light: &RectLight) -> Self {
        let lum = light.intensity;
        let n = light.right.cross(light.up).normalize();
        let r = light.right.normalize();
        let u = light.up.normalize();
        Self {
            color_intensity: [
                light.color.x * lum,
                light.color.y * lum,
                light.color.z * lum,
                1.0,
            ],
            position_range: [
                light.position.x,
                light.position.y,
                light.position.z,
                light.range,
            ],
            direction_kind: [n.x, n.y, n.z, 3.0],
            misc: [0.0, 0.0, 0.0, if light.two_sided { 1.0 } else { 0.0 }],
            right_half_w: [r.x, r.y, r.z, light.half_extents[0]],
            up_half_h: [u.x, u.y, u.z, light.half_extents[1]],
        }
    }

    pub(crate) fn from_sphere_area(light: &SphereLight) -> Self {
        let lum = light.intensity;
        Self {
            color_intensity: [
                light.color.x * lum,
                light.color.y * lum,
                light.color.z * lum,
                1.0,
            ],
            position_range: [
                light.position.x,
                light.position.y,
                light.position.z,
                light.range,
            ],
            direction_kind: [0.0, 0.0, 0.0, 4.0],
            misc: [1.0, 1.0, light.radius, 0.0],
            right_half_w: [0.0; 4],
            up_half_h: [0.0; 4],
        }
    }

    pub(crate) fn from_disk(light: &DiskLight) -> Self {
        let lum = light.intensity;
        let n = light.normal.normalize();
        Self {
            color_intensity: [
                light.color.x * lum,
                light.color.y * lum,
                light.color.z * lum,
                1.0,
            ],
            position_range: [
                light.position.x,
                light.position.y,
                light.position.z,
                light.range,
            ],
            direction_kind: [n.x, n.y, n.z, 5.0],
            misc: [
                0.0,
                0.0,
                light.radius,
                if light.two_sided { 1.0 } else { 0.0 },
            ],
            right_half_w: [0.0; 4],
            up_half_h: [0.0; 4],
        }
    }
}
