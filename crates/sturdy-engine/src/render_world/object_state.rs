use crate::ecs::Transform;

use super::{
    GpuObjectId, PreviousTransform, RenderBounds, RenderDirtyFlags, RenderMaterial, RenderMesh,
    RenderVisibility,
};

/// CPU-side source data for one persistent GPU object slot.
#[derive(Clone, Debug)]
pub struct RenderObjectState {
    pub object: GpuObjectId,
    pub transform: Option<Transform>,
    pub previous_transform: Option<PreviousTransform>,
    pub mesh: Option<RenderMesh>,
    pub material: Option<RenderMaterial>,
    pub bounds: Option<RenderBounds>,
    pub visibility: RenderVisibility,
    pub dirty: RenderDirtyFlags,
}

impl RenderObjectState {
    pub fn new(object: GpuObjectId) -> Self {
        Self {
            object,
            transform: None,
            previous_transform: None,
            mesh: None,
            material: None,
            bounds: None,
            visibility: RenderVisibility::default(),
            dirty: RenderDirtyFlags::STRUCTURAL,
        }
    }

    pub fn clear_dirty(&mut self) {
        self.dirty.clear();
    }
}
