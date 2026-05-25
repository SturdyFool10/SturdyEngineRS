/// Allocation statistics for scene-wide render-world GPU cull output buffers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderWorldGpuCullOutputStats {
    pub object_count: usize,
    pub visibility_capacity: usize,
    pub visibility_reallocated: bool,
    pub output_bytes: u64,
}

/// Dispatch result for the scene-wide render-world cull pass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderWorldGpuCullDispatchStats {
    pub dispatched: bool,
    pub object_count: u32,
    pub workgroup_count: u32,
    pub objects_per_workgroup: u32,
    pub skipped_reason: Option<String>,
}
