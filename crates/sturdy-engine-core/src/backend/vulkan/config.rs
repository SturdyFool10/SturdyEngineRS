use crate::AdapterSelection;

#[derive(Debug, Default)]
pub struct VulkanBackendConfig {
    pub validation: bool,
    pub adapter_selection: AdapterSelection,
    pub required_features: Vec<String>,
    pub optional_features: Vec<String>,
    pub disabled_features: Vec<String>,
    pub required_extensions: Vec<String>,
    pub optional_extensions: Vec<String>,
    pub disabled_extensions: Vec<String>,
}

impl VulkanBackendConfig {
    pub fn new(
        validation: bool,
        adapter_selection: AdapterSelection,
        required_features: Vec<String>,
        optional_features: Vec<String>,
        disabled_features: Vec<String>,
        required_extensions: Vec<String>,
        optional_extensions: Vec<String>,
        disabled_extensions: Vec<String>,
    ) -> Self {
        Self {
            validation,
            adapter_selection,
            required_features,
            optional_features,
            disabled_features,
            required_extensions,
            optional_extensions,
            disabled_extensions,
        }
    }
}
