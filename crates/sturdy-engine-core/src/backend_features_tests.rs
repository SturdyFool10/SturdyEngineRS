// Tests extracted from crates/sturdy-engine-core/src/backend_features.rs
// See scripts/extract_tests.py for the extraction logic.

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
fn disabling_video_features_clears_codec_and_queue_flags() {
    let mut features = BackendFeatures {
        video_queue: true,
        video_decode_h264: true,
        video_decode_h265: true,
        video_decode_av1: true,
        video_decode_vp9: true,
        video_encode_h264: true,
        video_encode_h265: true,
        video_encode_av1: true,
        video_encode_quantization_map: true,
        ..BackendFeatures::default()
    };

    features.disable_video_features();

    assert!(!features.video_queue);
    assert!(!features.video_decode_h264);
    assert!(!features.video_decode_h265);
    assert!(!features.video_decode_av1);
    assert!(!features.video_decode_vp9);
    assert!(!features.video_encode_h264);
    assert!(!features.video_encode_h265);
    assert!(!features.video_encode_av1);
    assert!(!features.video_encode_quantization_map);
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
fn disabling_device_generated_command_features_clears_extension_flags() {
    let mut features = BackendFeatures {
        device_generated_commands: true,
        device_generated_commands_nv: true,
        ..BackendFeatures::default()
    };

    features.disable_device_generated_command_features();

    assert!(!features.device_generated_commands);
    assert!(!features.device_generated_commands_nv);
}

#[test]
fn disabling_optical_flow_features_clears_extension_flags() {
    let mut features = BackendFeatures {
        optical_flow_nv: true,
        ..BackendFeatures::default()
    };

    features.disable_optical_flow_features();

    assert!(!features.optical_flow_nv);
}

#[test]
fn disabling_anti_lag_features_clears_extension_flags() {
    let mut features = BackendFeatures {
        anti_lag: true,
        ..BackendFeatures::default()
    };

    features.disable_anti_lag_features();

    assert!(!features.anti_lag);
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
