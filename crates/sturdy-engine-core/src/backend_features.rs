/// Backend features exposed to engine callers.
///
/// These flags describe executable engine support, not just raw driver
/// extension presence. Backends should leave a feature disabled until the engine
/// can create the required objects and record the corresponding work safely.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct BackendFeatures {
    pub ray_tracing: bool,
    pub mesh_shading: bool,
    pub bindless: bool,
    pub descriptor_indexing: bool,
    pub timeline_semaphores: bool,
    pub dynamic_rendering: bool,
    pub synchronization2: bool,
    /// VK_KHR_buffer_device_address (or Vulkan 1.2 core) — expose GPU virtual
    /// addresses for buffers created with SHADER_DEVICE_ADDRESS usage.
    pub buffer_device_address: bool,
    /// VK_KHR_ray_query — inline ray intersection queries from any shader stage.
    pub ray_query: bool,
    /// VK_KHR_ray_tracing_position_fetch — fetch vertex positions in hit shaders.
    pub ray_tracing_position_fetch: bool,
    /// VK_KHR_ray_tracing_maintenance1 — maintenance fixes for RT pipelines.
    pub ray_tracing_maintenance1: bool,
    /// VK_EXT_opacity_micromap — opacity micromap for alpha-tested geometry.
    pub opacity_micromap: bool,
    /// VK_EXT_ray_tracing_invocation_reorder — shader execution reordering (SER).
    pub shader_execution_reordering: bool,
    /// VK_NV_cluster_acceleration_structure — cluster acceleration structures.
    pub cluster_acceleration_structure: bool,
    pub hdr_output: bool,
    pub shader_fp16: bool,
    pub shader_fp64: bool,
    pub image_fp16_render: bool,
    pub image_fp32_render: bool,
    pub variable_rate_shading: bool,
    /// Per-draw pipeline fragment shading rate is enabled (tier 1).
    pub vrs_pipeline: bool,
    /// Per-primitive fragment shading rate is enabled (tier 2).
    pub vrs_primitive: bool,
    /// Attachment-based fragment shading rate is enabled (tier 3).
    pub vrs_attachment: bool,
    /// Multiple indirect draw commands can be submitted by one API call.
    pub multi_draw_indirect: bool,
    /// GPU-visible count buffers can limit indirect draw command consumption.
    pub draw_indirect_count: bool,
    /// Backend can create sparse/tiled resources with explicit page binding.
    pub sparse_binding: bool,
    /// Sparse buffers are supported.
    pub sparse_residency_buffer: bool,
    /// Sparse 2D images are supported.
    pub sparse_residency_image_2d: bool,
    /// Sparse 3D images are supported.
    pub sparse_residency_image_3d: bool,
    /// Multiple sparse resources can alias the same memory pages.
    pub sparse_residency_aliased: bool,
    /// VK_EXT_memory_budget — query per-heap budget and usage.
    pub memory_budget: bool,
    /// VK_EXT_memory_priority — assign allocation priority hints.
    pub memory_priority: bool,
    /// VK_EXT_pageable_device_local_memory — allow eviction of device-local pages.
    pub pageable_device_local_memory: bool,
    /// VK_EXT_device_fault — retrieve breadcrumbs after VK_ERROR_DEVICE_LOST.
    pub device_fault: bool,
    /// VK_NV_device_diagnostic_checkpoints — pass markers that survive device loss.
    pub device_diagnostic_checkpoints_nv: bool,
    /// VK_AMD_buffer_marker — command-buffer breadcrumbs written into GPU buffers.
    pub buffer_marker_amd: bool,
    /// VK_EXT_device_address_binding_report — debug callback events for GPU VA bindings.
    pub device_address_binding_report: bool,
    /// VK_EXT_device_memory_report — debug callback events for driver memory allocations.
    pub device_memory_report: bool,
    /// VK_EXT_host_image_copy (or Vulkan 1.4 core) — CPU→GPU image uploads
    /// without a staging buffer or explicit transfer command.
    pub host_image_copy: bool,
    /// VkPhysicalDeviceFeatures::samplerAnisotropy — anisotropic filtering.
    pub sampler_anisotropy: bool,
    /// VK_KHR_push_descriptor — inline descriptor updates pushed directly into
    /// a command buffer without a descriptor pool.
    pub push_descriptors: bool,
    /// VK_EXT_conditional_rendering — GPU-side predicate that skips draw/dispatch
    /// commands based on a buffer value.
    pub conditional_rendering: bool,
    /// VK_EXT_extended_dynamic_state3 — additional pipeline state settable dynamically.
    pub extended_dynamic_state3: bool,
    /// Polygon mode can be set dynamically (part of extended_dynamic_state3).
    pub extended_dynamic_state3_polygon_mode: bool,
    /// Color blend enable can be set dynamically (part of extended_dynamic_state3).
    pub extended_dynamic_state3_color_blend: bool,
    /// VK_EXT_vertex_input_dynamic_state — vertex input state settable dynamically.
    pub vertex_input_dynamic_state: bool,
    /// VK_EXT_conservative_rasterization overestimation mode.
    pub conservative_rasterization_overestimate: bool,
    /// VK_EXT_conservative_rasterization underestimation mode.
    pub conservative_rasterization_underestimate: bool,
    /// VK_KHR_global_priority — assign global GPU scheduler priority to queues.
    pub global_queue_priority: bool,
    /// VK_EXT_sampler_filter_minmax (core 1.2) — min/max reduction sampler filter.
    pub sampler_filter_minmax: bool,
    /// VK_EXT_custom_border_color — arbitrary border color for samplers.
    pub custom_border_color: bool,
    /// VK_EXT_filter_cubic — cubic filtering for sampled images.
    pub filter_cubic: bool,
    /// VK_EXT_image_view_min_lod — clamp minimum LOD in an image view.
    pub image_view_min_lod: bool,
    /// VK_EXT_image_compression_control — explicit lossy/lossless compression hints.
    pub image_compression_control: bool,
    /// VK_EXT_multisampled_render_to_single_sampled — render MSAA into single-sample storage.
    pub msaa_render_to_single_sampled: bool,
    /// VK_EXT_descriptor_buffer — descriptors stored in app-managed GPU buffers.
    pub descriptor_buffer: bool,
    /// VK_EXT_descriptor_heap — D3D12-style resource + sampler descriptor heaps.
    pub descriptor_heap: bool,
    /// VK_AMDX_shader_enqueue — AMD work graphs (shader-enqueued dispatch).
    pub work_graphs: bool,
    /// VK_EXT_shader_object — pipeline-free shader binding.
    pub shader_object: bool,

    // ── GFX-4: Video encode/decode ────────────────────────────────────────────
    /// VK_KHR_video_queue — base video queue support.
    pub video_queue: bool,
    /// VK_KHR_video_maintenance1 — simplifies session parameter management.
    pub video_maintenance1: bool,
    /// VK_KHR_video_maintenance2 — additional session management improvements.
    pub video_maintenance2: bool,
    /// VK_KHR_video_decode_h264 — H.264 video decode.
    pub video_decode_h264: bool,
    /// VK_KHR_video_decode_h265 — H.265 video decode.
    pub video_decode_h265: bool,
    /// VK_KHR_video_decode_av1 — AV1 video decode.
    pub video_decode_av1: bool,
    /// VK_KHR_video_decode_vp9 — VP9 video decode.
    pub video_decode_vp9: bool,
    /// VK_KHR_video_encode_h264 — H.264 video encode.
    pub video_encode_h264: bool,
    /// VK_KHR_video_encode_h265 — H.265 video encode.
    pub video_encode_h265: bool,
    /// VK_KHR_video_encode_av1 — AV1 video encode.
    pub video_encode_av1: bool,
    /// VK_KHR_video_encode_quantization_map — quantization map for encode quality.
    pub video_encode_quantization_map: bool,

    // ── GFX-5: External resource interop ─────────────────────────────────────
    /// VK_KHR_external_memory_fd — POSIX fd-based external memory import/export.
    pub external_memory_fd: bool,
    /// VK_KHR_external_memory_win32 — Win32 handle-based external memory import/export.
    pub external_memory_win32: bool,
    /// VK_EXT_external_memory_dma_buf — DMA-BUF external memory (Linux/Android).
    pub external_memory_dma_buf: bool,
    /// VK_EXT_external_memory_host — host pointer external memory import.
    pub external_memory_host: bool,
    /// VK_EXT_image_drm_format_modifier — DRM format modifiers for tiled images.
    pub drm_format_modifier: bool,
    /// VK_KHR_external_semaphore_fd — POSIX fd-based external semaphore import/export.
    pub external_semaphore_fd: bool,
    /// VK_KHR_external_semaphore_win32 — Win32 handle-based external semaphore import/export.
    pub external_semaphore_win32: bool,
    /// VK_KHR_external_fence_fd — POSIX fd-based external fence import/export.
    pub external_fence_fd: bool,
    /// VK_KHR_external_fence_win32 — Win32 handle-based external fence import/export.
    pub external_fence_win32: bool,

    // ── GFX-6a: Device-generated commands ────────────────────────────────────
    /// VK_EXT_device_generated_commands — Khronos standard DGC.
    pub device_generated_commands: bool,
    /// VK_NV_device_generated_commands — NVIDIA proprietary DGC.
    pub device_generated_commands_nv: bool,

    // ── GFX-6b: Latency reduction ─────────────────────────────────────────────
    /// VK_NV_low_latency2 — NVIDIA Reflex low-latency mode.
    pub reflex: bool,
    /// VK_AMD_anti_lag — AMD Anti-Lag.
    pub anti_lag: bool,

    // ── GFX-6c: Cooperative matrix ───────────────────────────────────────────
    /// VK_KHR_cooperative_matrix — Khronos standard cooperative matrix.
    pub cooperative_matrix: bool,
    /// VK_NV_cooperative_matrix — NVIDIA cooperative matrix v1.
    pub cooperative_matrix_nv: bool,
    /// VK_NV_cooperative_matrix2 — NVIDIA cooperative matrix v2.
    pub cooperative_matrix_nv2: bool,

    // ── GFX-6d: Advanced shader features ─────────────────────────────────────
    /// VK_KHR_fragment_shader_barycentric — barycentric coordinates in fragment shaders.
    pub fragment_shader_barycentric: bool,
    /// VK_EXT_fragment_shader_interlock — ordered rasterizer invocations (ROV).
    pub fragment_shader_interlock: bool,
    /// VK_EXT_shader_atomic_float — fp32 atomic add/min/max on buffers and images.
    pub shader_atomic_float: bool,
    /// VK_EXT_shader_atomic_float2 — fp16 atomic and additional fp32 atomics.
    pub shader_atomic_float16: bool,
    /// VK_KHR_compute_shader_derivatives — dFdx/dFdy in compute shaders.
    pub compute_shader_derivatives: bool,
    /// VK_KHR_shader_clock — realtime clock reads in shaders.
    pub shader_clock: bool,
    /// VK_EXT_post_depth_coverage — post-depth coverage sample mask modifier.
    pub post_depth_coverage: bool,

    // ── GFX-6e: Optical flow ─────────────────────────────────────────────────
    /// VK_NV_optical_flow — NVIDIA hardware optical flow estimation.
    pub optical_flow_nv: bool,

    // ── GFX-2h: Presentation extensions ──────────────────────────────────────
    /// VK_EXT_swapchain_colorspace — extended color-space support.
    pub swapchain_colorspace: bool,
    /// VK_KHR_present_id — attach a monotonic ID to each present.
    pub present_id: bool,
    /// VK_KHR_present_wait — wait for a specific present ID to complete.
    pub present_wait: bool,
    /// VK_KHR_swapchain_maintenance1 — swapchain maintenance fixes.
    pub swapchain_maintenance1: bool,
    /// VK_EXT_full_screen_exclusive — request exclusive full-screen.
    pub full_screen_exclusive: bool,
    /// VK_GOOGLE_display_timing — query and control display timing.
    pub display_timing: bool,
    /// VK_EXT_present_mode_fifo_latest_ready — FIFO with latest-ready frames.
    pub present_mode_fifo_latest_ready: bool,

    // ── GFX-2j: Performance query ─────────────────────────────────────────────
    /// VK_KHR_performance_query — hardware performance counters.
    pub performance_query: bool,
    /// VK_KHR_pipeline_executable_properties — inspect compiled pipeline internals.
    pub pipeline_executable_properties: bool,

    // ── GFX-2c: Pipeline library ──────────────────────────────────────────────
    /// VK_EXT_graphics_pipeline_library — pre-compile disjoint pipeline stages.
    pub graphics_pipeline_library: bool,
    /// VK_EXT_pipeline_creation_cache_control — non-blocking PSO compilation.
    pub pipeline_creation_cache_control: bool,

    // ── GFX-2j: AMD shader info ───────────────────────────────────────────────
    /// VK_AMD_shader_info — compiled shader statistics (VGPR/SGPR usage, code size).
    pub shader_info_amd: bool,

    // ── GFX-2k: Image compression swapchain ──────────────────────────────────
    /// VK_EXT_image_compression_control_swapchain — apply compression hint to swapchain images.
    pub image_compression_control_swapchain: bool,

    // ── GFX-5: External memory exports ───────────────────────────────────────
    /// VK_KHR_external_memory_fd is available and buffer/image export is supported.
    pub external_memory_fd_export: bool,

    // ── Vulkan 1.4 / modern maintenance ──────────────────────────────────────
    /// VK_KHR_maintenance5 (core in Vulkan 1.4) — `maintenance5` feature bit.
    /// Enables `VkBufferUsageFlags2CreateInfoKHR`, `renderingAreaInfo` dynamic
    /// rendering area, and pipeline identifier queries.
    pub maintenance5: bool,
    /// VK_KHR_maintenance6 (core in Vulkan 1.4) — `maintenance6` feature bit.
    /// Enables `VkBindDescriptorSetsInfoKHR` for atomic multi-set push-descriptor
    /// binding and per-draw-call partial binding.
    pub maintenance6: bool,
    /// VK_KHR_dynamic_rendering_local_read (core in Vulkan 1.4) — enables
    /// in-flight reads from color/depth attachments within the same dynamic
    /// render pass without explicit barriers or layout transitions.
    /// Essential for efficient deferred lighting with dynamic rendering.
    pub dynamic_rendering_local_read: bool,
    /// VK_EXT_robustness2 `nullDescriptor` feature — allows binding
    /// null/invalid descriptor handles (e.g. `MATERIAL_NO_TEXTURE` sentinel)
    /// without triggering undefined behaviour in the driver.
    pub null_descriptor: bool,
    /// VK_EXT_depth_clip_enable — allows enabling/disabling depth clipping
    /// dynamically (per-draw) without a full pipeline state object switch.
    pub depth_clip_enable: bool,
    /// VK_KHR_calibrated_timestamps (core in Vulkan 1.4) or VK_EXT variant —
    /// allows correlating GPU timestamps with a CPU clock domain for accurate
    /// CPU+GPU frame-time correlation.
    pub calibrated_timestamps: bool,
    /// VK_EXT_shader_module_identifier — allows querying a stable identifier
    /// (hash) for compiled shader modules. Used by the pipeline cache to
    /// skip re-compilation of already-compiled shaders across sessions.
    pub shader_module_identifier: bool,
}

impl BackendFeatures {
    pub(crate) fn disable_device_generated_command_features(&mut self) {
        self.device_generated_commands = false;
        self.device_generated_commands_nv = false;
    }

    pub(crate) fn disable_optical_flow_features(&mut self) {
        self.optical_flow_nv = false;
    }

    pub(crate) fn disable_anti_lag_features(&mut self) {
        self.anti_lag = false;
    }

    pub(crate) fn disable_video_features(&mut self) {
        self.video_queue = false;
        self.video_maintenance1 = false;
        self.video_maintenance2 = false;
        self.video_decode_h264 = false;
        self.video_decode_h265 = false;
        self.video_decode_av1 = false;
        self.video_decode_vp9 = false;
        self.video_encode_h264 = false;
        self.video_encode_h265 = false;
        self.video_encode_av1 = false;
        self.video_encode_quantization_map = false;
    }
}

#[cfg(test)]
#[path = "backend_features_tests.rs"]
mod tests;

impl BackendFeatures {
    /// True when any tier of variable-rate shading is available.
    pub fn supports_variable_rate_shading(&self) -> bool {
        self.variable_rate_shading || self.vrs_pipeline || self.vrs_primitive || self.vrs_attachment
    }

    /// True when at least one conservative rasterization mode is available.
    pub fn supports_conservative_rasterization(&self) -> bool {
        self.conservative_rasterization_overestimate
            || self.conservative_rasterization_underestimate
    }

    /// True when the backend can support sparse/tiled 2D texture residency.
    pub fn supports_sparse_2d_textures(&self) -> bool {
        self.sparse_binding && self.sparse_residency_image_2d
    }

    /// True when the backend can support sparse/tiled 3D texture residency.
    pub fn supports_sparse_3d_textures(&self) -> bool {
        self.sparse_binding && self.sparse_residency_image_3d
    }

    /// True when the backend can support sparse/tiled buffer residency.
    pub fn supports_sparse_buffers(&self) -> bool {
        self.sparse_binding && self.sparse_residency_buffer
    }

    /// True when the backend can consume GPU-compacted indirect draw streams
    /// through a count buffer.
    pub fn supports_gpu_draw_compaction(&self) -> bool {
        self.multi_draw_indirect && self.draw_indirect_count
    }

    /// All [`BackendFeature`] variants that are currently enabled on this device.
    ///
    /// Iterates every known variant and includes it when the corresponding
    /// flag is `true`.  Use this to display a feature list, drive UI toggles,
    /// or compare feature sets across devices.
    pub fn enabled_features(&self) -> Vec<crate::BackendFeature> {
        crate::BackendFeature::ALL
            .iter()
            .copied()
            .filter(|f| self.has(*f))
            .collect()
    }

    /// Returns `true` when the given [`BackendFeature`] variant is active.
    pub fn has(&self, feature: crate::BackendFeature) -> bool {
        use crate::BackendFeature as F;
        match feature {
            F::RayTracing => self.ray_tracing,
            F::AccelerationStructure => self.ray_tracing, // AS is required for RT
            F::RayQuery => self.ray_query,
            F::RayTracingPositionFetch => self.ray_tracing_position_fetch,
            F::RayTracingMaintenance1 => self.ray_tracing_maintenance1,
            F::ClusterAccelerationStructure => self.cluster_acceleration_structure,
            F::MeshShader => self.mesh_shading,
            F::TaskShader => self.mesh_shading, // task implies mesh
            F::PipelineFragmentShadingRate => self.vrs_pipeline,
            F::PrimitiveFragmentShadingRate => self.vrs_primitive,
            F::AttachmentFragmentShadingRate => self.vrs_attachment,
            F::BindlessResources => self.bindless,
            F::PushDescriptor => self.push_descriptors,
            F::DescriptorBuffer => self.descriptor_buffer,
            F::DescriptorHeap => self.descriptor_heap,
            F::TimelineSemaphore => self.timeline_semaphores,
            F::Synchronization2 => self.synchronization2,
            F::DynamicRendering => self.dynamic_rendering,
            F::BufferDeviceAddress => self.buffer_device_address,
            F::MemoryBudget => self.memory_budget,
            F::MemoryPriority => self.memory_priority,
            F::PageableDeviceLocalMemory => self.pageable_device_local_memory,
            F::HostImageCopy => self.host_image_copy,
            F::ConditionalRendering => self.conditional_rendering,
            F::ShaderObject => self.shader_object,
            F::DeviceGeneratedCommandsNv => self.device_generated_commands_nv,
            F::DeviceGeneratedCommands => self.device_generated_commands,
            F::GraphicsPipelineLibrary => self.graphics_pipeline_library,
            F::ExtendedDynamicState3 => self.extended_dynamic_state3,
            F::VertexInputDynamicState => self.vertex_input_dynamic_state,
            F::SamplerAnisotropy => self.sampler_anisotropy,
            F::SamplerFilterMinmax => self.sampler_filter_minmax,
            F::CustomBorderColor => self.custom_border_color,
            F::FilterCubic => self.filter_cubic,
            F::ImageViewMinLod => self.image_view_min_lod,
            F::ImageCompressionControl => self.image_compression_control,
            F::MsaaRenderToSingleSampled => self.msaa_render_to_single_sampled,
            F::ConservativeRasterization => self.supports_conservative_rasterization(),
            F::GlobalQueuePriority => self.global_queue_priority,
            F::DeviceFault => self.device_fault,
            F::DeviceDiagnosticCheckpointsNv => self.device_diagnostic_checkpoints_nv,
            F::BufferMarkerAmd => self.buffer_marker_amd,
            F::DeviceAddressBindingReport => self.device_address_binding_report,
            F::DeviceMemoryReport => self.device_memory_report,
            F::PerformanceQuery => self.performance_query,
            F::PipelineExecutableProperties => self.pipeline_executable_properties,
            F::Reflex => self.reflex,
            F::AntiLag => self.anti_lag,
            F::FragmentShaderBarycentric => self.fragment_shader_barycentric,
            F::FragmentShaderInterlock => self.fragment_shader_interlock,
            F::ShaderAtomicFloat => self.shader_atomic_float,
            F::ComputeShaderDerivatives => self.compute_shader_derivatives,
            F::ShaderClock => self.shader_clock,
            F::WorkGraphs => self.work_graphs,
            F::CooperativeMatrix => self.cooperative_matrix,
            F::OpticalFlowNv => self.optical_flow_nv,
            F::ExternalMemoryFd => self.external_memory_fd,
            F::ExternalSemaphoreFd => self.external_semaphore_fd,
            F::ExternalFenceFd => self.external_fence_fd,
            F::VideoQueue => self.video_queue,
            F::VideoDecodeH264 => self.video_decode_h264,
            F::VideoDecodeH265 => self.video_decode_h265,
            F::VideoDecodeAv1 => self.video_decode_av1,
            F::VideoEncodeH264 => self.video_encode_h264,
            F::VideoEncodeH265 => self.video_encode_h265,
            F::VideoEncodeAv1 => self.video_encode_av1,
            F::Maintenance5 => self.maintenance5,
            F::Maintenance6 => self.maintenance6,
            F::DynamicRenderingLocalRead => self.dynamic_rendering_local_read,
            F::NullDescriptor => self.null_descriptor,
            F::DepthClipEnable => self.depth_clip_enable,
            F::CalibratedTimestamps => self.calibrated_timestamps,
            F::ShaderModuleIdentifier => self.shader_module_identifier,
        }
    }
}
