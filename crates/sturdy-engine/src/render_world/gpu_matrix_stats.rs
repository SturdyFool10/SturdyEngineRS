/// Upload/allocation statistics for render-world GPU matrix source preparation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderWorldGpuMatrixStats {
    /// Number of renderable objects included in the compact transform-source table.
    pub object_count: usize,
    /// Allocated transform-source object capacity.
    pub source_capacity: usize,
    /// `true` when the transform-source buffer was newly allocated or grown.
    pub source_reallocated: bool,
    /// `true` when one or more derived matrix/bounds buffers were newly allocated or grown.
    pub derived_reallocated: bool,
    /// `true` when all source objects were uploaded because layout/capacity changed.
    pub full_source_upload: bool,
    /// Number of dirty source ranges uploaded this prepare call.
    pub uploaded_source_ranges: usize,
    /// Number of source objects covered by uploaded ranges.
    pub uploaded_source_objects: usize,
    /// Number of source bytes uploaded this prepare call.
    pub uploaded_source_bytes: u64,
    /// `true` when the selected plan uses GPU-side matrix/bounds generation.
    pub uses_gpu_generation: bool,
    /// Total bytes allocated/planned for GPU-derived matrix and bounds buffers.
    pub total_derived_bytes: u64,
    /// Reason GPU generation or partial uploads were degraded, when applicable.
    pub degraded_reason: Option<String>,
}
