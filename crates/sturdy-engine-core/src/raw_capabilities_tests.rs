// Tests extracted from crates/sturdy-engine-core/src/raw_capabilities.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn vulkan_raw_capabilities_preserve_feature_and_extension_names() {
    let caps = Caps {
        raw_extension_names: vec!["VK_KHR_swapchain".to_string()],
        raw_feature_names: vec!["timelineSemaphore".to_string()],
        ..Caps::default()
    };

    let raw = BackendRawCapabilities::for_backend(BackendKind::Vulkan, &caps);
    let vulkan = raw.as_vulkan().expect("vulkan raw capabilities");

    assert_eq!(vulkan.extension_names, vec!["VK_KHR_swapchain"]);
    assert_eq!(vulkan.feature_names, vec!["timelineSemaphore"]);
    assert!(raw.as_d3d12().is_none());
    assert!(raw.as_metal().is_none());
}

#[test]
fn null_backend_has_no_raw_capabilities() {
    let raw = BackendRawCapabilities::for_backend(BackendKind::Null, &Caps::default());

    assert_eq!(raw, BackendRawCapabilities::None);
}
