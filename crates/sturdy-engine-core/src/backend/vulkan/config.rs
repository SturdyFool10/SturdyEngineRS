use crate::{AdapterSelection, VulkanApiVersion};

#[derive(Debug)]
pub struct VulkanBackendConfig {
    pub validation: bool,
    pub adapter_selection: AdapterSelection,
    pub required_features: Vec<String>,
    pub optional_features: Vec<String>,
    pub disabled_features: Vec<String>,
    pub required_extensions: Vec<String>,
    pub optional_extensions: Vec<String>,
    pub disabled_extensions: Vec<String>,
    /// Minimum API version the selected physical device must support.
    pub min_api_version: VulkanApiVersion,
    /// API version to request in VkApplicationInfo. The driver will deliver
    /// the highest version it supports up to this cap.
    pub max_api_version: VulkanApiVersion,
}

impl Default for VulkanBackendConfig {
    fn default() -> Self {
        Self {
            validation: false,
            adapter_selection: AdapterSelection::default(),
            required_features: Vec::new(),
            optional_features: Vec::new(),
            disabled_features: Vec::new(),
            required_extensions: Vec::new(),
            optional_extensions: Vec::new(),
            disabled_extensions: Vec::new(),
            min_api_version: VulkanApiVersion::V1_2,
            max_api_version: VulkanApiVersion::LATEST,
        }
    }
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
        min_api_version: VulkanApiVersion,
        max_api_version: VulkanApiVersion,
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
            min_api_version,
            max_api_version,
        }
    }
}
