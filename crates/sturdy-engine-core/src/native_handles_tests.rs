// Tests extracted from crates/sturdy-engine-core/src/native_handles.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn vulkan_native_handle_policy_exposes_borrowed_exports_and_resource_imports() {
    let capabilities = native_handle_capabilities_for_backend(BackendKind::Vulkan);

    assert!(capabilities.supports_export(NativeHandleKind::VulkanDevice));
    assert!(capabilities.supports_export(NativeHandleKind::VulkanImage));
    assert!(capabilities.supports_import(NativeHandleKind::VulkanImage));
    assert!(capabilities.supports_import(NativeHandleKind::VulkanBuffer));
    assert!(!capabilities.supports_import(NativeHandleKind::VulkanDevice));
    assert!(
        capabilities
            .handles
            .iter()
            .all(|capability| capability.export_ownership == Some(NativeHandleOwnership::Borrowed))
    );
}

#[test]
fn null_backend_has_no_native_handles() {
    let capabilities = native_handle_capabilities_for_backend(BackendKind::Null);

    assert_eq!(capabilities.backend, BackendKind::Null);
    assert!(capabilities.handles.is_empty());
    assert!(!capabilities.supports_export(NativeHandleKind::VulkanDevice));
}
