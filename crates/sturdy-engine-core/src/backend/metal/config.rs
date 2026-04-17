#[derive(Debug)]
pub struct MetalBackendConfig {
    pub api_validation: bool,
}

impl Default for MetalBackendConfig {
    fn default() -> Self {
        Self {
            api_validation: cfg!(debug_assertions),
        }
    }
}
