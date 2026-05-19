use crate::{BoundingSphere, GpuInstanceData};

/// CPU-side metadata that travels alongside one legacy scene instance.
///
/// `InstanceData` remains matrix-only because existing vertex shaders read that
/// buffer directly. This metadata feeds the flat `GpuInstanceData` render-world
/// buffer used by culling and future GPU-driven paths.
#[derive(Copy, Clone, Debug)]
pub(super) struct SceneInstanceMetadata {
    pub local_sphere: BoundingSphere,
    pub material_id: u32,
    pub lod_bias: f32,
    pub flags: u32,
}

impl SceneInstanceMetadata {
    pub fn new(local_sphere: BoundingSphere, material_id: u32, flags: u32) -> Self {
        Self {
            local_sphere,
            material_id,
            lod_bias: 0.0,
            flags,
        }
    }

    pub fn with_lod_bias(mut self, lod_bias: f32) -> Self {
        self.lod_bias = lod_bias;
        self
    }

    pub fn dynamic_default(local_sphere: BoundingSphere, material_id: u32) -> Self {
        Self::new(
            local_sphere,
            material_id,
            GpuInstanceData::FLAG_DYNAMIC
                | GpuInstanceData::FLAG_CAST_SHADOW
                | GpuInstanceData::FLAG_RECEIVE_SHADOW,
        )
    }
}
