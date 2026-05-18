use crate::{BackendFeatures, Limits};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Caps {
    pub supports_raytracing: bool,
    pub supports_mesh_shading: bool,
    pub supports_bindless: bool,
    pub max_color_sample_count: u8,
    pub max_mip_levels: u32,
    pub max_frames_in_flight: u32,
    pub features: BackendFeatures,
    pub limits: Limits,
    pub raw_extension_names: Vec<String>,
    pub raw_feature_names: Vec<String>,
    /// Number of shader cores/CUs/SMs reported by the GPU (AMD or NVIDIA).
    ///
    /// AMD: total compute units (`shader_engine_count × arrays_per_engine × CUs_per_array`).
    /// NVIDIA: SM count from `VK_NV_shader_sm_builtins`.
    /// `None` when neither extension is available.
    pub shader_core_count: Option<u32>,
}

impl Default for Caps {
    fn default() -> Self {
        Self {
            supports_raytracing: false,
            supports_mesh_shading: false,
            supports_bindless: false,
            max_color_sample_count: 1,
            max_mip_levels: 1,
            max_frames_in_flight: 2,
            features: Default::default(),
            limits: Default::default(),
            raw_extension_names: Vec::new(),
            raw_feature_names: Vec::new(),
            shader_core_count: None,
        }
    }
}

/// Scope of a cooperative matrix operation.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CoopMatrixScope {
    Device,
    Workgroup,
    Subgroup,
    QueueFamily,
}

/// Element type for a cooperative matrix A/B/C/result matrix.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CoopMatrixElementType {
    Float16,
    Float32,
    Float64,
    Sint8,
    Sint16,
    Sint32,
    Uint8,
    Uint16,
    Uint32,
}

/// One valid cooperative-matrix multiply-accumulate configuration.
///
/// Returned by `Engine::enumerate_cooperative_matrix_properties()`. Use to select
/// workgroup sizes and loop dimensions that map to native hardware tiles.
#[derive(Clone, Debug)]
pub struct CoopMatrixProperty {
    pub scope: CoopMatrixScope,
    pub a_type: CoopMatrixElementType,
    pub b_type: CoopMatrixElementType,
    pub c_type: CoopMatrixElementType,
    pub result_type: CoopMatrixElementType,
    pub m_size: u32,
    pub n_size: u32,
    pub k_size: u32,
    pub saturating_accumulation: bool,
}

/// Per-executable (shader stage) statistics from `VK_KHR_pipeline_executable_properties`.
///
/// Returned by `Engine::pipeline_executable_stats(pipeline)`. Use to inspect
/// compiled register counts, code size, and IR for performance debugging.
#[derive(Clone, Debug)]
pub struct ExecutableStat {
    /// Human-readable name of the executable (e.g., "vertex", "fragment").
    pub name: String,
    /// Description of what this statistic measures.
    pub description: String,
    /// 64-bit integer value (used for counts, sizes, register numbers).
    pub value_int: Option<i64>,
    /// 64-bit float value (used for ratios, averages).
    pub value_float: Option<f64>,
    /// Boolean value (used for feature flags).
    pub value_bool: Option<bool>,
}

/// Per-stage shader statistics returned by `Engine::pipeline_shader_stats_amd`.
///
/// Requires `BackendFeatures::shader_info_amd`. Each entry corresponds to one active
/// shader stage in the pipeline.
#[derive(Clone, Debug, Default)]
pub struct AmdShaderStageStats {
    /// Shader stages covered by this entry (e.g. `VERTEX | FRAGMENT`).
    pub stage_mask: u32,
    /// VGPRs (vector registers) actually used by the compiled shader.
    pub num_used_vgprs: u32,
    /// SGPRs (scalar registers) actually used by the compiled shader.
    pub num_used_sgprs: u32,
    /// Physical VGPRs available on the hardware for this shader.
    pub num_physical_vgprs: u32,
    /// Physical SGPRs available on the hardware for this shader.
    pub num_physical_sgprs: u32,
    /// LDS (local data store) bytes per workgroup (compute only; 0 for graphics).
    pub lds_size_per_workgroup: u32,
    /// LDS bytes actually consumed by this shader.
    pub lds_usage_bytes: usize,
    /// Scratch memory bytes used (spilled registers).
    pub scratch_mem_bytes: usize,
    /// Compute workgroup size as `[x, y, z]` (zero for graphics stages).
    pub compute_workgroup_size: [u32; 3],
}

/// Category of a GPU hardware performance counter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PerfCounterCategory {
    CommandBuffer,
    Compute,
    Cache,
    Memory,
    Generic,
}

/// One hardware performance counter exposed by `Engine::enumerate_performance_counters()`.
///
/// Use `PerfCounterHandle` (the counter's index) to request recording via
/// `PassDesc::perf_counters`.
#[derive(Clone, Debug)]
pub struct PerfCounter {
    /// Stable index within the device's counter list. Passes as `PerfCounterHandle`.
    pub index: u32,
    /// Human-readable name (e.g. `"GPU Active Cycles"`).
    pub name: String,
    /// Description of what the counter measures.
    pub description: String,
    /// Logical category for grouping in profiling UIs.
    pub category: PerfCounterCategory,
}
