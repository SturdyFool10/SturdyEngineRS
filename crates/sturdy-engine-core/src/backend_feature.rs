/// Every backend feature/extension that can be requested at device-creation time
/// or queried at runtime.
///
/// Variants map 1-to-1 to the string names accepted by
/// [`DeviceDesc::prefer_backend_feature`] and [`DeviceDesc::require_backend_feature`].
/// Use [`BackendFeature::feature_name`] to get the canonical string, or
/// [`BackendFeature::from_feature_name`] to parse one.
///
/// Pass variants to [`BackendFeatureChange::enable`] / [`BackendFeatureChange::disable`]
/// instead of raw strings to get compile-time safety.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum BackendFeature {
    // ── Ray tracing ───────────────────────────────────────────────────────────
    /// VK_KHR_ray_tracing_pipeline + VK_KHR_acceleration_structure.
    RayTracing,
    /// VK_KHR_acceleration_structure only (without RT pipeline).
    AccelerationStructure,
    /// VK_KHR_ray_query — inline ray queries from any shader stage.
    RayQuery,
    /// VK_KHR_ray_tracing_position_fetch — vertex positions in hit shaders.
    RayTracingPositionFetch,
    /// VK_KHR_ray_tracing_maintenance1 — maintenance fixes for RT pipelines.
    RayTracingMaintenance1,
    /// VK_NV_cluster_acceleration_structure — cluster AS builds.
    ClusterAccelerationStructure,

    // ── Mesh / task shading ───────────────────────────────────────────────────
    /// VK_EXT_mesh_shader (mesh stage).
    MeshShader,
    /// VK_EXT_mesh_shader (task stage; implies MeshShader).
    TaskShader,

    // ── Variable rate shading ─────────────────────────────────────────────────
    /// Per-pipeline (tier 1) fragment shading rate.
    PipelineFragmentShadingRate,
    /// Per-primitive (tier 2) fragment shading rate.
    PrimitiveFragmentShadingRate,
    /// Attachment-based (tier 3) fragment shading rate.
    AttachmentFragmentShadingRate,

    // ── Descriptors ───────────────────────────────────────────────────────────
    /// VK_EXT_descriptor_indexing — bindless resource arrays.
    BindlessResources,
    /// VK_KHR_push_descriptor — inline descriptors pushed into a command buffer.
    PushDescriptor,
    /// VK_EXT_descriptor_buffer — descriptors in app-managed GPU buffers.
    DescriptorBuffer,
    /// VK_EXT_descriptor_heap — D3D12-style resource/sampler heap.
    DescriptorHeap,

    // ── Synchronisation ───────────────────────────────────────────────────────
    /// VK_KHR_timeline_semaphore (core 1.2).
    TimelineSemaphore,
    /// VK_KHR_synchronization2 (core 1.3).
    Synchronization2,
    /// VK_KHR_dynamic_rendering (core 1.3).
    DynamicRendering,

    // ── Memory ────────────────────────────────────────────────────────────────
    /// VK_KHR_buffer_device_address (core 1.2).
    BufferDeviceAddress,
    /// VK_EXT_memory_budget — per-heap VRAM budget query.
    MemoryBudget,
    /// VK_EXT_memory_priority — allocation priority hints.
    MemoryPriority,
    /// VK_EXT_pageable_device_local_memory — driver-managed page eviction.
    PageableDeviceLocalMemory,
    /// VK_EXT_host_image_copy (core 1.4) — CPU-to-GPU image upload without staging.
    HostImageCopy,

    // ── Draw / dispatch ───────────────────────────────────────────────────────
    /// Conditional rendering (GPU-side predicate skipping draws/dispatches).
    ConditionalRendering,
    /// Shader objects (pipeline-free shader binding).
    ShaderObject,
    /// VK_NV_device_generated_commands — NVIDIA proprietary DGC.
    DeviceGeneratedCommandsNv,
    /// VK_EXT_device_generated_commands — Khronos standard DGC.
    DeviceGeneratedCommands,
    /// VK_EXT_graphics_pipeline_library — pre-compile disjoint pipeline stages.
    GraphicsPipelineLibrary,

    // ── Dynamic state ─────────────────────────────────────────────────────────
    /// VK_EXT_extended_dynamic_state3 — additional dynamic pipeline state.
    ExtendedDynamicState3,
    /// VK_EXT_vertex_input_dynamic_state — dynamic vertex input format.
    VertexInputDynamicState,

    // ── Sampler / image ───────────────────────────────────────────────────────
    /// Anisotropic filtering.
    SamplerAnisotropy,
    /// VK_EXT_sampler_filter_minmax (core 1.2) — min/max reduction mode.
    SamplerFilterMinmax,
    /// VK_EXT_custom_border_color — arbitrary border color for samplers.
    CustomBorderColor,
    /// VK_EXT_filter_cubic — cubic filter mode for sampled images.
    FilterCubic,
    /// VK_EXT_image_view_min_lod — clamp minimum LOD in an image view.
    ImageViewMinLod,
    /// VK_EXT_image_compression_control — explicit lossy/lossless compression hints.
    ImageCompressionControl,
    /// VK_EXT_multisampled_render_to_single_sampled — MSAA without resolve attachment.
    MsaaRenderToSingleSampled,

    // ── Rasterisation ─────────────────────────────────────────────────────────
    /// VK_EXT_conservative_rasterization.
    ConservativeRasterization,

    // ── Queue / scheduling ────────────────────────────────────────────────────
    /// VK_KHR_global_priority — queue scheduler priority.
    GlobalQueuePriority,

    // ── Debug / diagnostic ────────────────────────────────────────────────────
    /// VK_EXT_device_fault — breadcrumbs after VK_ERROR_DEVICE_LOST.
    DeviceFault,
    /// VK_NV_device_diagnostic_checkpoints — NV pass markers for device loss.
    DeviceDiagnosticCheckpointsNv,
    /// VK_AMD_buffer_marker — AMD breadcrumb writes.
    BufferMarkerAmd,
    /// VK_EXT_device_address_binding_report — debug callbacks for GPU VA bindings.
    DeviceAddressBindingReport,
    /// VK_EXT_device_memory_report — debug callbacks for driver memory events.
    DeviceMemoryReport,

    // ── Performance ───────────────────────────────────────────────────────────
    /// VK_KHR_performance_query — hardware performance counters.
    PerformanceQuery,
    /// VK_KHR_pipeline_executable_properties — compiled pipeline inspection.
    PipelineExecutableProperties,

    // ── Latency reduction ─────────────────────────────────────────────────────
    /// VK_NV_low_latency2 — NVIDIA Reflex.
    Reflex,
    /// VK_AMD_anti_lag — AMD Anti-Lag.
    AntiLag,

    // ── Shader features ───────────────────────────────────────────────────────
    /// VK_KHR_fragment_shader_barycentric — barycentric coordinates in FS.
    FragmentShaderBarycentric,
    /// VK_EXT_fragment_shader_interlock — rasterizer-ordered views (ROV).
    FragmentShaderInterlock,
    /// VK_EXT_shader_atomic_float — fp32 atomic ops on buffers/images.
    ShaderAtomicFloat,
    /// VK_KHR_compute_shader_derivatives — dFdx/dFdy in compute.
    ComputeShaderDerivatives,
    /// VK_KHR_shader_clock — realtime clock reads in shaders.
    ShaderClock,
    /// VK_AMDX_shader_enqueue — AMD work graphs.
    WorkGraphs,
    /// VK_KHR_cooperative_matrix — Khronos cooperative matrix.
    CooperativeMatrix,

    // ── Optical flow ─────────────────────────────────────────────────────────
    /// VK_NV_optical_flow — NVIDIA hardware optical flow.
    OpticalFlowNv,

    // ── External interop ─────────────────────────────────────────────────────
    /// VK_KHR_external_memory_fd — POSIX fd external memory.
    ExternalMemoryFd,
    /// VK_KHR_external_semaphore_fd — POSIX fd external semaphore.
    ExternalSemaphoreFd,
    /// VK_KHR_external_fence_fd — POSIX fd external fence.
    ExternalFenceFd,

    // ── Video ─────────────────────────────────────────────────────────────────
    /// VK_KHR_video_queue — base video queue (required for all video).
    VideoQueue,
    /// VK_KHR_video_decode_h264.
    VideoDecodeH264,
    /// VK_KHR_video_decode_h265.
    VideoDecodeH265,
    /// VK_KHR_video_decode_av1.
    VideoDecodeAv1,
    /// VK_KHR_video_encode_h264.
    VideoEncodeH264,
    /// VK_KHR_video_encode_h265.
    VideoEncodeH265,
    /// VK_KHR_video_encode_av1.
    VideoEncodeAv1,
}

impl BackendFeature {
    /// The canonical string accepted by [`DeviceDesc`]'s `optional_features` /
    /// `required_features` lists and displayed in unsupported-feature error messages.
    pub const fn feature_name(self) -> &'static str {
        match self {
            Self::RayTracing => "ray_tracing",
            Self::AccelerationStructure => "acceleration_structure",
            Self::RayQuery => "ray_query",
            Self::RayTracingPositionFetch => "ray_tracing_position_fetch",
            Self::RayTracingMaintenance1 => "ray_tracing_maintenance1",
            Self::ClusterAccelerationStructure => "cluster_acceleration_structure",
            Self::MeshShader => "mesh_shader",
            Self::TaskShader => "task_shader",
            Self::PipelineFragmentShadingRate => "pipeline_fragment_shading_rate",
            Self::PrimitiveFragmentShadingRate => "primitive_fragment_shading_rate",
            Self::AttachmentFragmentShadingRate => "attachment_fragment_shading_rate",
            Self::BindlessResources => "bindless_resources",
            Self::PushDescriptor => "push_descriptor",
            Self::DescriptorBuffer => "descriptor_buffer",
            Self::DescriptorHeap => "descriptor_heap",
            Self::TimelineSemaphore => "timeline_semaphore",
            Self::Synchronization2 => "synchronization2",
            Self::DynamicRendering => "dynamic_rendering",
            Self::BufferDeviceAddress => "buffer_device_address",
            Self::MemoryBudget => "memory_budget",
            Self::MemoryPriority => "memory_priority",
            Self::PageableDeviceLocalMemory => "pageable_device_local_memory",
            Self::HostImageCopy => "host_image_copy",
            Self::ConditionalRendering => "conditional_rendering",
            Self::ShaderObject => "shader_object",
            Self::DeviceGeneratedCommandsNv => "device_generated_commands_nv",
            Self::DeviceGeneratedCommands => "device_generated_commands",
            Self::GraphicsPipelineLibrary => "graphics_pipeline_library",
            Self::ExtendedDynamicState3 => "extended_dynamic_state3",
            Self::VertexInputDynamicState => "vertex_input_dynamic_state",
            Self::SamplerAnisotropy => "sampler_anisotropy",
            Self::SamplerFilterMinmax => "sampler_filter_minmax",
            Self::CustomBorderColor => "custom_border_color",
            Self::FilterCubic => "filter_cubic",
            Self::ImageViewMinLod => "image_view_min_lod",
            Self::ImageCompressionControl => "image_compression_control",
            Self::MsaaRenderToSingleSampled => "msaa_render_to_single_sampled",
            Self::ConservativeRasterization => "conservative_rasterization",
            Self::GlobalQueuePriority => "global_queue_priority",
            Self::DeviceFault => "device_fault",
            Self::DeviceDiagnosticCheckpointsNv => "device_diagnostic_checkpoints_nv",
            Self::BufferMarkerAmd => "buffer_marker_amd",
            Self::DeviceAddressBindingReport => "device_address_binding_report",
            Self::DeviceMemoryReport => "device_memory_report",
            Self::PerformanceQuery => "performance_query",
            Self::PipelineExecutableProperties => "pipeline_executable_properties",
            Self::Reflex => "reflex",
            Self::AntiLag => "anti_lag",
            Self::FragmentShaderBarycentric => "fragment_shader_barycentric",
            Self::FragmentShaderInterlock => "fragment_shader_interlock",
            Self::ShaderAtomicFloat => "shader_atomic_float",
            Self::ComputeShaderDerivatives => "compute_shader_derivatives",
            Self::ShaderClock => "shader_clock",
            Self::WorkGraphs => "work_graphs",
            Self::CooperativeMatrix => "cooperative_matrix",
            Self::OpticalFlowNv => "optical_flow_nv",
            Self::ExternalMemoryFd => "external_memory_fd",
            Self::ExternalSemaphoreFd => "external_semaphore_fd",
            Self::ExternalFenceFd => "external_fence_fd",
            Self::VideoQueue => "video_queue",
            Self::VideoDecodeH264 => "video_decode_h264",
            Self::VideoDecodeH265 => "video_decode_h265",
            Self::VideoDecodeAv1 => "video_decode_av1",
            Self::VideoEncodeH264 => "video_encode_h264",
            Self::VideoEncodeH265 => "video_encode_h265",
            Self::VideoEncodeAv1 => "video_encode_av1",
        }
    }

    /// Parse a feature from its canonical string name.  Returns `None` for unknown names.
    pub fn from_feature_name(name: &str) -> Option<Self> {
        Some(match name {
            "ray_tracing" | "ray_tracing_pipeline" => Self::RayTracing,
            "acceleration_structure" => Self::AccelerationStructure,
            "ray_query" => Self::RayQuery,
            "ray_tracing_position_fetch" => Self::RayTracingPositionFetch,
            "ray_tracing_maintenance1" => Self::RayTracingMaintenance1,
            "cluster_acceleration_structure" => Self::ClusterAccelerationStructure,
            "mesh_shader" | "mesh_shading" => Self::MeshShader,
            "task_shader" => Self::TaskShader,
            "pipeline_fragment_shading_rate" | "variable_rate_shading" | "vrs_pipeline" => {
                Self::PipelineFragmentShadingRate
            }
            "primitive_fragment_shading_rate" | "vrs_primitive" => {
                Self::PrimitiveFragmentShadingRate
            }
            "attachment_fragment_shading_rate" | "vrs_attachment" => {
                Self::AttachmentFragmentShadingRate
            }
            "bindless_resources" | "descriptor_indexing" => Self::BindlessResources,
            "push_descriptor" | "push_descriptors" => Self::PushDescriptor,
            "descriptor_buffer" => Self::DescriptorBuffer,
            "descriptor_heap" => Self::DescriptorHeap,
            "timeline_semaphore" | "timeline_semaphores" => Self::TimelineSemaphore,
            "synchronization2" => Self::Synchronization2,
            "dynamic_rendering" => Self::DynamicRendering,
            "buffer_device_address" => Self::BufferDeviceAddress,
            "memory_budget" => Self::MemoryBudget,
            "memory_priority" => Self::MemoryPriority,
            "pageable_device_local_memory" => Self::PageableDeviceLocalMemory,
            "host_image_copy" => Self::HostImageCopy,
            "conditional_rendering" => Self::ConditionalRendering,
            "shader_object" => Self::ShaderObject,
            "device_generated_commands_nv" => Self::DeviceGeneratedCommandsNv,
            "device_generated_commands" => Self::DeviceGeneratedCommands,
            "graphics_pipeline_library" => Self::GraphicsPipelineLibrary,
            "extended_dynamic_state3" => Self::ExtendedDynamicState3,
            "vertex_input_dynamic_state" => Self::VertexInputDynamicState,
            "sampler_anisotropy" => Self::SamplerAnisotropy,
            "sampler_filter_minmax" => Self::SamplerFilterMinmax,
            "custom_border_color" => Self::CustomBorderColor,
            "filter_cubic" => Self::FilterCubic,
            "image_view_min_lod" => Self::ImageViewMinLod,
            "image_compression_control" => Self::ImageCompressionControl,
            "msaa_render_to_single_sampled" => Self::MsaaRenderToSingleSampled,
            "conservative_rasterization" => Self::ConservativeRasterization,
            "global_queue_priority" => Self::GlobalQueuePriority,
            "device_fault" => Self::DeviceFault,
            "device_diagnostic_checkpoints_nv" => Self::DeviceDiagnosticCheckpointsNv,
            "buffer_marker_amd" => Self::BufferMarkerAmd,
            "device_address_binding_report" => Self::DeviceAddressBindingReport,
            "device_memory_report" => Self::DeviceMemoryReport,
            "performance_query" => Self::PerformanceQuery,
            "pipeline_executable_properties" => Self::PipelineExecutableProperties,
            "reflex" => Self::Reflex,
            "anti_lag" => Self::AntiLag,
            "fragment_shader_barycentric" => Self::FragmentShaderBarycentric,
            "fragment_shader_interlock" => Self::FragmentShaderInterlock,
            "shader_atomic_float" => Self::ShaderAtomicFloat,
            "compute_shader_derivatives" => Self::ComputeShaderDerivatives,
            "shader_clock" => Self::ShaderClock,
            "work_graphs" => Self::WorkGraphs,
            "cooperative_matrix" => Self::CooperativeMatrix,
            "optical_flow_nv" | "optical_flow" => Self::OpticalFlowNv,
            "external_memory_fd" => Self::ExternalMemoryFd,
            "external_semaphore_fd" => Self::ExternalSemaphoreFd,
            "external_fence_fd" => Self::ExternalFenceFd,
            "video_queue" => Self::VideoQueue,
            "video_decode_h264" => Self::VideoDecodeH264,
            "video_decode_h265" => Self::VideoDecodeH265,
            "video_decode_av1" => Self::VideoDecodeAv1,
            "video_encode_h264" => Self::VideoEncodeH264,
            "video_encode_h265" => Self::VideoEncodeH265,
            "video_encode_av1" => Self::VideoEncodeAv1,
            _ => return None,
        })
    }

    /// All known variants in a stable order.
    pub const ALL: &'static [Self] = &[
        Self::RayTracing,
        Self::AccelerationStructure,
        Self::RayQuery,
        Self::RayTracingPositionFetch,
        Self::RayTracingMaintenance1,
        Self::ClusterAccelerationStructure,
        Self::MeshShader,
        Self::TaskShader,
        Self::PipelineFragmentShadingRate,
        Self::PrimitiveFragmentShadingRate,
        Self::AttachmentFragmentShadingRate,
        Self::BindlessResources,
        Self::PushDescriptor,
        Self::DescriptorBuffer,
        Self::DescriptorHeap,
        Self::TimelineSemaphore,
        Self::Synchronization2,
        Self::DynamicRendering,
        Self::BufferDeviceAddress,
        Self::MemoryBudget,
        Self::MemoryPriority,
        Self::PageableDeviceLocalMemory,
        Self::HostImageCopy,
        Self::ConditionalRendering,
        Self::ShaderObject,
        Self::DeviceGeneratedCommandsNv,
        Self::DeviceGeneratedCommands,
        Self::GraphicsPipelineLibrary,
        Self::ExtendedDynamicState3,
        Self::VertexInputDynamicState,
        Self::SamplerAnisotropy,
        Self::SamplerFilterMinmax,
        Self::CustomBorderColor,
        Self::FilterCubic,
        Self::ImageViewMinLod,
        Self::ImageCompressionControl,
        Self::MsaaRenderToSingleSampled,
        Self::ConservativeRasterization,
        Self::GlobalQueuePriority,
        Self::DeviceFault,
        Self::DeviceDiagnosticCheckpointsNv,
        Self::BufferMarkerAmd,
        Self::DeviceAddressBindingReport,
        Self::DeviceMemoryReport,
        Self::PerformanceQuery,
        Self::PipelineExecutableProperties,
        Self::Reflex,
        Self::AntiLag,
        Self::FragmentShaderBarycentric,
        Self::FragmentShaderInterlock,
        Self::ShaderAtomicFloat,
        Self::ComputeShaderDerivatives,
        Self::ShaderClock,
        Self::WorkGraphs,
        Self::CooperativeMatrix,
        Self::OpticalFlowNv,
        Self::ExternalMemoryFd,
        Self::ExternalSemaphoreFd,
        Self::ExternalFenceFd,
        Self::VideoQueue,
        Self::VideoDecodeH264,
        Self::VideoDecodeH265,
        Self::VideoDecodeAv1,
        Self::VideoEncodeH264,
        Self::VideoEncodeH265,
        Self::VideoEncodeAv1,
    ];
}

impl std::fmt::Display for BackendFeature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.feature_name())
    }
}

impl From<BackendFeature> for String {
    fn from(f: BackendFeature) -> Self {
        f.feature_name().to_string()
    }
}

impl std::str::FromStr for BackendFeature {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        BackendFeature::from_feature_name(s)
            .ok_or_else(|| format!("unknown backend feature: {s:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_all_variants() {
        for &feature in BackendFeature::ALL {
            let name = feature.feature_name();
            let parsed = BackendFeature::from_feature_name(name)
                .unwrap_or_else(|| panic!("from_feature_name({name:?}) returned None"));
            assert_eq!(parsed, feature, "round-trip failed for {name:?}");
        }
    }

    #[test]
    fn all_variants_have_unique_names() {
        let mut names = std::collections::HashSet::new();
        for &feature in BackendFeature::ALL {
            assert!(
                names.insert(feature.feature_name()),
                "duplicate feature_name: {:?}",
                feature.feature_name()
            );
        }
    }
}
