use glam::{Mat4, Quat, Vec3};

use crate::ecs::Transform;

/// ECS component storing the previous extracted transform for motion vectors.
#[derive(Clone, Debug)]
pub struct PreviousTransform {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl PreviousTransform {
    pub const IDENTITY: Self = Self {
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    pub fn from_transform(transform: &Transform) -> Self {
        Self {
            position: transform.position,
            rotation: transform.rotation,
            scale: transform.scale,
        }
    }

    pub fn to_transform(&self) -> Transform {
        Transform {
            position: self.position,
            rotation: self.rotation,
            scale: self.scale,
        }
    }

    pub fn to_mat4(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.position)
    }
}

impl Default for PreviousTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}
