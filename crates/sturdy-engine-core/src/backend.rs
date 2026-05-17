#[cfg(not(target_arch = "wasm32"))]
use crate::NativeSurfaceDesc;
use crate::native_handle_capabilities_for_backend;
use crate::shader_object::{ShaderObjectDesc, ShaderObjectHandle};
use crate::{
    AccelerationStructureBuildSizes, AccelerationStructureDesc, AccelerationStructureHandle,
    AntiLagMode, BackendRawCapabilities, BindGroupDesc, BindGroupHandle, BlasBuildDesc, BufferDesc,
    BufferHandle, CanonicalPipelineLayout, Caps, CompiledGraph, ComputePipelineDesc,
    ExternalBufferDesc, ExternalImageDesc, GpuCaptureDesc, GpuCaptureTool, GraphicsPipelineDesc,
    HdrMetadata, ImageDesc, ImageHandle, LatencyMode, NativeHandleCapabilities, PipelineHandle,
    PipelineLayoutHandle, RayTracingPipelineDesc, ReflexMode, Result, SamplerDesc, SamplerHandle,
    ShaderBindingTableProperties, ShaderDesc, ShaderHandle, ShaderTarget, SubmissionHandle,
    SurfaceCapabilities, SurfaceHandle, SurfaceInfo, SurfaceRecreateDesc, SurfaceSize,
    TlasBuildDesc, VideoSessionDesc, VideoSessionHandle,
};
use crate::{Format, FormatCapabilities, GpuMemoryBudget};

#[cfg(target_os = "windows")]
pub mod d3d12;
pub mod factory;
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "visionos"
))]
pub mod metal;
#[cfg(not(target_arch = "wasm32"))]
pub mod vulkan;

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum BackendKind {
    #[default]
    Auto,
    Vulkan,
    D3d12,
    Metal,
    Null,
}

impl BackendKind {
    pub fn is_available_on_target(self) -> bool {
        match self {
            Self::Auto | Self::Null => true,
            Self::Vulkan => cfg!(not(target_arch = "wasm32")),
            Self::D3d12 => cfg!(target_os = "windows"),
            Self::Metal => cfg!(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "tvos",
                target_os = "visionos"
            )),
        }
    }
}

pub fn available_backend_kinds() -> Vec<BackendKind> {
    let mut backends = Vec::new();
    if BackendKind::Vulkan.is_available_on_target() {
        backends.push(BackendKind::Vulkan);
    }
    if BackendKind::D3d12.is_available_on_target() {
        backends.push(BackendKind::D3d12);
    }
    if BackendKind::Metal.is_available_on_target() {
        backends.push(BackendKind::Metal);
    }
    backends.push(BackendKind::Null);
    backends
}

pub fn auto_backend_preference_order() -> Vec<BackendKind> {
    let candidates = if cfg!(target_os = "windows") {
        vec![BackendKind::D3d12, BackendKind::Vulkan]
    } else if cfg!(target_os = "linux") {
        vec![BackendKind::Vulkan]
    } else if cfg!(target_os = "macos") {
        vec![BackendKind::Vulkan, BackendKind::Metal]
    } else if cfg!(any(
        target_os = "ios",
        target_os = "tvos",
        target_os = "visionos"
    )) {
        vec![BackendKind::Metal]
    } else {
        vec![BackendKind::Vulkan]
    };

    candidates
        .into_iter()
        .filter(|backend| backend.is_available_on_target())
        .collect()
}

pub trait Backend: Send + Sync {
    fn kind(&self) -> BackendKind;
    fn adapter_name(&self) -> Option<String> {
        None
    }
    fn caps(&self) -> Caps;
    fn format_capabilities(&self, _format: Format) -> FormatCapabilities {
        FormatCapabilities::default()
    }
    /// Current GPU memory usage and capacity. Returns `None` on backends that
    /// don't expose allocator statistics.
    fn memory_budget(&self) -> Option<GpuMemoryBudget> {
        None
    }

    // ── Bindless descriptor heap (Track 8a) ───────────────────────────────────

    /// Register a sampled (read-only) image in the bindless heap.
    ///
    /// Returns the stable `u32` index to embed in push constants or a per-draw
    /// data buffer. Returns `None` when bindless is not supported or capacity
    /// is exhausted. Never returns the same index twice.
    fn register_bindless_sampled_image(&self, _handle: ImageHandle) -> Option<u32> {
        None
    }

    /// Register a sampler in the bindless heap.
    fn register_bindless_sampler(&self, _handle: SamplerHandle) -> Option<u32> {
        None
    }

    /// Register a storage (read-write) image in the bindless heap.
    fn register_bindless_storage_image(&self, _handle: ImageHandle) -> Option<u32> {
        None
    }

    /// Register a storage buffer in the bindless heap.
    fn register_bindless_storage_buffer(&self, _handle: BufferHandle) -> Option<u32> {
        None
    }

    /// Returns `true` when the bindless heap is available on this backend.
    fn bindless_supported(&self) -> bool {
        false
    }

    /// Returns the required alignment for descriptor buffer offsets when
    /// `VK_EXT_descriptor_buffer` is available, or `None` otherwise.
    fn descriptor_buffer_offset_alignment(&self) -> Option<u64> {
        None
    }

    fn create_shader_object(
        &self,
        _handle: ShaderObjectHandle,
        _desc: &ShaderObjectDesc,
    ) -> Result<()> {
        Err(crate::Error::Unsupported(
            "shader objects are not supported by this backend",
        ))
    }
    fn destroy_shader_object(&self, _handle: ShaderObjectHandle) -> Result<()> {
        Ok(())
    }

    fn native_handle_capabilities(&self) -> NativeHandleCapabilities {
        native_handle_capabilities_for_backend(self.kind())
    }
    fn raw_capabilities(&self) -> BackendRawCapabilities {
        BackendRawCapabilities::for_backend(self.kind(), &self.caps())
    }
    /// The shader IR format this backend requires. The device uses this to select
    /// the Slang compilation target when compiling from source.
    fn preferred_shader_ir(&self) -> ShaderTarget {
        ShaderTarget::Spirv
    }
    fn create_image(&self, _handle: ImageHandle, _desc: ImageDesc) -> Result<()> {
        Ok(())
    }
    unsafe fn import_external_image(
        &self,
        _handle: ImageHandle,
        _desc: ExternalImageDesc,
    ) -> Result<()> {
        Err(crate::Error::Unsupported(
            "backend does not support external image import",
        ))
    }
    /// Create a transient image that may be aliased with other transient images.
    ///
    /// Backends that support aliasing (Vulkan) defer memory binding to flush time.
    /// Backends that do not support aliasing fall back to `create_image`.
    fn create_transient_image(&self, handle: ImageHandle, desc: ImageDesc) -> Result<()> {
        self.create_image(handle, desc)
    }
    fn destroy_image(&self, _handle: ImageHandle) -> Result<()> {
        Ok(())
    }
    fn create_buffer(&self, _handle: BufferHandle, _desc: BufferDesc) -> Result<()> {
        Ok(())
    }
    fn buffer_device_address(&self, _handle: BufferHandle) -> Result<Option<u64>> {
        Ok(None)
    }
    unsafe fn import_external_buffer(
        &self,
        _handle: BufferHandle,
        _desc: ExternalBufferDesc,
    ) -> Result<()> {
        Err(crate::Error::Unsupported(
            "backend does not support external buffer import",
        ))
    }
    fn destroy_buffer(&self, _handle: BufferHandle) -> Result<()> {
        Ok(())
    }
    fn create_acceleration_structure(
        &self,
        _handle: AccelerationStructureHandle,
        _desc: AccelerationStructureDesc,
    ) -> Result<()> {
        Err(crate::Error::Unsupported(
            "backend does not support acceleration structures",
        ))
    }
    fn destroy_acceleration_structure(&self, _handle: AccelerationStructureHandle) -> Result<()> {
        Ok(())
    }
    fn blas_build_sizes(&self, _desc: &BlasBuildDesc) -> Result<AccelerationStructureBuildSizes> {
        Err(crate::Error::Unsupported(
            "backend does not support BLAS build size queries",
        ))
    }
    fn tlas_build_sizes(&self, _desc: &TlasBuildDesc) -> Result<AccelerationStructureBuildSizes> {
        Err(crate::Error::Unsupported(
            "backend does not support TLAS build size queries",
        ))
    }
    fn create_sampler(&self, _handle: SamplerHandle, _desc: SamplerDesc) -> Result<()> {
        Ok(())
    }
    fn destroy_sampler(&self, _handle: SamplerHandle) -> Result<()> {
        Ok(())
    }
    fn write_buffer(&self, _handle: BufferHandle, _offset: u64, _data: &[u8]) -> Result<()> {
        Ok(())
    }
    fn read_buffer(&self, _handle: BufferHandle, _offset: u64, out: &mut [u8]) -> Result<()> {
        out.fill(0);
        Ok(())
    }
    fn create_shader(&self, _handle: ShaderHandle, _desc: &ShaderDesc) -> Result<()> {
        Ok(())
    }
    fn destroy_shader(&self, _handle: ShaderHandle) -> Result<()> {
        Ok(())
    }
    fn create_pipeline_layout(
        &self,
        _handle: PipelineLayoutHandle,
        _layout: &CanonicalPipelineLayout,
    ) -> Result<()> {
        Ok(())
    }
    fn destroy_pipeline_layout(&self, _handle: PipelineLayoutHandle) -> Result<()> {
        Ok(())
    }
    fn create_bind_group(&self, _handle: BindGroupHandle, _desc: &BindGroupDesc) -> Result<()> {
        Ok(())
    }
    fn destroy_bind_group(&self, _handle: BindGroupHandle) -> Result<()> {
        Ok(())
    }
    fn create_compute_pipeline(
        &self,
        _handle: PipelineHandle,
        _desc: ComputePipelineDesc,
    ) -> Result<()> {
        Ok(())
    }
    fn create_graphics_pipeline(
        &self,
        _handle: PipelineHandle,
        _desc: &GraphicsPipelineDesc,
    ) -> Result<()> {
        Ok(())
    }
    fn create_ray_tracing_pipeline(
        &self,
        _handle: PipelineHandle,
        _desc: &RayTracingPipelineDesc,
    ) -> Result<()> {
        Err(crate::Error::Unsupported(
            "ray tracing pipelines are not supported by this backend",
        ))
    }
    fn shader_binding_table_properties(&self) -> Result<ShaderBindingTableProperties> {
        Err(crate::Error::Unsupported(
            "shader binding tables are not supported by this backend",
        ))
    }
    fn ray_tracing_shader_group_handles(
        &self,
        _pipeline: PipelineHandle,
        _first_group: u32,
        _group_count: u32,
    ) -> Result<Vec<u8>> {
        Err(crate::Error::Unsupported(
            "ray tracing shader group handles are not supported by this backend",
        ))
    }
    fn destroy_pipeline(&self, _handle: PipelineHandle) -> Result<()> {
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn create_surface(
        &self,
        _handle: SurfaceHandle,
        desc: NativeSurfaceDesc,
    ) -> Result<SurfaceInfo> {
        Ok(SurfaceInfo {
            size: desc.size,
            format: crate::Format::Unknown,
            color_space: crate::SurfaceColorSpace::Unknown,
        })
    }
    fn resize_surface(&self, _handle: SurfaceHandle, size: SurfaceSize) -> Result<SurfaceInfo> {
        Ok(SurfaceInfo {
            size,
            format: crate::Format::Unknown,
            color_space: crate::SurfaceColorSpace::Unknown,
        })
    }
    fn recreate_surface(
        &self,
        _handle: SurfaceHandle,
        desc: SurfaceRecreateDesc,
        current: SurfaceInfo,
    ) -> Result<SurfaceInfo> {
        Ok(SurfaceInfo {
            size: desc.size.unwrap_or(current.size),
            ..current
        })
    }
    fn acquire_surface_image(
        &self,
        _surface: SurfaceHandle,
        _image: ImageHandle,
    ) -> Result<(ImageDesc, u64)> {
        Err(crate::Error::Unsupported(
            "backend does not support surface image acquisition",
        ))
    }
    fn present_surface(&self, _surface: SurfaceHandle) -> Result<()> {
        Err(crate::Error::Unsupported(
            "backend does not support surface presentation",
        ))
    }
    fn destroy_surface(&self, _handle: SurfaceHandle) -> Result<()> {
        Ok(())
    }
    fn query_surface_capabilities(&self, _handle: SurfaceHandle) -> Result<SurfaceCapabilities> {
        Ok(SurfaceCapabilities::default())
    }
    /// Assign a debug name to an image resource. No-op when debug utils are unavailable.
    fn set_image_debug_name(&self, _handle: ImageHandle, _name: &str) {}
    /// Assign a debug name to a buffer resource. No-op when debug utils are unavailable.
    fn set_buffer_debug_name(&self, _handle: BufferHandle, _name: &str) {}
    /// Assign a debug name to a pipeline. No-op when debug utils are unavailable.
    fn set_pipeline_debug_name(&self, _handle: PipelineHandle, _name: &str) {}
    fn supported_gpu_capture_tools(&self) -> Vec<GpuCaptureTool> {
        Vec::new()
    }
    fn begin_gpu_capture(&self, _desc: &GpuCaptureDesc) -> Result<()> {
        Err(crate::Error::Unsupported(
            "backend does not support GPU capture",
        ))
    }
    fn end_gpu_capture(&self, _tool: GpuCaptureTool) -> Result<()> {
        Err(crate::Error::Unsupported(
            "backend does not support GPU capture",
        ))
    }
    // ── GFX-4: Video encode/decode ────────────────────────────────────────────

    fn create_video_session(
        &self,
        _handle: VideoSessionHandle,
        _desc: VideoSessionDesc,
    ) -> Result<()> {
        Err(crate::Error::Unsupported(
            "video sessions are not supported by this backend",
        ))
    }

    fn destroy_video_session(&self, _handle: VideoSessionHandle) -> Result<()> {
        Ok(())
    }

    // ── GFX-6b: Latency reduction ─────────────────────────────────────────────

    fn set_reflex_mode(&self, _mode: ReflexMode) -> Result<()> {
        Err(crate::Error::Unsupported(
            "NVIDIA Reflex latency mode is not supported by this backend",
        ))
    }

    fn set_anti_lag_mode(&self, _mode: AntiLagMode) -> Result<()> {
        Err(crate::Error::Unsupported(
            "AMD Anti-Lag latency mode is not supported by this backend",
        ))
    }

    fn latency_mode(&self) -> Option<LatencyMode> {
        None
    }

    fn set_surface_hdr_metadata(
        &self,
        _surface: SurfaceHandle,
        _metadata: HdrMetadata,
    ) -> Result<()> {
        Ok(())
    }

    // ── GFX-5a: External memory exports (Linux) ───────────────────────────────

    /// Export a file descriptor for a buffer's underlying memory.
    ///
    /// Requires the buffer to have been created with external memory flags and
    /// `VK_KHR_external_memory_fd` to be available. Returns the fd on success.
    /// The caller is responsible for closing the fd.
    fn export_buffer_fd(&self, _handle: BufferHandle) -> Result<i32> {
        Err(crate::Error::Unsupported(
            "buffer fd export requires VK_KHR_external_memory_fd and a buffer created with external memory flags",
        ))
    }

    /// Export a file descriptor for an image's underlying memory.
    ///
    /// Same requirements as `export_buffer_fd`.
    fn export_image_fd(&self, _handle: ImageHandle) -> Result<i32> {
        Err(crate::Error::Unsupported(
            "image fd export requires VK_KHR_external_memory_fd and an image created with external memory flags",
        ))
    }

    fn flush(&self, _graph: &CompiledGraph) -> Result<SubmissionHandle>;
    fn wait_submission(&self, _token: SubmissionHandle) -> Result<()> {
        Ok(())
    }
    /// Per-pass GPU timings from the most recently completed frame.
    ///
    /// Returns `(pass_name, gpu_milliseconds)` pairs in submission order.
    /// Empty on backends that don't support timestamp queries, or before the
    /// second frame (timestamps are read back one frame in arrears).
    fn pass_timings(&self) -> Vec<(String, f32)> {
        Vec::new()
    }
    fn present(&self) -> Result<()>;
    fn wait_idle(&self) -> Result<()>;
}

#[derive(Debug)]
pub(crate) struct NullBackend {
    kind: BackendKind,
    caps: Caps,
}

impl NullBackend {
    pub(crate) fn new() -> Self {
        Self::for_kind(BackendKind::Null)
    }

    pub(crate) fn for_kind(kind: BackendKind) -> Self {
        Self {
            kind,
            caps: Caps::default(),
        }
    }
}

impl Backend for NullBackend {
    fn kind(&self) -> BackendKind {
        self.kind
    }

    fn caps(&self) -> Caps {
        self.caps.clone()
    }

    fn flush(&self, _graph: &CompiledGraph) -> Result<SubmissionHandle> {
        Ok(SubmissionHandle(0))
    }

    fn present(&self) -> Result<()> {
        Ok(())
    }

    fn wait_idle(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_preference_order_matches_target_policy() {
        let order = auto_backend_preference_order();

        if cfg!(target_os = "windows") {
            assert_eq!(order, vec![BackendKind::D3d12, BackendKind::Vulkan]);
        } else if cfg!(target_os = "linux") {
            assert_eq!(order, vec![BackendKind::Vulkan]);
        } else if cfg!(target_os = "macos") {
            assert_eq!(order, vec![BackendKind::Vulkan, BackendKind::Metal]);
        } else if cfg!(any(
            target_os = "ios",
            target_os = "tvos",
            target_os = "visionos"
        )) {
            assert_eq!(order, vec![BackendKind::Metal]);
        }
    }
}
