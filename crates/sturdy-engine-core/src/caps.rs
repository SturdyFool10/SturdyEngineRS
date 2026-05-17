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
