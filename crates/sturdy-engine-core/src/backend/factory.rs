#[cfg(not(target_arch = "wasm32"))]
use crate::backend::vulkan::{VulkanBackend, VulkanBackendConfig};
use crate::backend::{auto_backend_preference_order, Backend, BackendKind, NullBackend};
use crate::{AdapterInfo, DeviceDesc, Error, Result};

/// Create the concrete backend requested by a device descriptor.
pub(crate) fn create_backend(desc: &DeviceDesc) -> Result<Box<dyn Backend>> {
    match desc.backend {
        BackendKind::Auto => {
            let preferred = auto_backend_preference_order();
            let mut last_error = None;
            for kind in preferred {
                let sub = DeviceDesc {
                    backend: kind,
                    ..desc.clone()
                };
                match create_backend(&sub) {
                    Ok(backend) => return Ok(backend),
                    Err(error) => last_error = Some(error),
                }
            }
            Err(last_error.unwrap_or(Error::Unsupported("no backend is available on this target")))
        }
        BackendKind::Null => Ok(Box::new(NullBackend::new())),
        BackendKind::Vulkan => create_vulkan_backend(desc),
        BackendKind::D3d12 => create_available_backend(BackendKind::D3d12, "D3D12"),
        BackendKind::Metal => create_available_backend(BackendKind::Metal, "Metal"),
    }
}

/// Enumerate all physical adapters for a backend without creating a device.
///
/// Returns an empty list for backends that are not available on this target.
pub fn enumerate_adapters(backend: BackendKind) -> Result<Vec<AdapterInfo>> {
    match backend {
        BackendKind::Vulkan => enumerate_vulkan_adapters(),
        BackendKind::Auto => {
            for kind in auto_backend_preference_order() {
                let adapters = enumerate_adapters(kind)?;
                if !adapters.is_empty() {
                    return Ok(adapters);
                }
            }
            Ok(Vec::new())
        }
        _ => Ok(Vec::new()),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn create_vulkan_backend(desc: &DeviceDesc) -> Result<Box<dyn Backend>> {
    if !BackendKind::Vulkan.is_available_on_target() {
        return Err(Error::Unsupported("Vulkan is not available on this target"));
    }
    Ok(Box::new(VulkanBackend::create(VulkanBackendConfig::new(
        desc.validation,
        desc.adapter.clone(),
        desc.required_features.clone(),
        desc.optional_features.clone(),
        desc.disabled_features.clone(),
        desc.required_extensions.clone(),
        desc.optional_extensions.clone(),
        desc.disabled_extensions.clone(),
    ))?))
}

#[cfg(target_arch = "wasm32")]
fn create_vulkan_backend(_desc: &DeviceDesc) -> Result<Box<dyn Backend>> {
    Err(Error::Unsupported("Vulkan is not available on this target"))
}

#[cfg(not(target_arch = "wasm32"))]
fn enumerate_vulkan_adapters() -> Result<Vec<AdapterInfo>> {
    VulkanBackend::enumerate_adapters(&VulkanBackendConfig::default())
}

#[cfg(target_arch = "wasm32")]
fn enumerate_vulkan_adapters() -> Result<Vec<AdapterInfo>> {
    Ok(Vec::new())
}

fn create_available_backend(kind: BackendKind, name: &'static str) -> Result<Box<dyn Backend>> {
    if !kind.is_available_on_target() {
        return Err(Error::Unsupported(match kind {
            BackendKind::Vulkan => "Vulkan is not available on this target",
            BackendKind::D3d12 => "D3D12 is not available on this target",
            BackendKind::Metal => "Metal is not available on this target",
            BackendKind::Auto | BackendKind::Null => "backend is not available on this target",
        }));
    }

    let _name = name;
    Ok(Box::new(NullBackend::for_kind(kind)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_backend_factory_creates_null_backend() {
        let backend = create_backend(&DeviceDesc {
            backend: BackendKind::Null,
            ..DeviceDesc::default()
        })
        .unwrap();

        assert_eq!(backend.kind(), BackendKind::Null);
    }

    #[test]
    fn null_adapter_enumeration_is_empty() {
        assert!(enumerate_adapters(BackendKind::Null).unwrap().is_empty());
    }
}
