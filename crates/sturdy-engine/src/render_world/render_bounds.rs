use crate::{BoundingSphere, Mesh};

use super::Aabb;

/// ECS component containing conservative local-space render bounds.
#[derive(Copy, Clone, Debug)]
pub struct RenderBounds {
    pub local_sphere: BoundingSphere,
    pub local_aabb: Aabb,
}

impl RenderBounds {
    pub fn new(local_sphere: BoundingSphere, local_aabb: Aabb) -> Self {
        Self {
            local_sphere,
            local_aabb,
        }
    }

    pub fn from_sphere(local_sphere: BoundingSphere) -> Self {
        let radius = glam::Vec3::splat(local_sphere.radius);
        Self {
            local_sphere,
            local_aabb: Aabb::from_center_extents(local_sphere.center, radius),
        }
    }

    pub fn from_aabb(local_aabb: Aabb) -> Self {
        let center = local_aabb.center();
        let radius = local_aabb.extents().length();
        Self {
            local_sphere: BoundingSphere { center, radius },
            local_aabb,
        }
    }

    pub fn from_mesh(mesh: &Mesh) -> Self {
        Self::from_sphere(mesh.bounding_sphere)
    }
}

impl Default for RenderBounds {
    fn default() -> Self {
        Self::from_sphere(BoundingSphere::EMPTY)
    }
}
