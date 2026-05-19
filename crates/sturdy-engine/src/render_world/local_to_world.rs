use super::GpuObjectId;

/// ECS component linking an entity to its persistent GPU object slot.
///
/// This intentionally stores a handle, not a CPU matrix. Rendering reads compact
/// source data from ECS, writes it into the render world, and later GPU compute
/// can expand that into world/previous/normal matrices and bounds.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct LocalToWorld {
    pub object: GpuObjectId,
}

impl LocalToWorld {
    pub fn new(object: GpuObjectId) -> Self {
        Self { object }
    }
}
