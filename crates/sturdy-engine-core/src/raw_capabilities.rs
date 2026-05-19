use crate::{BackendKind, Caps};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VulkanRawCapabilities {
    pub extension_names: Vec<String>,
    pub feature_names: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct D3d12RawCapabilities {
    pub feature_names: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MetalRawCapabilities {
    pub feature_names: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendRawCapabilities {
    Vulkan(VulkanRawCapabilities),
    D3d12(D3d12RawCapabilities),
    Metal(MetalRawCapabilities),
    None,
}

impl BackendRawCapabilities {
    pub fn for_backend(backend: BackendKind, caps: &Caps) -> Self {
        match backend {
            BackendKind::Vulkan => Self::Vulkan(VulkanRawCapabilities {
                extension_names: caps.raw_extension_names.clone(),
                feature_names: caps.raw_feature_names.clone(),
            }),
            BackendKind::D3d12 => Self::D3d12(D3d12RawCapabilities {
                feature_names: caps.raw_feature_names.clone(),
            }),
            BackendKind::Metal => Self::Metal(MetalRawCapabilities {
                feature_names: caps.raw_feature_names.clone(),
            }),
            BackendKind::Auto | BackendKind::Null => Self::None,
        }
    }

    pub fn as_vulkan(&self) -> Option<&VulkanRawCapabilities> {
        match self {
            Self::Vulkan(capabilities) => Some(capabilities),
            _ => None,
        }
    }

    pub fn as_d3d12(&self) -> Option<&D3d12RawCapabilities> {
        match self {
            Self::D3d12(capabilities) => Some(capabilities),
            _ => None,
        }
    }

    pub fn as_metal(&self) -> Option<&MetalRawCapabilities> {
        match self {
            Self::Metal(capabilities) => Some(capabilities),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "raw_capabilities_tests.rs"]
mod tests;
