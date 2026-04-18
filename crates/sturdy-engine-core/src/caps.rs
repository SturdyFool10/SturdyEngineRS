use crate::{BackendFeatures, Limits};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Caps {
    pub supports_raytracing: bool,
    pub supports_mesh_shading: bool,
    pub supports_bindless: bool,
    pub max_mip_levels: u32,
    pub max_frames_in_flight: u32,
    pub features: BackendFeatures,
    pub limits: Limits,
    pub raw_extension_names: Vec<String>,
    pub raw_feature_names: Vec<String>,
}
