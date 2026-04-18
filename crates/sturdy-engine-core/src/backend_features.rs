#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct BackendFeatures {
    pub ray_tracing: bool,
    pub mesh_shading: bool,
    pub bindless: bool,
    pub descriptor_indexing: bool,
    pub timeline_semaphores: bool,
    pub dynamic_rendering: bool,
    pub synchronization2: bool,
    pub hdr_output: bool,
    pub shader_fp16: bool,
    pub shader_fp64: bool,
    pub image_fp16_render: bool,
    pub image_fp32_render: bool,
    pub variable_rate_shading: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_features_are_conservative() {
        let features = BackendFeatures::default();

        assert!(!features.mesh_shading);
        assert!(!features.ray_tracing);
        assert!(!features.bindless);
        assert!(!features.hdr_output);
        assert!(!features.shader_fp16);
        assert!(!features.shader_fp64);
        assert!(!features.image_fp16_render);
        assert!(!features.image_fp32_render);
        assert!(!features.dynamic_rendering);
        assert!(!features.timeline_semaphores);
    }
}
