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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_features_are_conservative() {
        let features = BackendFeatures::default();

        assert!(!features.mesh_shading);
        assert!(!features.ray_tracing);
        assert!(!features.bindless);
        assert!(!features.hdr_output);
        assert!(!features.shader_fp16);
        assert!(!features.shader_fp64);
        assert!(!features.image_fp16_render);
        assert!(!features.image_fp32_render);
        assert!(!features.dynamic_rendering);
        assert!(!features.timeline_semaphores);
        assert!(!features.multi_draw_indirect);
        assert!(!features.draw_indirect_count);
        assert!(!features.sparse_binding);
        assert!(!features.sparse_residency_buffer);
        assert!(!features.sparse_residency_image_2d);
        assert!(!features.sparse_residency_image_3d);
        assert!(!features.sparse_residency_aliased);
        assert!(!features.descriptor_buffer);
        assert!(!features.descriptor_heap);
        assert!(!features.work_graphs);
        assert!(!features.shader_object);

        // GFX-4: video
        assert!(!features.video_queue);
        assert!(!features.video_decode_h264);
        assert!(!features.video_decode_h265);
        assert!(!features.video_decode_av1);
        assert!(!features.video_decode_vp9);
        assert!(!features.video_encode_h264);
        assert!(!features.video_encode_h265);
        assert!(!features.video_encode_av1);
        assert!(!features.video_encode_quantization_map);

        // GFX-5: external interop
        assert!(!features.external_memory_fd);
        assert!(!features.external_memory_win32);
        assert!(!features.external_memory_dma_buf);
        assert!(!features.external_memory_host);
        assert!(!features.drm_format_modifier);
        assert!(!features.external_semaphore_fd);
        assert!(!features.external_semaphore_win32);
        assert!(!features.external_fence_fd);
        assert!(!features.external_fence_win32);

        // GFX-6a: DGC
        assert!(!features.device_generated_commands);
        assert!(!features.device_generated_commands_nv);

        // GFX-6b: latency
        assert!(!features.reflex);
        assert!(!features.anti_lag);

        // GFX-6c: cooperative matrix
        assert!(!features.cooperative_matrix);
        assert!(!features.cooperative_matrix_nv);
        assert!(!features.cooperative_matrix_nv2);

        // GFX-6d: advanced shader features
        assert!(!features.fragment_shader_barycentric);
        assert!(!features.fragment_shader_interlock);
        assert!(!features.shader_atomic_float);
        assert!(!features.shader_atomic_float16);
        assert!(!features.compute_shader_derivatives);
        assert!(!features.shader_clock);
        assert!(!features.post_depth_coverage);

        // GFX-6e: optical flow
        assert!(!features.optical_flow_nv);

        // GFX-2a: VRS sub-modes
        assert!(!features.vrs_pipeline);
        assert!(!features.vrs_primitive);
        assert!(!features.vrs_attachment);

        // GFX-2f: conservative rasterization modes
        assert!(!features.conservative_rasterization_overestimate);
        assert!(!features.conservative_rasterization_underestimate);

        // GFX-2h: presentation extensions
        assert!(!features.swapchain_colorspace);
        assert!(!features.present_id);
        assert!(!features.present_wait);
        assert!(!features.swapchain_maintenance1);
        assert!(!features.full_screen_exclusive);
        assert!(!features.display_timing);
        assert!(!features.present_mode_fifo_latest_ready);

        // GFX-2j: performance query
        assert!(!features.performance_query);
        assert!(!features.pipeline_executable_properties);
        assert!(!features.graphics_pipeline_library);
        assert!(!features.pipeline_creation_cache_control);

        // GFX-1 remaining
        assert!(!features.synchronization2);
        assert!(!features.descriptor_indexing);
        assert!(!features.conditional_rendering);
        assert!(!features.memory_budget);
        assert!(!features.memory_priority);
        assert!(!features.pageable_device_local_memory);
        assert!(!features.push_descriptors);
        assert!(!features.device_fault);
        assert!(!features.device_memory_report);
        assert!(!features.device_address_binding_report);
        assert!(!features.buffer_marker_amd);
        assert!(!features.device_diagnostic_checkpoints_nv);
        assert!(!features.host_image_copy);
        assert!(!features.buffer_device_address);

        // GFX-2 remaining
        assert!(!features.variable_rate_shading);
        assert!(!features.extended_dynamic_state3);
        assert!(!features.extended_dynamic_state3_polygon_mode);
        assert!(!features.extended_dynamic_state3_color_blend);
        assert!(!features.vertex_input_dynamic_state);
        assert!(!features.global_queue_priority);
        assert!(!features.sampler_filter_minmax);
        assert!(!features.custom_border_color);
        assert!(!features.filter_cubic);
        assert!(!features.image_view_min_lod);
        assert!(!features.image_compression_control);
        assert!(!features.msaa_render_to_single_sampled);

        // GFX-3 remaining
        assert!(!features.ray_query);
        assert!(!features.ray_tracing_position_fetch);
        assert!(!features.ray_tracing_maintenance1);
        assert!(!features.opacity_micromap);
        assert!(!features.shader_execution_reordering);
        assert!(!features.cluster_acceleration_structure);
    }

    #[test]
    fn sparse_texture_ready_requires_binding_and_image_residency() {
        let features = BackendFeatures {
            sparse_binding: true,
            sparse_residency_image_2d: true,
            ..BackendFeatures::default()
        };

        assert!(features.supports_sparse_2d_textures());
        assert!(!features.supports_sparse_buffers());
    }

    #[test]
    fn gpu_draw_compaction_requires_multi_draw_and_indirect_count() {
        let features = BackendFeatures {
            multi_draw_indirect: true,
            draw_indirect_count: true,
            ..BackendFeatures::default()
        };

        assert!(features.supports_gpu_draw_compaction());
    }

    #[test]
    fn conservative_rasterization_supports_either_mode() {
        let overestimate = BackendFeatures {
            conservative_rasterization_overestimate: true,
            ..BackendFeatures::default()
        };
        let underestimate = BackendFeatures {
            conservative_rasterization_underestimate: true,
            ..BackendFeatures::default()
        };

        assert!(overestimate.supports_conservative_rasterization());
        assert!(underestimate.supports_conservative_rasterization());
        assert!(!BackendFeatures::default().supports_conservative_rasterization());
    }
}

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
}
