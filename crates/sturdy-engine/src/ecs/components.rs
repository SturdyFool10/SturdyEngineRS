// Built-in ECS components.
//
// These cover the most common game-object data. All implement `Component`
// automatically (any `'static + Send + Sync` type does).
//
// Add custom components by defining your own structs and using them directly
// with `world.spawn().with(MyComponent { ... }).id()`.

use glam::{Mat4, Quat, Vec3};

use super::storage::Component;
use crate::ObjectId;

// ── Auto-impl Component for all suitable types ────────────────────────────────

// Blanket implementation: every 'static + Send + Sync type is a component.
impl<T: std::any::Any + Send + Sync + 'static> Component for T {}

// ── Transform ─────────────────────────────────────────────────────────────────

/// 3-D world-space position, orientation, and scale.
///
/// Use `sync_scene_transforms` to flush all `(Transform, SceneLink)` pairs to
/// the render `Scene` each frame.
///
/// # Example
/// ```ignore
/// let t = Transform::from_position([1.0, 0.0, 3.0]);
/// let t = Transform {
///     position: Vec3::new(0.0, 2.0, 0.0),
///     rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
///     scale:    Vec3::ONE,
/// };
/// ```
#[derive(Clone, Debug)]
pub struct Transform {
    /// World-space position.
    pub position: Vec3,
    /// Orientation as a unit quaternion. Default = no rotation.
    pub rotation: Quat,
    /// Non-uniform scale. Default = `[1, 1, 1]`.
    pub scale: Vec3,
}

impl Transform {
    /// Identity transform — origin, no rotation, unit scale.
    pub const IDENTITY: Self = Self {
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    /// Construct at `position` with identity rotation and unit scale.
    pub fn from_position(position: impl Into<Vec3>) -> Self {
        Self {
            position: position.into(),
            ..Self::IDENTITY
        }
    }

    /// Construct from a full TRS triple.
    pub fn from_trs(position: Vec3, rotation: Quat, scale: Vec3) -> Self {
        Self {
            position,
            rotation,
            scale,
        }
    }

    /// Convert to a column-major 4×4 matrix suitable for `Scene::set_transform`.
    pub fn to_mat4(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.position)
    }

    /// Translate by `delta` in world space.
    pub fn translate(&mut self, delta: Vec3) {
        self.position += delta;
    }

    /// Rotate by `rotation` in local space.
    pub fn rotate(&mut self, rotation: Quat) {
        self.rotation = (self.rotation * rotation).normalize();
    }

    /// Look toward `target` from the current position. `up` is the world-up vector.
    pub fn look_at(&mut self, target: Vec3, up: Vec3) {
        let to_target = target - self.position;
        if to_target.length_squared() <= 1e-12 {
            return;
        }
        let forward = to_target.normalize();
        let mut up = if up.length_squared() > 1e-12 {
            up.normalize()
        } else {
            Vec3::Y
        };
        if forward.cross(up).length_squared() <= 1e-10 {
            up = fallback_up(forward);
        }

        let right = forward.cross(up).normalize();
        let corrected_up = right.cross(forward).normalize();
        self.rotation =
            Quat::from_mat3(&glam::Mat3::from_cols(right, corrected_up, -forward)).normalize();
    }

    /// Return the forward direction in world space (−Z by convention).
    pub fn forward(&self) -> Vec3 {
        self.rotation * Vec3::NEG_Z
    }

    /// Return the right direction in world space (+X by convention).
    pub fn right(&self) -> Vec3 {
        self.rotation * Vec3::X
    }

    /// Return the up direction in world space (+Y by convention).
    pub fn up(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }
}

fn fallback_up(forward: Vec3) -> Vec3 {
    if forward.dot(Vec3::Y).abs() < 0.99 {
        Vec3::Y
    } else {
        Vec3::Z
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

// ── LocalTransform ────────────────────────────────────────────────────────────

/// Transform relative to a parent entity.
///
/// If an entity has both `Transform` (world) and `LocalTransform`, the engine
/// computes the world transform as `parent_world × local`. Systems are
/// responsible for propagating this; use the `propagate_local_transforms`
/// built-in system.
#[derive(Clone, Debug)]
pub struct LocalTransform {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
    /// Parent entity (must be alive and have a `Transform`).
    pub parent: Option<super::Entity>,
}

impl Default for LocalTransform {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
            parent: None,
        }
    }
}

// ── Velocity ──────────────────────────────────────────────────────────────────

/// Linear and angular velocity for Newtonian integration.
///
/// Integrate with `Transform` each fixed step:
/// ```ignore
/// // In a system:
/// for (_, (transform, velocity)) in world.query2_mut::<Transform, Velocity>() {
///     transform.position += velocity.linear * fixed_step;
///     transform.rotation = (transform.rotation
///         * Quat::from_scaled_axis(velocity.angular * fixed_step)).normalize();
/// }
/// ```
#[derive(Clone, Debug, Default)]
pub struct Velocity {
    /// Metres per second, world space.
    pub linear: Vec3,
    /// Radians per second, world space (axis-angle).
    pub angular: Vec3,
}

impl Velocity {
    pub fn linear(linear: Vec3) -> Self {
        Self {
            linear,
            angular: Vec3::ZERO,
        }
    }
}

// ── Acceleration ──────────────────────────────────────────────────────────────

/// Per-frame force accumulator. Cleared each fixed step after integration.
#[derive(Clone, Debug, Default)]
pub struct Acceleration {
    pub linear: Vec3,
}

// ── SceneLink ─────────────────────────────────────────────────────────────────

/// Links an ECS entity to a render-scene `ObjectId`.
///
/// When this component is present, `World::sync_scene_transforms` will
/// propagate the entity's `Transform` to `scene.set_transform(object_id, ...)`.
///
/// # Creating a linked entity
/// ```ignore
/// // Create the render object first, then link the ECS entity to it.
/// let mesh_id = scene.add_mesh(mesh, program);
/// let object_id = scene.add_object(mesh_id, ObjectKind::Dynamic);
///
/// let entity = world.spawn()
///     .with(Transform::from_position([0.0, 1.0, 0.0]))
///     .with(SceneLink { object_id })
///     .id();
/// ```
#[derive(Clone, Debug, Copy)]
pub struct SceneLink {
    /// The render-scene handle to sync this entity's `Transform` to.
    pub object_id: ObjectId,
}

// ── Name ──────────────────────────────────────────────────────────────────────

/// Human-readable debug label for an entity.
///
/// Not required for any engine functionality; useful for debugging and tooling.
#[derive(Clone, Debug)]
pub struct Name(pub String);

impl Name {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Name {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

// ── Active ────────────────────────────────────────────────────────────────────

/// Whether an entity participates in systems. Default `true`.
///
/// Systems can check `world.has::<Active>(entity)` or query with
/// `world.get::<Active>(entity).map(|a| a.0).unwrap_or(true)` to skip
/// inactive entities.
#[derive(Clone, Debug, Copy)]
pub struct Active(pub bool);

impl Default for Active {
    fn default() -> Self {
        Self(true)
    }
}

// ── Health ────────────────────────────────────────────────────────────────────

/// Hit-point component with current/max tracking.
#[derive(Clone, Debug, Copy)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

impl Health {
    pub fn new(max: f32) -> Self {
        Self { current: max, max }
    }
    pub fn is_dead(&self) -> bool {
        self.current <= 0.0
    }
    pub fn fraction(&self) -> f32 {
        (self.current / self.max).clamp(0.0, 1.0)
    }

    pub fn damage(&mut self, amount: f32) {
        self.current = (self.current - amount).max(0.0);
    }

    pub fn heal(&mut self, amount: f32) {
        self.current = (self.current + amount).min(self.max);
    }
}

// ── Built-in systems (free functions) ─────────────────────────────────────────

use super::World;

/// Integrate `Velocity` into `Transform` over `dt` seconds.
///
/// Register in your `fixed_schedule`:
/// ```ignore
/// fixed_schedule.add_system("integrate", move |world| {
///     integrate_transforms(world, fixed_step.as_secs_f32());
/// });
/// ```
pub fn integrate_transforms(world: &mut World, dt: f32) {
    for (_, transform, vel) in world.query2_mut::<Transform, Velocity>() {
        transform.position += vel.linear * dt;
        if vel.angular.length_squared() > 1e-12 {
            transform.rotation =
                (transform.rotation * Quat::from_scaled_axis(vel.angular * dt)).normalize();
        }
    }
}

/// Propagate `LocalTransform` into `Transform` for entities with a parent.
///
/// Must run after the parent's `Transform` has been updated.
/// Register in the render or post-physics stage.
pub fn propagate_local_transforms(world: &mut World) {
    // Collect parents first to avoid borrow conflicts.
    let children: Vec<(super::Entity, LocalTransform)> = world
        .query::<LocalTransform>()
        .map(|(e, lt)| (e, lt.clone()))
        .collect();

    for (child, local) in children {
        let parent_mat = local
            .parent
            .and_then(|p| world.get::<Transform>(p))
            .map(|pt| pt.to_mat4())
            .unwrap_or(Mat4::IDENTITY);

        let local_mat =
            Mat4::from_scale_rotation_translation(local.scale, local.rotation, local.position);
        let world_mat = parent_mat * local_mat;
        let (scale, rotation, position) = world_mat.to_scale_rotation_translation();
        if let Some(t) = world.get_mut::<Transform>(child) {
            t.position = position;
            t.rotation = rotation;
            t.scale = scale;
        }
    }
}

/// Despawn entities whose `Health` has dropped to zero.
pub fn despawn_dead(world: &mut World) {
    let dead: Vec<super::Entity> = world
        .query::<Health>()
        .filter(|(_, h)| h.is_dead())
        .map(|(e, _)| e)
        .collect();
    for e in dead {
        world.despawn(e);
    }
}
