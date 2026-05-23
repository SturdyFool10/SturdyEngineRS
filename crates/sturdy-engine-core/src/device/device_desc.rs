use crate::VulkanApiVersion;
use crate::adapter_selection::AdapterSelection;
use crate::backend::BackendKind;

#[derive(Clone, Debug)]
pub struct DeviceDesc {
    pub backend: BackendKind,
    pub validation: bool,
    pub adapter: AdapterSelection,
    pub required_features: Vec<String>,
    pub optional_features: Vec<String>,
    pub disabled_features: Vec<String>,
    pub required_extensions: Vec<String>,
    pub optional_extensions: Vec<String>,
    pub disabled_extensions: Vec<String>,
    /// Minimum Vulkan API version required. Device creation fails if the physical
    /// device does not support at least this version. Ignored for non-Vulkan backends.
    pub min_vulkan_version: VulkanApiVersion,
    /// Maximum Vulkan API version to request from the instance. Higher versions
    /// promote more extensions to core. Defaults to `VulkanApiVersion::LATEST`.
    /// The driver will deliver the highest version it supports up to this cap.
    pub max_vulkan_version: VulkanApiVersion,
}

/// Backend-agnostic device features that an app can require, prefer, or disable.
///
/// Required features make device creation fail when the selected backend or
/// adapter cannot provide them. Preferred features are enabled when available,
/// allowing the app to inspect [`Caps::features`] and choose a runtime fallback.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum DeviceFeature {
    RayTracing,
    RayQuery,
    MeshShading,
    BindlessResources,
    BufferDeviceAddress,
    VrsPipeline,
    VrsPrimitive,
    VrsAttachment,
    VariableRateShading,
    SamplerAnisotropy,
}

impl DeviceFeature {
    pub const fn backend_feature_names(self) -> &'static [&'static str] {
        match self {
            Self::RayTracing => &["ray_tracing"],
            Self::RayQuery => &["ray_query"],
            Self::MeshShading => &["mesh_shading"],
            Self::BindlessResources => &["bindless_resources"],
            Self::BufferDeviceAddress => &["buffer_device_address"],
            Self::VrsPipeline => &["pipeline_fragment_shading_rate"],
            Self::VrsPrimitive => &["primitive_fragment_shading_rate"],
            Self::VrsAttachment => &["attachment_fragment_shading_rate"],
            Self::VariableRateShading => &["variable_rate_shading"],
            Self::SamplerAnisotropy => &["sampler_anisotropy"],
        }
    }
}

impl Default for DeviceDesc {
    fn default() -> Self {
        Self {
            backend: BackendKind::Auto,
            validation: cfg!(debug_assertions),
            adapter: AdapterSelection::Auto,
            required_features: Vec::new(),
            optional_features: Vec::new(),
            disabled_features: Vec::new(),
            required_extensions: Vec::new(),
            optional_extensions: Vec::new(),
            disabled_extensions: Vec::new(),
            min_vulkan_version: VulkanApiVersion::V1_2,
            max_vulkan_version: VulkanApiVersion::LATEST,
        }
    }
}

impl DeviceDesc {
    /// Set the minimum Vulkan API version required. Device creation fails if
    /// the physical device does not support this version.
    pub fn require_vulkan_version(mut self, version: VulkanApiVersion) -> Self {
        self.min_vulkan_version = version;
        self
    }

    /// Set the maximum Vulkan API version to request. Higher versions unlock
    /// more promoted-to-core features. Defaults to `VulkanApiVersion::LATEST`.
    pub fn cap_vulkan_version(mut self, version: VulkanApiVersion) -> Self {
        self.max_vulkan_version = version;
        self
    }

    pub fn require_feature(mut self, feature: DeviceFeature) -> Self {
        push_device_feature(&mut self.required_features, feature);
        self
    }

    pub fn prefer_feature(mut self, feature: DeviceFeature) -> Self {
        push_device_feature(&mut self.optional_features, feature);
        self
    }

    pub fn disable_feature(mut self, feature: DeviceFeature) -> Self {
        push_device_feature(&mut self.disabled_features, feature);
        self
    }

    /// Require a backend-specific feature by name.
    ///
    /// Prefer [`DeviceDesc::require_feature`] for portable app-facing feature
    /// policy. This is an escape hatch for backend experiments and diagnostics.
    pub fn require_backend_feature(mut self, name: impl Into<String>) -> Self {
        self.required_features.push(name.into());
        self
    }

    /// Prefer a backend-specific feature by name when it is available.
    ///
    /// Prefer [`DeviceDesc::prefer_feature`] for portable app-facing feature
    /// policy. This is an escape hatch for backend experiments and diagnostics.
    pub fn prefer_backend_feature(mut self, name: impl Into<String>) -> Self {
        self.optional_features.push(name.into());
        self
    }

    /// Disable a backend-specific feature by name.
    ///
    /// Prefer [`DeviceDesc::disable_feature`] for portable app-facing feature
    /// policy. This is an escape hatch for backend experiments and diagnostics.
    pub fn disable_backend_feature(mut self, name: impl Into<String>) -> Self {
        self.disabled_features.push(name.into());
        self
    }

    pub fn require_backend_extension(mut self, name: impl Into<String>) -> Self {
        self.required_extensions.push(name.into());
        self
    }

    pub fn prefer_backend_extension(mut self, name: impl Into<String>) -> Self {
        self.optional_extensions.push(name.into());
        self
    }

    pub fn disable_backend_extension(mut self, name: impl Into<String>) -> Self {
        self.disabled_extensions.push(name.into());
        self
    }
}

fn push_device_feature(features: &mut Vec<String>, feature: DeviceFeature) {
    for name in feature.backend_feature_names() {
        if !features.iter().any(|existing| existing == name) {
            features.push((*name).to_string());
        }
    }
}
