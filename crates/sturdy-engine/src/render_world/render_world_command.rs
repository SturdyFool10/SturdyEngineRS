use crate::ecs::Transform;

use super::{
    GpuObjectId, PreviousTransform, RenderBounds, RenderMaterial, RenderMesh, RenderVisibility,
};

/// Deferred mutation for [`RenderWorld`](super::RenderWorld).
#[derive(Clone, Debug)]
pub enum RenderWorldCommand {
    CreateObject(GpuObjectId),
    ReleaseObject(GpuObjectId),
    SetTransform(GpuObjectId, Transform),
    SetPreviousTransform(GpuObjectId, PreviousTransform),
    SetMesh(GpuObjectId, RenderMesh),
    SetMaterial(GpuObjectId, RenderMaterial),
    SetBounds(GpuObjectId, RenderBounds),
    SetVisibility(GpuObjectId, RenderVisibility),
}
