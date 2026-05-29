use std::ffi::CStr;

use ash::vk::TaggedStructure;
use ash::{Instance, vk};

use crate::{BackendFeatures, Caps, Format, FormatCapabilities, Limits};

pub fn query_caps(instance: &Instance, physical_device: vk::PhysicalDevice) -> Caps {
    let properties = unsafe { instance.get_physical_device_properties(physical_device) };
    let lim = &properties.limits;

    let max_dimension = lim.max_image_dimension2_d.max(1);
    let max_mip_levels = u32::BITS - max_dimension.leading_zeros();

    let extensions = available_device_extensions(instance, physical_device);
    let raw_extension_names = extensions
        .iter()
        .map(|ext| unsafe { CStr::from_ptr(ext.extension_name.as_ptr()) })
        .map(|name| name.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let has = |name: &[u8]| {
        let wanted = unsafe { CStr::from_bytes_with_nul_unchecked(name) };
        extensions
            .iter()
            .any(|ext| unsafe { CStr::from_ptr(ext.extension_name.as_ptr()) } == wanted)
    };

    let ray_tracing =
        has(b"VK_KHR_ray_tracing_pipeline\0") && has(b"VK_KHR_acceleration_structure\0");
    // GFX-3c: ray query extension detection only.
    let ray_query = has(b"VK_KHR_ray_query\0");
    // GFX-3c: RT enhancement detection (extension presence only, no device feature enabling).
    let ray_tracing_position_fetch_ext = has(b"VK_KHR_ray_tracing_position_fetch\0");
    let ray_tracing_maintenance1 = has(b"VK_KHR_ray_tracing_maintenance1\0");
    let opacity_micromap = has(b"VK_EXT_opacity_micromap\0");
    let shader_execution_reordering = has(b"VK_EXT_ray_tracing_invocation_reorder\0");
    let cluster_acceleration_structure = has(b"VK_NV_cluster_acceleration_structure\0");
    let descriptor_indexing = has(b"VK_EXT_descriptor_indexing\0");
    let dynamic_rendering = has(b"VK_KHR_dynamic_rendering\0");
    let synchronization2 = has(b"VK_KHR_synchronization2\0");
    // Timeline semaphores are core in Vulkan 1.2; also available as extension.
    let timeline_semaphores =
        properties.api_version >= vk::API_VERSION_1_2 || has(b"VK_KHR_timeline_semaphore\0");
    let hdr_output = has(b"VK_EXT_hdr_metadata\0") || has(b"VK_AMD_display_native_hdr\0");
    // Replaced by sub-mode detection below (after feature_chain is available).
    let draw_indirect_count =
        properties.api_version >= vk::API_VERSION_1_2 || has(b"VK_KHR_draw_indirect_count\0");
    let buffer_device_address =
        properties.api_version >= vk::API_VERSION_1_2 || has(b"VK_KHR_buffer_device_address\0");
    let memory_budget = has(b"VK_EXT_memory_budget\0");
    let memory_priority = has(b"VK_EXT_memory_priority\0");
    let pageable_device_local_memory = has(b"VK_EXT_pageable_device_local_memory\0");
    let device_fault = has(b"VK_EXT_device_fault\0");
    let device_diagnostic_checkpoints_nv = has(b"VK_NV_device_diagnostic_checkpoints\0");
    let buffer_marker_amd = has(b"VK_AMD_buffer_marker\0");
    let device_address_binding_report = has(b"VK_EXT_device_address_binding_report\0");
    let device_memory_report = has(b"VK_EXT_device_memory_report\0");
    // VK_EXT_host_image_copy is core in Vulkan 1.4 (version 0x00401000).
    let vk_1_4 = vk::make_api_version(0, 1, 4, 0);
    let host_image_copy = has(b"VK_EXT_host_image_copy\0") || properties.api_version >= vk_1_4;
    // GFX-1f: VK_KHR_push_descriptor — no feature struct, purely extension presence.
    let push_descriptors = has(b"VK_KHR_push_descriptor\0");
    // GFX-2g: VK_KHR_global_priority — no feature struct, purely extension presence.
    let global_queue_priority = has(b"VK_KHR_global_priority\0");
    // GFX-2f: VK_EXT_conservative_rasterization — overestimation is part of
    // the extension; underestimation is reported in the extension properties.
    let conservative_rasterization_ext = has(b"VK_EXT_conservative_rasterization\0");
    // GFX-2k: VK_EXT_filter_cubic — no feature struct.
    let filter_cubic = has(b"VK_EXT_filter_cubic\0");
    // GFX-2j: shader core count — vendor extensions only, no feature struct.
    let amd_shader_core_properties = has(b"VK_AMD_shader_core_properties\0");
    let nv_shader_sm_builtins = has(b"VK_NV_shader_sm_builtins\0");

    let core_features = unsafe { instance.get_physical_device_features(physical_device) };
    let feature_chain = available_feature_chain(instance, physical_device);
    let conservative_rasterization_properties = conservative_rasterization_ext
        .then(|| conservative_rasterization_properties(instance, physical_device));
    // GFX-2a: VRS sub-mode detection.
    let vrs_pipeline = has(b"VK_KHR_fragment_shading_rate\0")
        && feature_chain
            .fragment_shading_rate
            .pipeline_fragment_shading_rate
            == vk::TRUE;
    let vrs_primitive = has(b"VK_KHR_fragment_shading_rate\0")
        && feature_chain
            .fragment_shading_rate
            .primitive_fragment_shading_rate
            == vk::TRUE;
    let vrs_attachment = has(b"VK_KHR_fragment_shading_rate\0")
        && feature_chain
            .fragment_shading_rate
            .attachment_fragment_shading_rate
            == vk::TRUE;
    let variable_rate_shading = vrs_pipeline || vrs_primitive || vrs_attachment;

    let ray_tracing_position_fetch = ray_tracing_position_fetch_ext
        && feature_chain
            .ray_tracing_position_fetch
            .ray_tracing_position_fetch
            == vk::TRUE;
    let mesh_shading =
        has(b"VK_EXT_mesh_shader\0") && feature_chain.mesh_shader.mesh_shader == vk::TRUE;
    let bindless = descriptor_indexing
        && feature_chain.descriptor_indexing.runtime_descriptor_array == vk::TRUE
        && feature_chain
            .descriptor_indexing
            .descriptor_binding_partially_bound
            == vk::TRUE
        && feature_chain
            .descriptor_indexing
            .descriptor_binding_sampled_image_update_after_bind
            == vk::TRUE
        && feature_chain
            .descriptor_indexing
            .descriptor_binding_storage_image_update_after_bind
            == vk::TRUE
        && feature_chain
            .descriptor_indexing
            .descriptor_binding_storage_buffer_update_after_bind
            == vk::TRUE;
    let sampler_anisotropy = core_features.sampler_anisotropy == vk::TRUE;
    let shader_fp16 = feature_chain.shader_float16_int8.shader_float16 == vk::TRUE;
    let shader_fp64 = core_features.shader_float64 == vk::TRUE;
    let multi_draw_indirect = core_features.multi_draw_indirect == vk::TRUE;
    let sparse_binding = core_features.sparse_binding == vk::TRUE;
    let sparse_residency_buffer = core_features.sparse_residency_buffer == vk::TRUE;
    let sparse_residency_image_2d = core_features.sparse_residency_image2_d == vk::TRUE;
    let sparse_residency_image_3d = core_features.sparse_residency_image3_d == vk::TRUE;
    let sparse_residency_aliased = core_features.sparse_residency_aliased == vk::TRUE;
    let image_fp16_render = format_supports_color_attachment(
        instance,
        physical_device,
        vk::Format::R16G16B16A16_SFLOAT,
    );
    let image_fp32_render = format_supports_color_attachment(
        instance,
        physical_device,
        vk::Format::R32G32B32A32_SFLOAT,
    );
    let max_color_sample_count = max_sample_count(lim.framebuffer_color_sample_counts);

    // GFX-2b: VK_EXT_conditional_rendering
    let conditional_rendering = has(b"VK_EXT_conditional_rendering\0")
        && feature_chain.conditional_rendering.conditional_rendering == vk::TRUE;

    // GFX-2d: VK_EXT_extended_dynamic_state3
    let extended_dynamic_state3 = has(b"VK_EXT_extended_dynamic_state3\0")
        && (feature_chain
            .extended_dynamic_state3
            .extended_dynamic_state3_polygon_mode
            == vk::TRUE
            || feature_chain
                .extended_dynamic_state3
                .extended_dynamic_state3_rasterization_samples
                == vk::TRUE
            || feature_chain
                .extended_dynamic_state3
                .extended_dynamic_state3_color_blend_enable
                == vk::TRUE);
    let extended_dynamic_state3_polygon_mode = has(b"VK_EXT_extended_dynamic_state3\0")
        && feature_chain
            .extended_dynamic_state3
            .extended_dynamic_state3_polygon_mode
            == vk::TRUE;
    let extended_dynamic_state3_color_blend = has(b"VK_EXT_extended_dynamic_state3\0")
        && feature_chain
            .extended_dynamic_state3
            .extended_dynamic_state3_color_blend_enable
            == vk::TRUE;

    // GFX-2e: VK_EXT_vertex_input_dynamic_state
    let vertex_input_dynamic_state = has(b"VK_EXT_vertex_input_dynamic_state\0")
        && feature_chain
            .vertex_input_dynamic_state
            .vertex_input_dynamic_state
            == vk::TRUE;

    // GFX-7a: VK_EXT_descriptor_buffer — descriptors stored in app-managed GPU buffers.
    let descriptor_buffer = has(b"VK_EXT_descriptor_buffer\0");
    // GFX-7b: VK_EXT_descriptor_heap — D3D12-style resource + sampler descriptor heaps.
    // Gated on both extension presence and the feature bit (extension has a mandatory feature struct).
    let descriptor_heap = has(b"VK_EXT_descriptor_heap\0")
        && feature_chain.descriptor_heap.descriptor_heap == vk::TRUE;
    // GFX-7c: VK_AMDX_shader_enqueue — AMD work graphs (shader-enqueued dispatch).
    let work_graphs = has(b"VK_AMDX_shader_enqueue\0");
    // GFX-8: VK_EXT_shader_object — pipeline-free shader binding.
    let shader_object = has(b"VK_EXT_shader_object\0");

    // GFX-2k: sampler/image quality extensions
    let vk_1_2 = vk::make_api_version(0, 1, 2, 0);
    let vk_1_3 = vk::make_api_version(0, 1, 3, 0);
    let sampler_filter_minmax =
        properties.api_version >= vk_1_2 || has(b"VK_EXT_sampler_filter_minmax\0");
    let custom_border_color = has(b"VK_EXT_custom_border_color\0")
        && feature_chain.custom_border_color.custom_border_colors == vk::TRUE;
    let image_view_min_lod =
        has(b"VK_EXT_image_view_min_lod\0") && feature_chain.image_view_min_lod.min_lod == vk::TRUE;
    let image_compression_control = has(b"VK_EXT_image_compression_control\0")
        && feature_chain
            .image_compression_control
            .image_compression_control
            == vk::TRUE;
    let msaa_render_to_single_sampled = has(b"VK_EXT_multisampled_render_to_single_sampled\0")
        && feature_chain
            .msaa_render_to_single_sampled
            .multisampled_render_to_single_sampled
            == vk::TRUE;

    // GFX-4: Video encode/decode
    let video_queue = has(b"VK_KHR_video_queue\0");
    let video_decode_h264 = video_queue && has(b"VK_KHR_video_decode_h264\0");
    let video_decode_h265 = video_queue && has(b"VK_KHR_video_decode_h265\0");
    let video_decode_av1 = video_queue && has(b"VK_KHR_video_decode_av1\0");
    let video_decode_vp9 = video_queue && has(b"VK_KHR_video_decode_vp9\0");
    let video_encode_h264 = video_queue && has(b"VK_KHR_video_encode_h264\0");
    let video_encode_h265 = video_queue && has(b"VK_KHR_video_encode_h265\0");
    let video_encode_av1 = video_queue && has(b"VK_KHR_video_encode_av1\0");
    let video_encode_quantization_map =
        video_queue && has(b"VK_KHR_video_encode_quantization_map\0");
    // GFX-4: video maintenance extensions (simplify session parameter management).
    let video_maintenance1 = video_queue && has(b"VK_KHR_video_maintenance1\0");
    let video_maintenance2 = video_queue && has(b"VK_KHR_video_maintenance2\0");

    // GFX-5: External resource interop
    let external_memory_fd = has(b"VK_KHR_external_memory_fd\0");
    let external_memory_win32 = has(b"VK_KHR_external_memory_win32\0");
    let external_memory_dma_buf = has(b"VK_EXT_external_memory_dma_buf\0");
    let external_memory_host = has(b"VK_EXT_external_memory_host\0");
    let drm_format_modifier = has(b"VK_EXT_image_drm_format_modifier\0");
    let external_semaphore_fd = has(b"VK_KHR_external_semaphore_fd\0");
    let external_semaphore_win32 = has(b"VK_KHR_external_semaphore_win32\0");
    let external_fence_fd = has(b"VK_KHR_external_fence_fd\0");
    let external_fence_win32 = has(b"VK_KHR_external_fence_win32\0");

    // GFX-6a: Device-generated commands
    let device_generated_commands = has(b"VK_EXT_device_generated_commands\0");
    let device_generated_commands_nv = has(b"VK_NV_device_generated_commands\0");

    // GFX-6b: Latency reduction
    let reflex = has(b"VK_NV_low_latency2\0");
    let anti_lag = has(b"VK_AMD_anti_lag\0");

    // GFX-6c: Cooperative matrix
    let cooperative_matrix = has(b"VK_KHR_cooperative_matrix\0");
    let cooperative_matrix_nv = has(b"VK_NV_cooperative_matrix\0");
    let cooperative_matrix_nv2 = has(b"VK_NV_cooperative_matrix2\0");

    // GFX-6d: Advanced shader features
    let fragment_shader_barycentric = has(b"VK_KHR_fragment_shader_barycentric\0");
    let fragment_shader_interlock = has(b"VK_EXT_fragment_shader_interlock\0");
    let shader_atomic_float = has(b"VK_EXT_shader_atomic_float\0");
    let shader_atomic_float16 = has(b"VK_EXT_shader_atomic_float2\0");
    let compute_shader_derivatives = has(b"VK_KHR_compute_shader_derivatives\0");
    let shader_clock = has(b"VK_KHR_shader_clock\0");
    let post_depth_coverage = has(b"VK_EXT_post_depth_coverage\0");

    // GFX-6e: Optical flow
    let optical_flow_nv = has(b"VK_NV_optical_flow\0");

    // GFX-2h: Presentation extensions
    let swapchain_colorspace = has(b"VK_EXT_swapchain_colorspace\0");
    let present_id = has(b"VK_KHR_present_id\0");
    let present_wait = has(b"VK_KHR_present_wait\0");
    let swapchain_maintenance1 = has(b"VK_KHR_swapchain_maintenance1\0");
    let full_screen_exclusive = has(b"VK_EXT_full_screen_exclusive\0");
    let display_timing = has(b"VK_GOOGLE_display_timing\0");
    let present_mode_fifo_latest_ready = has(b"VK_EXT_present_mode_fifo_latest_ready\0")
        || has(b"VK_KHR_present_mode_fifo_latest_ready\0");

    // GFX-2j: Performance query
    let performance_query = has(b"VK_KHR_performance_query\0");
    let pipeline_executable_properties = has(b"VK_KHR_pipeline_executable_properties\0");
    let shader_info_amd = has(b"VK_AMD_shader_info\0");
    let graphics_pipeline_library = has(b"VK_EXT_graphics_pipeline_library\0")
        && feature_chain
            .graphics_pipeline_library
            .graphics_pipeline_library
            == vk::TRUE;
    let pipeline_creation_cache_control =
        properties.api_version >= vk_1_3 || has(b"VK_EXT_pipeline_creation_cache_control\0");
    // GFX-2k: image compression swapchain
    let image_compression_control_swapchain = has(b"VK_EXT_image_compression_control_swapchain\0");
    // GFX-5: external memory export capability
    let external_memory_fd_export = has(b"VK_KHR_external_memory_fd\0");

    let features = BackendFeatures {
        ray_tracing,
        ray_query,
        ray_tracing_position_fetch,
        ray_tracing_maintenance1,
        opacity_micromap,
        shader_execution_reordering,
        cluster_acceleration_structure,
        mesh_shading,
        bindless,
        descriptor_indexing,
        timeline_semaphores,
        dynamic_rendering,
        synchronization2,
        buffer_device_address,
        hdr_output,
        shader_fp16,
        shader_fp64,
        image_fp16_render,
        image_fp32_render,
        variable_rate_shading,
        vrs_pipeline,
        vrs_primitive,
        vrs_attachment,
        multi_draw_indirect,
        draw_indirect_count,
        sparse_binding,
        sparse_residency_buffer,
        sparse_residency_image_2d,
        sparse_residency_image_3d,
        sparse_residency_aliased,
        memory_budget,
        memory_priority,
        pageable_device_local_memory,
        device_fault,
        device_diagnostic_checkpoints_nv,
        buffer_marker_amd,
        device_address_binding_report,
        device_memory_report,
        host_image_copy,
        sampler_anisotropy,
        push_descriptors,
        conditional_rendering,
        extended_dynamic_state3,
        extended_dynamic_state3_polygon_mode,
        extended_dynamic_state3_color_blend,
        vertex_input_dynamic_state,
        conservative_rasterization_overestimate: conservative_rasterization_ext,
        conservative_rasterization_underestimate: conservative_rasterization_properties
            .is_some_and(|properties| properties.primitive_underestimation == vk::TRUE),
        global_queue_priority,
        sampler_filter_minmax,
        custom_border_color,
        filter_cubic,
        image_view_min_lod,
        image_compression_control,
        msaa_render_to_single_sampled,
        descriptor_buffer,
        descriptor_heap,
        work_graphs,
        shader_object,
        // GFX-4: video
        video_queue,
        video_maintenance1,
        video_maintenance2,
        video_decode_h264,
        video_decode_h265,
        video_decode_av1,
        video_decode_vp9,
        video_encode_h264,
        video_encode_h265,
        video_encode_av1,
        video_encode_quantization_map,
        // GFX-5: external interop
        external_memory_fd,
        external_memory_win32,
        external_memory_dma_buf,
        external_memory_host,
        drm_format_modifier,
        external_semaphore_fd,
        external_semaphore_win32,
        external_fence_fd,
        external_fence_win32,
        // GFX-6a: DGC
        device_generated_commands,
        device_generated_commands_nv,
        // GFX-6b: latency
        reflex,
        anti_lag,
        // GFX-6c: cooperative matrix
        cooperative_matrix,
        cooperative_matrix_nv,
        cooperative_matrix_nv2,
        // GFX-6d: advanced shader features
        fragment_shader_barycentric,
        fragment_shader_interlock,
        shader_atomic_float,
        shader_atomic_float16,
        compute_shader_derivatives,
        shader_clock,
        post_depth_coverage,
        // GFX-6e: optical flow
        optical_flow_nv,
        // GFX-2h: presentation extensions
        swapchain_colorspace,
        present_id,
        present_wait,
        swapchain_maintenance1,
        full_screen_exclusive,
        display_timing,
        present_mode_fifo_latest_ready,
        // GFX-2j: performance query
        performance_query,
        pipeline_executable_properties,
        graphics_pipeline_library,
        pipeline_creation_cache_control,
        shader_info_amd,
        image_compression_control_swapchain,
        external_memory_fd_export,
    };

    let limits = Limits {
        max_image_dimension_2d: lim.max_image_dimension2_d,
        max_image_dimension_3d: lim.max_image_dimension3_d,
        max_texture_2d_size: lim.max_image_dimension2_d,
        max_texture_array_layers: lim.max_image_array_layers,
        max_mip_levels,
        max_push_constants_size: lim.max_push_constants_size,
        max_bound_descriptor_sets: lim.max_bound_descriptor_sets,
        max_per_stage_samplers: lim.max_per_stage_descriptor_samplers,
        max_per_stage_sampled_images: lim.max_per_stage_descriptor_sampled_images,
        max_per_stage_storage_images: lim.max_per_stage_descriptor_storage_images,
        max_per_stage_uniform_buffers: lim.max_per_stage_descriptor_uniform_buffers,
        max_per_stage_storage_buffers: lim.max_per_stage_descriptor_storage_buffers,
        max_descriptor_set_samplers: lim.max_descriptor_set_samplers,
        max_descriptor_set_sampled_images: lim.max_descriptor_set_sampled_images,
        max_descriptor_set_storage_images: lim.max_descriptor_set_storage_images,
        max_descriptor_set_uniform_buffers: lim.max_descriptor_set_uniform_buffers,
        max_descriptor_set_storage_buffers: lim.max_descriptor_set_storage_buffers,
        max_color_attachments: lim.max_color_attachments,
        max_compute_workgroup_size: lim.max_compute_work_group_size,
        max_compute_invocations: lim.max_compute_work_group_invocations,
        max_frames_in_flight: 2,
    };

    let shader_core_count = query_shader_core_count(
        instance,
        physical_device,
        amd_shader_core_properties,
        nv_shader_sm_builtins,
    );

    Caps {
        supports_raytracing: ray_tracing,
        supports_mesh_shading: mesh_shading,
        supports_bindless: bindless,
        max_color_sample_count,
        max_mip_levels,
        max_frames_in_flight: 2,
        features,
        limits,
        raw_extension_names,
        raw_feature_names: available_feature_names(instance, physical_device),
        shader_core_count,
    }
}

fn max_sample_count(flags: vk::SampleCountFlags) -> u8 {
    if flags.contains(vk::SampleCountFlags::TYPE_64) {
        64
    } else if flags.contains(vk::SampleCountFlags::TYPE_32) {
        32
    } else if flags.contains(vk::SampleCountFlags::TYPE_16) {
        16
    } else if flags.contains(vk::SampleCountFlags::TYPE_8) {
        8
    } else if flags.contains(vk::SampleCountFlags::TYPE_4) {
        4
    } else if flags.contains(vk::SampleCountFlags::TYPE_2) {
        2
    } else {
        1
    }
}

pub fn available_device_extension_names(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
) -> Vec<String> {
    available_device_extensions(instance, physical_device)
        .iter()
        .map(|ext| unsafe { CStr::from_ptr(ext.extension_name.as_ptr()) })
        .map(|name| name.to_string_lossy().into_owned())
        .collect()
}

fn format_supports_color_attachment(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
    format: vk::Format,
) -> bool {
    let props = unsafe { instance.get_physical_device_format_properties(physical_device, format) };
    props
        .optimal_tiling_features
        .contains(vk::FormatFeatureFlags::COLOR_ATTACHMENT)
}

pub fn query_format_capabilities(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
    format: Format,
) -> FormatCapabilities {
    let Ok(vk_format) = vk_format(format) else {
        return FormatCapabilities::default();
    };
    let props =
        unsafe { instance.get_physical_device_format_properties(physical_device, vk_format) };
    let optimal = props.optimal_tiling_features;

    FormatCapabilities {
        sampled: optimal.contains(vk::FormatFeatureFlags::SAMPLED_IMAGE),
        storage: optimal.contains(vk::FormatFeatureFlags::STORAGE_IMAGE),
        color_attachment: optimal.contains(vk::FormatFeatureFlags::COLOR_ATTACHMENT),
        depth_stencil_attachment: optimal
            .contains(vk::FormatFeatureFlags::DEPTH_STENCIL_ATTACHMENT),
        copy_src: optimal.contains(vk::FormatFeatureFlags::TRANSFER_SRC),
        copy_dst: optimal.contains(vk::FormatFeatureFlags::TRANSFER_DST),
        linear_filter: optimal.contains(vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR),
    }
}

fn vk_format(format: Format) -> Result<vk::Format, ()> {
    match format {
        Format::Unknown => Err(()),
        Format::Rgba8Unorm => Ok(vk::Format::R8G8B8A8_UNORM),
        Format::Bgra8Unorm => Ok(vk::Format::B8G8R8A8_UNORM),
        Format::Rgba16Float => Ok(vk::Format::R16G16B16A16_SFLOAT),
        Format::Rgba32Float => Ok(vk::Format::R32G32B32A32_SFLOAT),
        Format::R8Unorm => Ok(vk::Format::R8_UNORM),
        Format::Rg8Unorm => Ok(vk::Format::R8G8_UNORM),
        Format::Bc3Unorm => Ok(vk::Format::BC3_UNORM_BLOCK),
        Format::Bc3UnormSrgb => Ok(vk::Format::BC3_SRGB_BLOCK),
        Format::Bc4Unorm => Ok(vk::Format::BC4_UNORM_BLOCK),
        Format::Bc5Unorm => Ok(vk::Format::BC5_UNORM_BLOCK),
        Format::Bc7Unorm => Ok(vk::Format::BC7_UNORM_BLOCK),
        Format::Bc7UnormSrgb => Ok(vk::Format::BC7_SRGB_BLOCK),
        Format::Bc6hUfloat => Ok(vk::Format::BC6H_UFLOAT_BLOCK),
        Format::G8_B8R8_2PLANE_420_UNORM => Ok(vk::Format::G8_B8R8_2PLANE_420_UNORM),
        Format::Depth32Float => Ok(vk::Format::D32_SFLOAT),
        Format::Depth24Stencil8 => Ok(vk::Format::D24_UNORM_S8_UINT),
    }
}

pub fn available_core_feature_names(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
) -> Vec<String> {
    let features = unsafe { instance.get_physical_device_features(physical_device) };
    let mut names = Vec::new();
    macro_rules! push_feature {
        ($field:ident) => {
            if features.$field == vk::TRUE {
                names.push(stringify!($field).to_string());
            }
        };
    }

    push_feature!(robust_buffer_access);
    push_feature!(full_draw_index_uint32);
    push_feature!(image_cube_array);
    push_feature!(independent_blend);
    push_feature!(geometry_shader);
    push_feature!(tessellation_shader);
    push_feature!(sample_rate_shading);
    push_feature!(dual_src_blend);
    push_feature!(logic_op);
    push_feature!(multi_draw_indirect);
    push_feature!(draw_indirect_first_instance);
    push_feature!(depth_clamp);
    push_feature!(depth_bias_clamp);
    push_feature!(fill_mode_non_solid);
    push_feature!(depth_bounds);
    push_feature!(wide_lines);
    push_feature!(large_points);
    push_feature!(alpha_to_one);
    push_feature!(multi_viewport);
    push_feature!(sampler_anisotropy);
    push_feature!(texture_compression_etc2);
    push_feature!(texture_compression_astc_ldr);
    push_feature!(texture_compression_bc);
    push_feature!(occlusion_query_precise);
    push_feature!(pipeline_statistics_query);
    push_feature!(vertex_pipeline_stores_and_atomics);
    push_feature!(fragment_stores_and_atomics);
    push_feature!(shader_tessellation_and_geometry_point_size);
    push_feature!(shader_image_gather_extended);
    push_feature!(shader_storage_image_extended_formats);
    push_feature!(shader_storage_image_multisample);
    push_feature!(shader_storage_image_read_without_format);
    push_feature!(shader_storage_image_write_without_format);
    push_feature!(shader_uniform_buffer_array_dynamic_indexing);
    push_feature!(shader_sampled_image_array_dynamic_indexing);
    push_feature!(shader_storage_buffer_array_dynamic_indexing);
    push_feature!(shader_storage_image_array_dynamic_indexing);
    push_feature!(shader_clip_distance);
    push_feature!(shader_cull_distance);
    push_feature!(shader_float64);
    push_feature!(shader_int64);
    push_feature!(shader_int16);
    push_feature!(shader_resource_residency);
    push_feature!(shader_resource_min_lod);
    push_feature!(sparse_binding);
    push_feature!(sparse_residency_buffer);
    push_feature!(sparse_residency_image2_d);
    push_feature!(sparse_residency_image3_d);
    push_feature!(sparse_residency2_samples);
    push_feature!(sparse_residency4_samples);
    push_feature!(sparse_residency8_samples);
    push_feature!(sparse_residency16_samples);
    push_feature!(sparse_residency_aliased);
    push_feature!(variable_multisample_rate);
    push_feature!(inherited_queries);

    names
}

pub fn available_feature_names(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
) -> Vec<String> {
    let mut names = available_core_feature_names(instance, physical_device);
    let chain = available_feature_chain(instance, physical_device);
    let extensions = available_device_extension_names(instance, physical_device)
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    let has_ext = |name: &str| extensions.contains(name);

    if chain.vulkan11.shader_draw_parameters == vk::TRUE {
        names.push("shader_draw_parameters".into());
    }

    if chain.graphics_pipeline_library.graphics_pipeline_library == vk::TRUE
        && has_ext("VK_EXT_graphics_pipeline_library")
    {
        names.push("graphics_pipeline_library".into());
    }

    if chain.timeline.timeline_semaphore == vk::TRUE {
        names.push("timeline_semaphore".into());
        names.push("timeline_semaphores".into());
    }
    if chain.dynamic_rendering.dynamic_rendering == vk::TRUE {
        names.push("dynamic_rendering".into());
    }
    if chain.synchronization2.synchronization2 == vk::TRUE {
        names.push("synchronization2".into());
    }
    if chain.buffer_device_address.buffer_device_address == vk::TRUE {
        names.push("buffer_device_address".into());
    }
    if chain.mesh_shader.mesh_shader == vk::TRUE {
        names.push("mesh_shader".into());
        names.push("mesh_shading".into());
    }
    if chain.mesh_shader.task_shader == vk::TRUE {
        names.push("task_shader".into());
    }
    if chain.ray_tracing.ray_tracing_pipeline == vk::TRUE
        && chain.acceleration_structure.acceleration_structure == vk::TRUE
    {
        names.push("ray_tracing".into());
    }
    if chain.ray_tracing.ray_tracing_pipeline == vk::TRUE {
        names.push("ray_tracing_pipeline".into());
    }
    if chain.acceleration_structure.acceleration_structure == vk::TRUE {
        names.push("acceleration_structure".into());
    }
    if chain.fragment_shading_rate.pipeline_fragment_shading_rate == vk::TRUE {
        names.push("pipeline_fragment_shading_rate".into());
        names.push("variable_rate_shading".into());
    }
    if chain.fragment_shading_rate.primitive_fragment_shading_rate == vk::TRUE {
        names.push("primitive_fragment_shading_rate".into());
    }
    if chain.fragment_shading_rate.attachment_fragment_shading_rate == vk::TRUE {
        names.push("attachment_fragment_shading_rate".into());
    }
    if chain.memory_priority.memory_priority == vk::TRUE {
        names.push("memory_priority".into());
    }
    if chain.conditional_rendering.conditional_rendering == vk::TRUE {
        names.push("conditional_rendering".into());
    }
    if chain
        .extended_dynamic_state3
        .extended_dynamic_state3_polygon_mode
        == vk::TRUE
        || chain
            .extended_dynamic_state3
            .extended_dynamic_state3_color_blend_enable
            == vk::TRUE
    {
        names.push("extended_dynamic_state3".into());
    }
    if chain.vertex_input_dynamic_state.vertex_input_dynamic_state == vk::TRUE {
        names.push("vertex_input_dynamic_state".into());
    }
    if chain.custom_border_color.custom_border_colors == vk::TRUE {
        names.push("custom_border_color".into());
    }
    if chain.image_view_min_lod.min_lod == vk::TRUE {
        names.push("image_view_min_lod".into());
    }
    if chain.image_compression_control.image_compression_control == vk::TRUE {
        names.push("image_compression_control".into());
    }
    if chain
        .msaa_render_to_single_sampled
        .multisampled_render_to_single_sampled
        == vk::TRUE
    {
        names.push("msaa_render_to_single_sampled".into());
    }
    if has_ext("VK_KHR_push_descriptor") {
        names.push("push_descriptor".into());
        names.push("push_descriptors".into());
    }
    if has_ext("VK_KHR_global_priority") {
        names.push("global_queue_priority".into());
    }
    if has_ext("VK_EXT_conservative_rasterization") {
        names.push("conservative_rasterization".into());
    }
    if has_ext("VK_NV_device_diagnostic_checkpoints") {
        names.push("device_diagnostic_checkpoints_nv".into());
    }
    if has_ext("VK_AMD_buffer_marker") {
        names.push("buffer_marker_amd".into());
    }
    if has_ext("VK_EXT_device_address_binding_report") {
        names.push("device_address_binding_report".into());
    }
    if has_ext("VK_EXT_device_memory_report") {
        names.push("device_memory_report".into());
    }
    if has_ext("VK_EXT_sampler_filter_minmax") {
        names.push("sampler_filter_minmax".into());
    }
    if has_ext("VK_EXT_filter_cubic") {
        names.push("filter_cubic".into());
    }

    push_descriptor_indexing_feature_names(&mut names, &chain.descriptor_indexing);
    names.sort();
    names.dedup();
    names
}

#[derive(Clone, Copy)]
pub struct AvailableFeatureChain<'a> {
    pub vulkan11: vk::PhysicalDeviceVulkan11Features<'a>,
    pub descriptor_indexing: vk::PhysicalDeviceDescriptorIndexingFeatures<'a>,
    pub timeline: vk::PhysicalDeviceTimelineSemaphoreFeatures<'a>,
    pub dynamic_rendering: vk::PhysicalDeviceDynamicRenderingFeatures<'a>,
    pub synchronization2: vk::PhysicalDeviceSynchronization2Features<'a>,
    pub buffer_device_address: vk::PhysicalDeviceBufferDeviceAddressFeatures<'a>,
    pub shader_float16_int8: vk::PhysicalDeviceShaderFloat16Int8Features<'a>,
    pub mesh_shader: vk::PhysicalDeviceMeshShaderFeaturesEXT<'a>,
    pub acceleration_structure: vk::PhysicalDeviceAccelerationStructureFeaturesKHR<'a>,
    pub ray_tracing: vk::PhysicalDeviceRayTracingPipelineFeaturesKHR<'a>,
    pub ray_query: vk::PhysicalDeviceRayQueryFeaturesKHR<'a>,
    pub ray_tracing_position_fetch: vk::PhysicalDeviceRayTracingPositionFetchFeaturesKHR<'a>,
    pub fragment_shading_rate: vk::PhysicalDeviceFragmentShadingRateFeaturesKHR<'a>,
    pub memory_priority: vk::PhysicalDeviceMemoryPriorityFeaturesEXT<'a>,
    pub conditional_rendering: vk::PhysicalDeviceConditionalRenderingFeaturesEXT<'a>,
    pub extended_dynamic_state3: vk::PhysicalDeviceExtendedDynamicState3FeaturesEXT<'a>,
    pub vertex_input_dynamic_state: vk::PhysicalDeviceVertexInputDynamicStateFeaturesEXT<'a>,
    pub custom_border_color: vk::PhysicalDeviceCustomBorderColorFeaturesEXT<'a>,
    pub image_view_min_lod: vk::PhysicalDeviceImageViewMinLodFeaturesEXT<'a>,
    pub image_compression_control: vk::PhysicalDeviceImageCompressionControlFeaturesEXT<'a>,
    pub msaa_render_to_single_sampled:
        vk::PhysicalDeviceMultisampledRenderToSingleSampledFeaturesEXT<'a>,
    pub shader_object: vk::PhysicalDeviceShaderObjectFeaturesEXT<'a>,
    pub optical_flow: vk::PhysicalDeviceOpticalFlowFeaturesNV<'a>,
    pub graphics_pipeline_library: vk::PhysicalDeviceGraphicsPipelineLibraryFeaturesEXT<'a>,
    /// GFX-7b: VK_EXT_descriptor_heap feature detection.
    pub descriptor_heap: vk::PhysicalDeviceDescriptorHeapFeaturesEXT<'a>,
}

pub fn available_feature_chain(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
) -> AvailableFeatureChain<'static> {
    let mut chain = AvailableFeatureChain {
        vulkan11: vk::PhysicalDeviceVulkan11Features::default(),
        descriptor_indexing: vk::PhysicalDeviceDescriptorIndexingFeatures::default(),
        timeline: vk::PhysicalDeviceTimelineSemaphoreFeatures::default(),
        dynamic_rendering: vk::PhysicalDeviceDynamicRenderingFeatures::default(),
        synchronization2: vk::PhysicalDeviceSynchronization2Features::default(),
        buffer_device_address: vk::PhysicalDeviceBufferDeviceAddressFeatures::default(),
        shader_float16_int8: vk::PhysicalDeviceShaderFloat16Int8Features::default(),
        mesh_shader: vk::PhysicalDeviceMeshShaderFeaturesEXT::default(),
        acceleration_structure: vk::PhysicalDeviceAccelerationStructureFeaturesKHR::default(),
        ray_tracing: vk::PhysicalDeviceRayTracingPipelineFeaturesKHR::default(),
        ray_query: vk::PhysicalDeviceRayQueryFeaturesKHR::default(),
        ray_tracing_position_fetch: vk::PhysicalDeviceRayTracingPositionFetchFeaturesKHR::default(),
        fragment_shading_rate: vk::PhysicalDeviceFragmentShadingRateFeaturesKHR::default(),
        memory_priority: vk::PhysicalDeviceMemoryPriorityFeaturesEXT::default(),
        conditional_rendering: vk::PhysicalDeviceConditionalRenderingFeaturesEXT::default(),
        extended_dynamic_state3: vk::PhysicalDeviceExtendedDynamicState3FeaturesEXT::default(),
        vertex_input_dynamic_state: vk::PhysicalDeviceVertexInputDynamicStateFeaturesEXT::default(),
        custom_border_color: vk::PhysicalDeviceCustomBorderColorFeaturesEXT::default(),
        image_view_min_lod: vk::PhysicalDeviceImageViewMinLodFeaturesEXT::default(),
        image_compression_control: vk::PhysicalDeviceImageCompressionControlFeaturesEXT::default(),
        msaa_render_to_single_sampled:
            vk::PhysicalDeviceMultisampledRenderToSingleSampledFeaturesEXT::default(),
        shader_object: vk::PhysicalDeviceShaderObjectFeaturesEXT::default(),
        optical_flow: vk::PhysicalDeviceOpticalFlowFeaturesNV::default(),
        graphics_pipeline_library: vk::PhysicalDeviceGraphicsPipelineLibraryFeaturesEXT::default(),
        descriptor_heap: vk::PhysicalDeviceDescriptorHeapFeaturesEXT::default(),
    };
    let mut features2 = vk::PhysicalDeviceFeatures2::default()
        .push(&mut chain.vulkan11)
        .push(&mut chain.descriptor_indexing)
        .push(&mut chain.timeline)
        .push(&mut chain.dynamic_rendering)
        .push(&mut chain.synchronization2)
        .push(&mut chain.buffer_device_address)
        .push(&mut chain.shader_float16_int8)
        .push(&mut chain.mesh_shader)
        .push(&mut chain.acceleration_structure)
        .push(&mut chain.ray_tracing)
        .push(&mut chain.ray_query)
        .push(&mut chain.ray_tracing_position_fetch)
        .push(&mut chain.fragment_shading_rate)
        .push(&mut chain.memory_priority)
        .push(&mut chain.conditional_rendering)
        .push(&mut chain.extended_dynamic_state3)
        .push(&mut chain.vertex_input_dynamic_state)
        .push(&mut chain.custom_border_color)
        .push(&mut chain.image_view_min_lod)
        .push(&mut chain.image_compression_control)
        .push(&mut chain.msaa_render_to_single_sampled)
        .push(&mut chain.shader_object)
        .push(&mut chain.optical_flow)
        .push(&mut chain.graphics_pipeline_library)
        .push(&mut chain.descriptor_heap);
    unsafe { instance.get_physical_device_features2(physical_device, &mut features2) };
    chain
}

fn push_descriptor_indexing_feature_names(
    names: &mut Vec<String>,
    features: &vk::PhysicalDeviceDescriptorIndexingFeatures<'_>,
) {
    macro_rules! push_feature {
        ($field:ident) => {
            if features.$field == vk::TRUE {
                names.push(stringify!($field).to_string());
            }
        };
    }

    push_feature!(shader_input_attachment_array_dynamic_indexing);
    push_feature!(shader_uniform_texel_buffer_array_dynamic_indexing);
    push_feature!(shader_storage_texel_buffer_array_dynamic_indexing);
    push_feature!(shader_uniform_buffer_array_non_uniform_indexing);
    push_feature!(shader_sampled_image_array_non_uniform_indexing);
    push_feature!(shader_storage_buffer_array_non_uniform_indexing);
    push_feature!(shader_storage_image_array_non_uniform_indexing);
    push_feature!(shader_input_attachment_array_non_uniform_indexing);
    push_feature!(shader_uniform_texel_buffer_array_non_uniform_indexing);
    push_feature!(shader_storage_texel_buffer_array_non_uniform_indexing);
    push_feature!(descriptor_binding_uniform_buffer_update_after_bind);
    push_feature!(descriptor_binding_sampled_image_update_after_bind);
    push_feature!(descriptor_binding_storage_image_update_after_bind);
    push_feature!(descriptor_binding_storage_buffer_update_after_bind);
    push_feature!(descriptor_binding_uniform_texel_buffer_update_after_bind);
    push_feature!(descriptor_binding_storage_texel_buffer_update_after_bind);
    push_feature!(descriptor_binding_update_unused_while_pending);
    push_feature!(descriptor_binding_partially_bound);
    push_feature!(descriptor_binding_variable_descriptor_count);
    push_feature!(runtime_descriptor_array);

    if features.runtime_descriptor_array == vk::TRUE
        && features.descriptor_binding_partially_bound == vk::TRUE
    {
        names.push("descriptor_indexing".into());
        names.push("bindless_resources".into());
    }
}

pub fn enable_core_feature(features: &mut vk::PhysicalDeviceFeatures, name: &str) -> bool {
    macro_rules! enable_feature {
        ($field:ident) => {
            if name == stringify!($field) {
                features.$field = vk::TRUE;
                return true;
            }
        };
    }

    enable_feature!(robust_buffer_access);
    enable_feature!(full_draw_index_uint32);
    enable_feature!(image_cube_array);
    enable_feature!(independent_blend);
    enable_feature!(geometry_shader);
    enable_feature!(tessellation_shader);
    enable_feature!(sample_rate_shading);
    enable_feature!(dual_src_blend);
    enable_feature!(logic_op);
    enable_feature!(multi_draw_indirect);
    enable_feature!(draw_indirect_first_instance);
    enable_feature!(depth_clamp);
    enable_feature!(depth_bias_clamp);
    enable_feature!(fill_mode_non_solid);
    enable_feature!(depth_bounds);
    enable_feature!(wide_lines);
    enable_feature!(large_points);
    enable_feature!(alpha_to_one);
    enable_feature!(multi_viewport);
    enable_feature!(sampler_anisotropy);
    enable_feature!(texture_compression_etc2);
    enable_feature!(texture_compression_astc_ldr);
    enable_feature!(texture_compression_bc);
    enable_feature!(occlusion_query_precise);
    enable_feature!(pipeline_statistics_query);
    enable_feature!(vertex_pipeline_stores_and_atomics);
    enable_feature!(fragment_stores_and_atomics);
    enable_feature!(shader_tessellation_and_geometry_point_size);
    enable_feature!(shader_image_gather_extended);
    enable_feature!(shader_storage_image_extended_formats);
    enable_feature!(shader_storage_image_multisample);
    enable_feature!(shader_storage_image_read_without_format);
    enable_feature!(shader_storage_image_write_without_format);
    enable_feature!(shader_uniform_buffer_array_dynamic_indexing);
    enable_feature!(shader_sampled_image_array_dynamic_indexing);
    enable_feature!(shader_storage_buffer_array_dynamic_indexing);
    enable_feature!(shader_storage_image_array_dynamic_indexing);
    enable_feature!(shader_clip_distance);
    enable_feature!(shader_cull_distance);
    enable_feature!(shader_float64);
    enable_feature!(shader_int64);
    enable_feature!(shader_int16);
    enable_feature!(shader_resource_residency);
    enable_feature!(shader_resource_min_lod);
    enable_feature!(sparse_binding);
    enable_feature!(sparse_residency_buffer);
    enable_feature!(sparse_residency_image2_d);
    enable_feature!(sparse_residency_image3_d);
    enable_feature!(sparse_residency2_samples);
    enable_feature!(sparse_residency4_samples);
    enable_feature!(sparse_residency8_samples);
    enable_feature!(sparse_residency16_samples);
    enable_feature!(sparse_residency_aliased);
    enable_feature!(variable_multisample_rate);
    enable_feature!(inherited_queries);

    false
}

fn available_device_extensions(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
) -> Vec<vk::ExtensionProperties> {
    unsafe {
        instance
            .enumerate_device_extension_properties(physical_device)
            .unwrap_or_default()
    }
}

fn conservative_rasterization_properties(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
) -> vk::PhysicalDeviceConservativeRasterizationPropertiesEXT<'static> {
    let mut properties = vk::PhysicalDeviceConservativeRasterizationPropertiesEXT::default();
    let mut properties2 = vk::PhysicalDeviceProperties2::default().push(&mut properties);
    unsafe {
        instance.get_physical_device_properties2(physical_device, &mut properties2);
    }
    properties
}

/// Query hardware shader-core/SM count from AMD or NVIDIA vendor extensions.
///
/// Returns the total compute-unit or SM count reported by the driver, or `None`
/// when neither extension is available on the device.
pub(crate) fn query_shader_core_count(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
    has_amd_shader_core_properties: bool,
    has_nv_shader_sm_builtins: bool,
) -> Option<u32> {
    if has_amd_shader_core_properties {
        let mut props = vk::PhysicalDeviceShaderCorePropertiesAMD::default();
        let mut props2 = vk::PhysicalDeviceProperties2::default().push(&mut props);
        unsafe { instance.get_physical_device_properties2(physical_device, &mut props2) };
        let total = props.shader_engine_count
            * props.shader_arrays_per_engine_count
            * props.compute_units_per_shader_array;
        return Some(total);
    }
    if has_nv_shader_sm_builtins {
        let mut props = vk::PhysicalDeviceShaderSMBuiltinsPropertiesNV::default();
        let mut props2 = vk::PhysicalDeviceProperties2::default().push(&mut props);
        unsafe { instance.get_physical_device_properties2(physical_device, &mut props2) };
        return Some(props.shader_sm_count);
    }
    None
}
