use std::ffi::CStr;

use ash::{vk, Instance};

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
    let mesh_shading = has(b"VK_EXT_mesh_shader\0");
    let descriptor_indexing = has(b"VK_EXT_descriptor_indexing\0");
    let dynamic_rendering = has(b"VK_KHR_dynamic_rendering\0");
    let synchronization2 = has(b"VK_KHR_synchronization2\0");
    // Timeline semaphores are core in Vulkan 1.2; also available as extension.
    let timeline_semaphores =
        properties.api_version >= vk::API_VERSION_1_2 || has(b"VK_KHR_timeline_semaphore\0");
    let hdr_output = has(b"VK_EXT_hdr_metadata\0") || has(b"VK_AMD_display_native_hdr\0");
    let variable_rate_shading = has(b"VK_KHR_fragment_shading_rate\0");
    let core_features = unsafe { instance.get_physical_device_features(physical_device) };
    let feature_chain = available_feature_chain(instance, physical_device);
    let bindless = descriptor_indexing
        && feature_chain.descriptor_indexing.runtime_descriptor_array == vk::TRUE
        && feature_chain
            .descriptor_indexing
            .descriptor_binding_partially_bound
            == vk::TRUE;
    let shader_fp16 = feature_chain.shader_float16_int8.shader_float16 == vk::TRUE;
    let shader_fp64 = core_features.shader_float64 == vk::TRUE;
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

    let features = BackendFeatures {
        ray_tracing,
        mesh_shading,
        bindless,
        descriptor_indexing,
        timeline_semaphores,
        dynamic_rendering,
        synchronization2,
        hdr_output,
        shader_fp16,
        shader_fp64,
        image_fp16_render,
        image_fp32_render,
        variable_rate_shading,
    };

    let limits = Limits {
        max_image_dimension_2d: lim.max_image_dimension2_d,
        max_image_dimension_3d: lim.max_image_dimension3_d,
        max_texture_2d_size: lim.max_image_dimension2_d,
        max_texture_array_layers: lim.max_image_array_layers,
        max_mip_levels,
        max_push_constants_size: lim.max_push_constants_size,
        max_bound_descriptor_sets: lim.max_bound_descriptor_sets,
        max_color_attachments: lim.max_color_attachments,
        max_compute_workgroup_size: lim.max_compute_work_group_size,
        max_compute_invocations: lim.max_compute_work_group_invocations,
        max_frames_in_flight: 2,
    };

    Caps {
        supports_raytracing: ray_tracing,
        supports_mesh_shading: mesh_shading,
        supports_bindless: bindless,
        max_mip_levels,
        max_frames_in_flight: 2,
        features,
        limits,
        raw_extension_names,
        raw_feature_names: available_feature_names(instance, physical_device),
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

    push_descriptor_indexing_feature_names(&mut names, &chain.descriptor_indexing);
    names.sort();
    names.dedup();
    names
}

#[derive(Clone, Copy)]
pub struct AvailableFeatureChain<'a> {
    pub descriptor_indexing: vk::PhysicalDeviceDescriptorIndexingFeatures<'a>,
    pub timeline: vk::PhysicalDeviceTimelineSemaphoreFeatures<'a>,
    pub dynamic_rendering: vk::PhysicalDeviceDynamicRenderingFeatures<'a>,
    pub synchronization2: vk::PhysicalDeviceSynchronization2Features<'a>,
    pub shader_float16_int8: vk::PhysicalDeviceShaderFloat16Int8Features<'a>,
    pub mesh_shader: vk::PhysicalDeviceMeshShaderFeaturesEXT<'a>,
    pub acceleration_structure: vk::PhysicalDeviceAccelerationStructureFeaturesKHR<'a>,
    pub ray_tracing: vk::PhysicalDeviceRayTracingPipelineFeaturesKHR<'a>,
    pub fragment_shading_rate: vk::PhysicalDeviceFragmentShadingRateFeaturesKHR<'a>,
}

pub fn available_feature_chain(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
) -> AvailableFeatureChain<'static> {
    let mut chain = AvailableFeatureChain {
        descriptor_indexing: vk::PhysicalDeviceDescriptorIndexingFeatures::default(),
        timeline: vk::PhysicalDeviceTimelineSemaphoreFeatures::default(),
        dynamic_rendering: vk::PhysicalDeviceDynamicRenderingFeatures::default(),
        synchronization2: vk::PhysicalDeviceSynchronization2Features::default(),
        shader_float16_int8: vk::PhysicalDeviceShaderFloat16Int8Features::default(),
        mesh_shader: vk::PhysicalDeviceMeshShaderFeaturesEXT::default(),
        acceleration_structure: vk::PhysicalDeviceAccelerationStructureFeaturesKHR::default(),
        ray_tracing: vk::PhysicalDeviceRayTracingPipelineFeaturesKHR::default(),
        fragment_shading_rate: vk::PhysicalDeviceFragmentShadingRateFeaturesKHR::default(),
    };
    let mut features2 = vk::PhysicalDeviceFeatures2::default()
        .push_next(&mut chain.descriptor_indexing)
        .push_next(&mut chain.timeline)
        .push_next(&mut chain.dynamic_rendering)
        .push_next(&mut chain.synchronization2)
        .push_next(&mut chain.shader_float16_int8)
        .push_next(&mut chain.mesh_shader)
        .push_next(&mut chain.acceleration_structure)
        .push_next(&mut chain.ray_tracing)
        .push_next(&mut chain.fragment_shading_rate);
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
