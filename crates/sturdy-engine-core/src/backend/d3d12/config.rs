#[derive(Debug)]
pub struct D3d12BackendConfig {
    pub debug_layer: bool,
    pub gpu_validation: bool,
}

impl Default for D3d12BackendConfig {
    fn default() -> Self {
        Self {
            debug_layer: cfg!(debug_assertions),
            gpu_validation: false,
        }
    }
}
