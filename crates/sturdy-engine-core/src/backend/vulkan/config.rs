#[derive(Debug)]
pub struct VulkanBackendConfig {
    pub validation: bool,
}

impl Default for VulkanBackendConfig {
    fn default() -> Self {
        Self {
            validation: cfg!(debug_assertions),
        }
    }
}
