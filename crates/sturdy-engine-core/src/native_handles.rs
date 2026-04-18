use crate::BackendKind;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum NativeHandleKind {
    VulkanInstance,
    VulkanPhysicalDevice,
    VulkanDevice,
    VulkanQueue,
    VulkanImage,
    VulkanImageView,
    VulkanBuffer,
    VulkanSampler,
    VulkanShaderModule,
    VulkanPipelineLayout,
    VulkanPipeline,
    VulkanSurface,
    VulkanSwapchain,
    D3d12Adapter,
    D3d12Device,
    D3d12CommandQueue,
    D3d12Resource,
    D3d12PipelineState,
    MetalDevice,
    MetalCommandQueue,
    MetalResource,
    MetalPipelineState,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum NativeHandleOwnership {
    /// The engine keeps ownership. The handle is valid only while the matching
    /// engine object remains alive.
    Borrowed,
    /// Ownership/lifetime is shared with an externally created object that was
    /// imported into the engine.
    Imported,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct NativeHandleCapability {
    pub kind: NativeHandleKind,
    pub exportable: bool,
    pub importable: bool,
    pub ownership: NativeHandleOwnership,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeHandleCapabilities {
    pub backend: BackendKind,
    pub handles: Vec<NativeHandleCapability>,
}

impl NativeHandleCapabilities {
    pub fn supports_export(&self, kind: NativeHandleKind) -> bool {
        self.handles
            .iter()
            .any(|capability| capability.kind == kind && capability.exportable)
    }

    pub fn supports_import(&self, kind: NativeHandleKind) -> bool {
        self.handles
            .iter()
            .any(|capability| capability.kind == kind && capability.importable)
    }
}

pub fn native_handle_capabilities_for_backend(backend: BackendKind) -> NativeHandleCapabilities {
    let handles = match backend {
        BackendKind::Vulkan => borrowed_exports(&[
            NativeHandleKind::VulkanInstance,
            NativeHandleKind::VulkanPhysicalDevice,
            NativeHandleKind::VulkanDevice,
            NativeHandleKind::VulkanQueue,
            NativeHandleKind::VulkanImage,
            NativeHandleKind::VulkanImageView,
            NativeHandleKind::VulkanBuffer,
            NativeHandleKind::VulkanSampler,
            NativeHandleKind::VulkanShaderModule,
            NativeHandleKind::VulkanPipelineLayout,
            NativeHandleKind::VulkanPipeline,
            NativeHandleKind::VulkanSurface,
            NativeHandleKind::VulkanSwapchain,
        ]),
        BackendKind::D3d12 => borrowed_exports(&[
            NativeHandleKind::D3d12Adapter,
            NativeHandleKind::D3d12Device,
            NativeHandleKind::D3d12CommandQueue,
            NativeHandleKind::D3d12Resource,
            NativeHandleKind::D3d12PipelineState,
        ]),
        BackendKind::Metal => borrowed_exports(&[
            NativeHandleKind::MetalDevice,
            NativeHandleKind::MetalCommandQueue,
            NativeHandleKind::MetalResource,
            NativeHandleKind::MetalPipelineState,
        ]),
        BackendKind::Auto | BackendKind::Null => Vec::new(),
    };

    NativeHandleCapabilities { backend, handles }
}

fn borrowed_exports(kinds: &[NativeHandleKind]) -> Vec<NativeHandleCapability> {
    kinds
        .iter()
        .copied()
        .map(|kind| NativeHandleCapability {
            kind,
            exportable: true,
            importable: false,
            ownership: NativeHandleOwnership::Borrowed,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vulkan_native_handle_policy_exposes_borrowed_exports_only() {
        let capabilities = native_handle_capabilities_for_backend(BackendKind::Vulkan);

        assert!(capabilities.supports_export(NativeHandleKind::VulkanDevice));
        assert!(capabilities.supports_export(NativeHandleKind::VulkanImage));
        assert!(!capabilities.supports_import(NativeHandleKind::VulkanImage));
        assert!(capabilities
            .handles
            .iter()
            .all(|capability| capability.ownership == NativeHandleOwnership::Borrowed));
    }

    #[test]
    fn null_backend_has_no_native_handles() {
        let capabilities = native_handle_capabilities_for_backend(BackendKind::Null);

        assert_eq!(capabilities.backend, BackendKind::Null);
        assert!(capabilities.handles.is_empty());
        assert!(!capabilities.supports_export(NativeHandleKind::VulkanDevice));
    }
}
