use std::collections::HashSet;
use std::ffi::{CStr, CString, c_void};

use ash::vk::TaggedStructure;
use ash::{Device as AshDevice, Instance, vk};

use crate::{AdapterSelection, Error, Result};

// GFX-1g: Device memory report callback — logs allocations in debug builds.
unsafe extern "system" fn device_memory_report_callback(
    _data: *const vk::DeviceMemoryReportCallbackDataEXT<'_>,
    _user_data: *mut c_void,
) {
    #[cfg(debug_assertions)]
    {
        if let Some(data) = unsafe { _data.as_ref() } {
            let type_str = match data.ty {
                vk::DeviceMemoryReportEventTypeEXT::ALLOCATE => "alloc",
                vk::DeviceMemoryReportEventTypeEXT::FREE => "free",
                vk::DeviceMemoryReportEventTypeEXT::IMPORT => "import",
                vk::DeviceMemoryReportEventTypeEXT::UNIMPORT => "unimport",
                vk::DeviceMemoryReportEventTypeEXT::ALLOCATION_FAILED => "alloc-failed",
                _ => "unknown",
            };
            let bytes = data.size;
            if bytes > 0 {
                tracing::info!(
                    "{type_str} type={} size={}B obj={:#x}",
                    data.memory_type,
                    bytes,
                    data.memory_object_id
                );
            }
        }
    }
}

use super::adapter;
use super::caps;
use super::config::VulkanBackendConfig;
use super::queues::{QueueFamilyMap, VulkanQueues};

pub struct DeviceSelection {
    pub physical_device: vk::PhysicalDevice,
    pub queue_families: QueueFamilyMap,
}

pub struct LogicalDevice {
    pub device: AshDevice,
    pub queue_families: QueueFamilyMap,
    pub queues: VulkanQueues,
    pub enabled_extension_names: Vec<String>,
    pub mesh_shader_enabled: bool,
    pub synchronization2_enabled: bool,
    pub dynamic_rendering_enabled: bool,
    pub timeline_semaphores_enabled: bool,
    pub buffer_device_address_enabled: bool,
    pub memory_priority_enabled: bool,
    pub push_descriptors_enabled: bool,
    pub conditional_rendering_enabled: bool,
    pub custom_border_color_enabled: bool,
    pub conservative_rasterization_enabled: bool,
    pub vrs_pipeline_enabled: bool,
    pub vrs_primitive_enabled: bool,
    pub vrs_attachment_enabled: bool,
    pub global_queue_priority_enabled: bool,
    pub acceleration_structure_enabled: bool,
    pub ray_tracing_pipeline_enabled: bool,
    pub ray_query_enabled: bool,
    pub ray_tracing_position_fetch_enabled: bool,
    pub extended_dynamic_state3_enabled: bool,
    pub vertex_input_dynamic_state_enabled: bool,
    pub shader_object_enabled: bool,
    pub graphics_pipeline_library_enabled: bool,
}

impl DeviceSelection {
    pub fn pick(instance: &Instance, selection: &AdapterSelection) -> Result<Self> {
        let physical_device = adapter::pick(instance, selection)?;
        let families =
            unsafe { instance.get_physical_device_queue_family_properties(physical_device) };
        let graphics = families
            .iter()
            .enumerate()
            .find(|(_, f)| f.queue_count > 0 && f.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .map(|(i, _)| i as u32)
            .ok_or(Error::Unsupported(
                "selected Vulkan physical device has no graphics queue",
            ))?;
        let video_decode = QueueFamilyMap::select_video_decode(&families, graphics);
        let video_encode = QueueFamilyMap::select_video_encode(&families, graphics);
        let async_compute = QueueFamilyMap::select_async_compute(&families, graphics);
        let dma = QueueFamilyMap::select_dma(&families, graphics);
        Ok(Self {
            physical_device,
            queue_families: QueueFamilyMap {
                graphics,
                compute: graphics,
                transfer: graphics,
                video_decode,
                video_encode,
                async_compute,
                dma,
            },
        })
    }
}

fn queue_global_priority(
    queue_families: QueueFamilyMap,
    family: u32,
) -> vk::QueueGlobalPriorityKHR {
    if family == queue_families.graphics {
        vk::QueueGlobalPriorityKHR::HIGH
    } else if family == queue_families.compute {
        vk::QueueGlobalPriorityKHR::MEDIUM
    } else {
        vk::QueueGlobalPriorityKHR::LOW
    }
}

pub fn create_logical_device(
    instance: &Instance,
    selection: &DeviceSelection,
    config: &VulkanBackendConfig,
) -> Result<LogicalDevice> {
    // Validate minimum API version before any expensive work.
    let device_props =
        unsafe { instance.get_physical_device_properties(selection.physical_device) };
    let device_version = crate::VulkanApiVersion::from_vk(device_props.api_version);
    if device_version < config.min_api_version {
        return Err(crate::Error::Unsupported(
            "selected Vulkan physical device does not meet the minimum required API version",
        ));
    }

    let priority = [1.0f32];
    let mut unique_families = vec![
        selection.queue_families.graphics,
        selection.queue_families.compute,
        selection.queue_families.transfer,
    ];
    unique_families.sort_unstable();
    unique_families.dedup();
    let mut feature_request = FeatureRequest::resolve(instance, selection.physical_device, config)?;
    let mesh_shader_enabled = feature_request.mesh_shader.mesh_shader == vk::TRUE;
    let synchronization2_enabled = feature_request.synchronization2.synchronization2 == vk::TRUE;
    let dynamic_rendering_enabled = feature_request.dynamic_rendering.dynamic_rendering == vk::TRUE;
    let timeline_semaphores_enabled = feature_request.timeline.timeline_semaphore == vk::TRUE;
    let buffer_device_address_enabled =
        feature_request.buffer_device_address.buffer_device_address == vk::TRUE;
    let memory_priority_enabled = feature_request.memory_priority.memory_priority == vk::TRUE;
    let conditional_rendering_enabled =
        feature_request.conditional_rendering.conditional_rendering == vk::TRUE;
    let custom_border_color_enabled =
        feature_request.custom_border_color.custom_border_colors == vk::TRUE;
    let vrs_pipeline_enabled = feature_request
        .fragment_shading_rate
        .pipeline_fragment_shading_rate
        == vk::TRUE;
    let vrs_primitive_enabled = feature_request
        .fragment_shading_rate
        .primitive_fragment_shading_rate
        == vk::TRUE;
    let vrs_attachment_enabled = feature_request
        .fragment_shading_rate
        .attachment_fragment_shading_rate
        == vk::TRUE;
    let acceleration_structure_enabled = feature_request
        .acceleration_structure
        .acceleration_structure
        == vk::TRUE;
    let ray_tracing_pipeline_enabled = feature_request.ray_tracing.ray_tracing_pipeline == vk::TRUE;
    let ray_query_enabled = feature_request.ray_query.ray_query == vk::TRUE;
    let ray_tracing_position_fetch_enabled = feature_request
        .ray_tracing_position_fetch
        .ray_tracing_position_fetch
        == vk::TRUE;
    let extended_dynamic_state3_enabled = feature_request
        .extended_dynamic_state3
        .extended_dynamic_state3_polygon_mode
        == vk::TRUE
        || feature_request
            .extended_dynamic_state3
            .extended_dynamic_state3_rasterization_samples
            == vk::TRUE
        || feature_request
            .extended_dynamic_state3
            .extended_dynamic_state3_color_blend_enable
            == vk::TRUE;
    let vertex_input_dynamic_state_enabled = feature_request
        .vertex_input_dynamic_state
        .vertex_input_dynamic_state
        == vk::TRUE;
    let shader_object_enabled = feature_request.shader_object.shader_object == vk::TRUE;
    let graphics_pipeline_library_enabled = feature_request
        .graphics_pipeline_library
        .graphics_pipeline_library
        == vk::TRUE;
    // push_descriptors, conservative_rasterization, and global_queue_priority are extension-only
    // (no feature struct).
    // They are enabled if the extension was added to required_extensions by resolve().
    let push_descriptors_enabled = feature_request
        .required_extensions
        .iter()
        .any(|e| *e == ash::khr::push_descriptor::NAME);
    let conservative_rasterization_enabled = feature_request
        .required_extensions
        .iter()
        .any(|e| *e == ash::ext::conservative_rasterization::NAME);
    let global_queue_priority_enabled = feature_request
        .required_extensions
        .iter()
        .any(|e| *e == ash::khr::global_priority::NAME);
    let extension_request = ExtensionRequest::resolve(
        instance,
        selection.physical_device,
        config,
        &feature_request.required_extensions,
    )?;
    // GFX-1g: In debug builds, automatically enable diagnostic extensions when available.
    // These are telemetry-only and do not affect rendering correctness.
    #[cfg(debug_assertions)]
    let extension_request = {
        let mut er = extension_request;
        let available_exts =
            caps::available_device_extension_names(instance, selection.physical_device)
                .into_iter()
                .collect::<HashSet<_>>();
        for ext_name in [
            "VK_EXT_device_memory_report",
            "VK_EXT_device_address_binding_report",
            "VK_EXT_device_fault",
            "VK_NV_device_diagnostic_checkpoints",
        ] {
            if available_exts.contains(ext_name) {
                let Ok(c_name) = CString::new(ext_name) else {
                    continue;
                };
                if !er.names.iter().any(|n| n == &c_name) {
                    er.names.push(c_name);
                }
            }
        }
        er
    };
    let device_extension_ptrs = extension_request
        .names
        .iter()
        .map(|extension| extension.as_ptr())
        .collect::<Vec<_>>();

    let global_priority_infos = unique_families
        .iter()
        .map(|family| {
            let priority = queue_global_priority(selection.queue_families, *family);
            vk::DeviceQueueGlobalPriorityCreateInfoKHR::default().global_priority(priority)
        })
        .collect::<Vec<_>>();
    let mut queue_info = Vec::with_capacity(unique_families.len());
    for (index, family) in unique_families.iter().enumerate() {
        let mut info = vk::DeviceQueueCreateInfo::default()
            .queue_family_index(*family)
            .queue_priorities(&priority);
        if global_queue_priority_enabled {
            info.p_next = (&global_priority_infos[index]
                as *const vk::DeviceQueueGlobalPriorityCreateInfoKHR)
                .cast::<c_void>();
        }
        queue_info.push(info);
    }

    let device_info_base = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queue_info)
        .enabled_extension_names(&device_extension_ptrs);
    let mut device_info = feature_request.apply_to(device_info_base);

    // GFX-1g: Chain device memory report callback when the extension is available.
    let mut memory_report_info;
    let has_memory_report = extension_request
        .names
        .iter()
        .any(|n| n.to_bytes() == b"VK_EXT_device_memory_report");
    if has_memory_report {
        memory_report_info = vk::DeviceDeviceMemoryReportCreateInfoEXT::default()
            .pfn_user_callback(Some(device_memory_report_callback))
            .user_data(std::ptr::null_mut());
        device_info = device_info.push(&mut memory_report_info);
    }
    // GFX-1g: Enable address binding report feature when extension is available.
    let mut address_binding_report_features;
    let has_address_binding_report = extension_request
        .names
        .iter()
        .any(|n| n.to_bytes() == b"VK_EXT_device_address_binding_report");
    if has_address_binding_report {
        address_binding_report_features =
            vk::PhysicalDeviceAddressBindingReportFeaturesEXT::default()
                .report_address_binding(true);
        device_info = device_info.push(&mut address_binding_report_features);
    }

    let device = unsafe {
        instance
            .create_device(selection.physical_device, &device_info, None)
            .map_err(|error| Error::Backend(format!("failed to create Vulkan device: {error:?}")))?
    };
    let queues = unsafe {
        VulkanQueues {
            graphics: device.get_device_queue(selection.queue_families.graphics, 0),
            compute: device.get_device_queue(selection.queue_families.compute, 0),
            transfer: device.get_device_queue(selection.queue_families.transfer, 0),
            async_compute: device.get_device_queue(selection.queue_families.async_compute, 0),
            dma: device.get_device_queue(selection.queue_families.dma, 0),
        }
    };

    Ok(LogicalDevice {
        device,
        queue_families: selection.queue_families,
        queues,
        enabled_extension_names: extension_request
            .names
            .iter()
            .map(|name| name.to_string_lossy().into_owned())
            .collect(),
        mesh_shader_enabled,
        synchronization2_enabled,
        dynamic_rendering_enabled,
        timeline_semaphores_enabled,
        buffer_device_address_enabled,
        memory_priority_enabled,
        push_descriptors_enabled,
        conditional_rendering_enabled,
        custom_border_color_enabled,
        conservative_rasterization_enabled,
        vrs_pipeline_enabled,
        vrs_primitive_enabled,
        vrs_attachment_enabled,
        global_queue_priority_enabled,
        acceleration_structure_enabled,
        ray_tracing_pipeline_enabled,
        ray_query_enabled,
        ray_tracing_position_fetch_enabled,
        extended_dynamic_state3_enabled,
        vertex_input_dynamic_state_enabled,
        shader_object_enabled,
        graphics_pipeline_library_enabled,
    })
}

pub fn physical_device_name(instance: &Instance, physical_device: vk::PhysicalDevice) -> String {
    let properties = unsafe { instance.get_physical_device_properties(physical_device) };
    let name = unsafe { CStr::from_ptr(properties.device_name.as_ptr()) };
    name.to_string_lossy().into_owned()
}

struct ExtensionRequest {
    names: Vec<CString>,
}

impl ExtensionRequest {
    fn resolve(
        instance: &Instance,
        physical_device: vk::PhysicalDevice,
        config: &VulkanBackendConfig,
        feature_required_extensions: &[&'static CStr],
    ) -> Result<Self> {
        let available = caps::available_device_extension_names(instance, physical_device)
            .into_iter()
            .collect::<HashSet<_>>();
        let disabled = config
            .disabled_extensions
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let mut requested = Vec::new();

        for extension in required_device_extensions() {
            let name = extension.to_string_lossy().into_owned();
            if disabled.contains(&name) {
                return Err(Error::Unsupported(
                    "required Vulkan swapchain extension was disabled",
                ));
            }
            if !available.contains(&name) {
                return Err(Error::Unsupported(
                    "selected Vulkan physical device does not support a required device extension",
                ));
            }
            push_unique(&mut requested, name);
        }
        for extension in feature_required_extensions {
            let name = extension.to_string_lossy().into_owned();
            if disabled.contains(&name) {
                return Err(Error::Unsupported(
                    "Vulkan extension required by a requested feature was disabled",
                ));
            }
            if !available.contains(&name) {
                return Err(Error::InvalidInput(format!(
                    "selected Vulkan physical device does not support extension {name} required by requested features"
                )));
            }
            push_unique(&mut requested, name);
        }

        for name in &config.required_extensions {
            if disabled.contains(name) {
                return Err(Error::Unsupported(
                    "Vulkan device extension was both required and disabled",
                ));
            }
            if !available.contains(name) {
                return Err(Error::InvalidInput(format!(
                    "selected Vulkan physical device does not support required device extension {name}"
                )));
            }
            push_unique(&mut requested, name.clone());
        }

        for name in &config.optional_extensions {
            if !disabled.contains(name) && available.contains(name) {
                push_unique(&mut requested, name.clone());
            }
        }

        let names = requested
            .into_iter()
            .map(|name| {
                CString::new(name.as_str()).map_err(|_| {
                    Error::InvalidInput(format!(
                        "Vulkan device extension name contains an interior nul byte: {name:?}"
                    ))
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { names })
    }
}

struct FeatureRequest<'a> {
    features2: vk::PhysicalDeviceFeatures2<'a>,
    vulkan11: vk::PhysicalDeviceVulkan11Features<'a>,
    descriptor_indexing: vk::PhysicalDeviceDescriptorIndexingFeatures<'a>,
    timeline: vk::PhysicalDeviceTimelineSemaphoreFeatures<'a>,
    dynamic_rendering: vk::PhysicalDeviceDynamicRenderingFeatures<'a>,
    synchronization2: vk::PhysicalDeviceSynchronization2Features<'a>,
    buffer_device_address: vk::PhysicalDeviceBufferDeviceAddressFeatures<'a>,
    mesh_shader: vk::PhysicalDeviceMeshShaderFeaturesEXT<'a>,
    acceleration_structure: vk::PhysicalDeviceAccelerationStructureFeaturesKHR<'a>,
    ray_tracing: vk::PhysicalDeviceRayTracingPipelineFeaturesKHR<'a>,
    ray_query: vk::PhysicalDeviceRayQueryFeaturesKHR<'a>,
    ray_tracing_position_fetch: vk::PhysicalDeviceRayTracingPositionFetchFeaturesKHR<'a>,
    fragment_shading_rate: vk::PhysicalDeviceFragmentShadingRateFeaturesKHR<'a>,
    memory_priority: vk::PhysicalDeviceMemoryPriorityFeaturesEXT<'a>,
    conditional_rendering: vk::PhysicalDeviceConditionalRenderingFeaturesEXT<'a>,
    custom_border_color: vk::PhysicalDeviceCustomBorderColorFeaturesEXT<'a>,
    extended_dynamic_state3: vk::PhysicalDeviceExtendedDynamicState3FeaturesEXT<'a>,
    vertex_input_dynamic_state: vk::PhysicalDeviceVertexInputDynamicStateFeaturesEXT<'a>,
    shader_object: vk::PhysicalDeviceShaderObjectFeaturesEXT<'a>,
    optical_flow: vk::PhysicalDeviceOpticalFlowFeaturesNV<'a>,
    graphics_pipeline_library: vk::PhysicalDeviceGraphicsPipelineLibraryFeaturesEXT<'a>,
    use_feature_chain: bool,
    required_extensions: Vec<&'static CStr>,
}

impl FeatureRequest<'static> {
    fn resolve(
        instance: &Instance,
        physical_device: vk::PhysicalDevice,
        config: &VulkanBackendConfig,
    ) -> Result<Self> {
        let available_core = caps::available_core_feature_names(instance, physical_device)
            .into_iter()
            .collect::<HashSet<_>>();
        let available_chain = caps::available_feature_chain(instance, physical_device);
        let properties = unsafe { instance.get_physical_device_properties(physical_device) };
        let available_extensions =
            caps::available_device_extension_names(instance, physical_device)
                .into_iter()
                .collect::<HashSet<_>>();
        let disabled = config
            .disabled_features
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let mut request = Self {
            features2: vk::PhysicalDeviceFeatures2::default(),
            vulkan11: vk::PhysicalDeviceVulkan11Features::default(),
            descriptor_indexing: vk::PhysicalDeviceDescriptorIndexingFeatures::default(),
            timeline: vk::PhysicalDeviceTimelineSemaphoreFeatures::default(),
            dynamic_rendering: vk::PhysicalDeviceDynamicRenderingFeatures::default(),
            synchronization2: vk::PhysicalDeviceSynchronization2Features::default(),
            buffer_device_address: vk::PhysicalDeviceBufferDeviceAddressFeatures::default(),
            mesh_shader: vk::PhysicalDeviceMeshShaderFeaturesEXT::default(),
            acceleration_structure: vk::PhysicalDeviceAccelerationStructureFeaturesKHR::default(),
            ray_tracing: vk::PhysicalDeviceRayTracingPipelineFeaturesKHR::default(),
            ray_query: vk::PhysicalDeviceRayQueryFeaturesKHR::default(),
            ray_tracing_position_fetch:
                vk::PhysicalDeviceRayTracingPositionFetchFeaturesKHR::default(),
            fragment_shading_rate: vk::PhysicalDeviceFragmentShadingRateFeaturesKHR::default(),
            memory_priority: vk::PhysicalDeviceMemoryPriorityFeaturesEXT::default(),
            conditional_rendering: vk::PhysicalDeviceConditionalRenderingFeaturesEXT::default(),
            custom_border_color: vk::PhysicalDeviceCustomBorderColorFeaturesEXT::default(),
            extended_dynamic_state3: vk::PhysicalDeviceExtendedDynamicState3FeaturesEXT::default(),
            vertex_input_dynamic_state:
                vk::PhysicalDeviceVertexInputDynamicStateFeaturesEXT::default(),
            shader_object: vk::PhysicalDeviceShaderObjectFeaturesEXT::default(),
            optical_flow: vk::PhysicalDeviceOpticalFlowFeaturesNV::default(),
            graphics_pipeline_library:
                vk::PhysicalDeviceGraphicsPipelineLibraryFeaturesEXT::default(),
            use_feature_chain: false,
            required_extensions: Vec::new(),
        };

        for name in &config.required_features {
            if disabled.contains(name) {
                return Err(Error::Unsupported(
                    "Vulkan feature was both required and disabled",
                ));
            }
            request.enable_feature(
                name,
                true,
                &available_core,
                &available_chain,
                &available_extensions,
                properties.api_version,
            )?;
        }

        for name in &config.optional_features {
            if disabled.contains(name) {
                continue;
            }
            match request.enable_feature(
                name,
                false,
                &available_core,
                &available_chain,
                &available_extensions,
                properties.api_version,
            ) {
                Ok(()) => tracing::debug!("optional Vulkan feature enabled: {name}"),
                Err(e) => {
                    tracing::debug!("optional Vulkan feature skipped (not available): {name}: {e}")
                }
            }
        }

        if !disabled.contains("shader_draw_parameters")
            && available_chain.vulkan11.shader_draw_parameters == vk::TRUE
        {
            request.vulkan11.shader_draw_parameters = vk::TRUE;
        }

        if request.ray_tracing.ray_tracing_pipeline == vk::TRUE
            && !disabled.contains("ray_tracing_position_fetch")
        {
            request.enable_feature(
                "ray_tracing_position_fetch",
                false,
                &available_core,
                &available_chain,
                &available_extensions,
                properties.api_version,
            )?;
        }

        // Do NOT call rebuild_chain() here — the struct is returned by value and
        // rebuild_chain() stores raw self-pointers in the pNext chain.  Moving the
        // struct after that would leave every pNext pointer dangling.  Instead,
        // rebuild_chain() is called inside apply_to(), after the struct is at its
        // final stable address in the caller's stack frame.
        Ok(request)
    }

    fn apply_to<'a>(&'a mut self, info: vk::DeviceCreateInfo<'a>) -> vk::DeviceCreateInfo<'a> {
        // Rebuild the pNext chain now that `self` is at its stable location and
        // will not be moved again before vkCreateDevice is called.
        self.rebuild_chain();
        if self.use_feature_chain {
            let mut info = info;
            info.p_next = (&mut self.features2 as *mut vk::PhysicalDeviceFeatures2<'static>).cast();
            info
        } else {
            info.enabled_features(&self.features2.features)
        }
    }

    fn enable_feature(
        &mut self,
        name: &str,
        required: bool,
        available_core: &HashSet<String>,
        available_chain: &caps::AvailableFeatureChain<'_>,
        available_extensions: &HashSet<String>,
        api_version: u32,
    ) -> Result<()> {
        if available_core.contains(name) {
            if !caps::enable_core_feature(&mut self.features2.features, name) {
                return Err(Error::InvalidInput(format!(
                    "unknown Vulkan core feature name: {name}"
                )));
            }
            return Ok(());
        }

        let supported = self.enable_chain_feature(name, available_chain);
        if supported {
            self.require_feature_extensions(name, available_extensions, api_version)?;
            return Ok(());
        }

        if required {
            return Err(Error::InvalidInput(format!(
                "selected Vulkan physical device does not support required feature {name}"
            )));
        }

        if is_known_feature_name(name) {
            return Ok(());
        }

        Err(Error::InvalidInput(format!(
            "unknown Vulkan feature name: {name}"
        )))
    }

    fn enable_chain_feature(
        &mut self,
        name: &str,
        available: &caps::AvailableFeatureChain<'_>,
    ) -> bool {
        match name {
            "descriptor_indexing" | "bindless_resources" => {
                if available.descriptor_indexing.runtime_descriptor_array != vk::TRUE
                    || available
                        .descriptor_indexing
                        .descriptor_binding_partially_bound
                        != vk::TRUE
                {
                    return false;
                }
                self.descriptor_indexing.runtime_descriptor_array = vk::TRUE;
                self.descriptor_indexing.descriptor_binding_partially_bound = vk::TRUE;
                self.descriptor_indexing
                    .descriptor_binding_variable_descriptor_count = available
                    .descriptor_indexing
                    .descriptor_binding_variable_descriptor_count;
                self.descriptor_indexing
                    .shader_sampled_image_array_non_uniform_indexing = available
                    .descriptor_indexing
                    .shader_sampled_image_array_non_uniform_indexing;
                if available
                    .descriptor_indexing
                    .descriptor_binding_sampled_image_update_after_bind
                    != vk::TRUE
                    || available
                        .descriptor_indexing
                        .descriptor_binding_storage_image_update_after_bind
                        != vk::TRUE
                    || available
                        .descriptor_indexing
                        .descriptor_binding_storage_buffer_update_after_bind
                        != vk::TRUE
                {
                    return false;
                }
                self.descriptor_indexing
                    .descriptor_binding_sampled_image_update_after_bind = vk::TRUE;
                self.descriptor_indexing
                    .descriptor_binding_storage_image_update_after_bind = vk::TRUE;
                self.descriptor_indexing
                    .descriptor_binding_storage_buffer_update_after_bind = vk::TRUE;
                true
            }
            "timeline_semaphore" | "timeline_semaphores" => {
                if available.timeline.timeline_semaphore != vk::TRUE {
                    return false;
                }
                self.timeline.timeline_semaphore = vk::TRUE;
                true
            }
            "dynamic_rendering" => {
                if available.dynamic_rendering.dynamic_rendering != vk::TRUE {
                    return false;
                }
                self.dynamic_rendering.dynamic_rendering = vk::TRUE;
                true
            }
            "synchronization2" => {
                if available.synchronization2.synchronization2 != vk::TRUE {
                    return false;
                }
                self.synchronization2.synchronization2 = vk::TRUE;
                true
            }
            "buffer_device_address" => {
                if available.buffer_device_address.buffer_device_address != vk::TRUE {
                    return false;
                }
                self.buffer_device_address.buffer_device_address = vk::TRUE;
                true
            }
            "mesh_shading" | "mesh_shader" => {
                if available.mesh_shader.mesh_shader != vk::TRUE {
                    return false;
                }
                self.mesh_shader.mesh_shader = vk::TRUE;
                self.mesh_shader.task_shader = available.mesh_shader.task_shader;
                true
            }
            "task_shader" => {
                if available.mesh_shader.task_shader != vk::TRUE {
                    return false;
                }
                self.mesh_shader.task_shader = vk::TRUE;
                true
            }
            "ray_tracing" => {
                if available.ray_tracing.ray_tracing_pipeline != vk::TRUE
                    || available.acceleration_structure.acceleration_structure != vk::TRUE
                {
                    return false;
                }
                self.ray_tracing.ray_tracing_pipeline = vk::TRUE;
                self.acceleration_structure.acceleration_structure = vk::TRUE;
                true
            }
            "ray_tracing_pipeline" => {
                if available.ray_tracing.ray_tracing_pipeline != vk::TRUE {
                    return false;
                }
                self.ray_tracing.ray_tracing_pipeline = vk::TRUE;
                true
            }
            "ray_query" => {
                if available.ray_query.ray_query != vk::TRUE
                    || available.acceleration_structure.acceleration_structure != vk::TRUE
                {
                    return false;
                }
                self.ray_query.ray_query = vk::TRUE;
                self.acceleration_structure.acceleration_structure = vk::TRUE;
                true
            }
            "ray_tracing_position_fetch" => {
                if available
                    .ray_tracing_position_fetch
                    .ray_tracing_position_fetch
                    != vk::TRUE
                {
                    return false;
                }
                self.ray_tracing_position_fetch.ray_tracing_position_fetch = vk::TRUE;
                true
            }
            "acceleration_structure" => {
                if available.acceleration_structure.acceleration_structure != vk::TRUE {
                    return false;
                }
                self.acceleration_structure.acceleration_structure = vk::TRUE;
                true
            }
            "variable_rate_shading" | "pipeline_fragment_shading_rate" => {
                if available
                    .fragment_shading_rate
                    .pipeline_fragment_shading_rate
                    != vk::TRUE
                {
                    return false;
                }
                self.fragment_shading_rate.pipeline_fragment_shading_rate = vk::TRUE;
                true
            }
            "primitive_fragment_shading_rate" => {
                if available
                    .fragment_shading_rate
                    .primitive_fragment_shading_rate
                    != vk::TRUE
                {
                    return false;
                }
                self.fragment_shading_rate.primitive_fragment_shading_rate = vk::TRUE;
                true
            }
            "attachment_fragment_shading_rate" => {
                if available
                    .fragment_shading_rate
                    .attachment_fragment_shading_rate
                    != vk::TRUE
                {
                    return false;
                }
                self.fragment_shading_rate.attachment_fragment_shading_rate = vk::TRUE;
                true
            }
            "memory_priority" => {
                if available.memory_priority.memory_priority != vk::TRUE {
                    return false;
                }
                self.memory_priority.memory_priority = vk::TRUE;
                true
            }
            "conditional_rendering" => {
                if available.conditional_rendering.conditional_rendering != vk::TRUE {
                    return false;
                }
                self.conditional_rendering.conditional_rendering = vk::TRUE;
                true
            }
            "custom_border_color" | "custom_border_colors" => {
                if available.custom_border_color.custom_border_colors != vk::TRUE {
                    return false;
                }
                self.custom_border_color.custom_border_colors = vk::TRUE;
                true
            }
            // push_descriptor, conservative_rasterization, and global_queue_priority are
            // extension-only — no feature struct.
            // They are handled purely via require_feature_extensions.
            "push_descriptor"
            | "push_descriptors"
            | "conservative_rasterization"
            | "global_queue_priority" => {
                // Return true so require_feature_extensions is called.
                true
            }
            "extended_dynamic_state3" => {
                let any = available
                    .extended_dynamic_state3
                    .extended_dynamic_state3_polygon_mode
                    == vk::TRUE
                    || available
                        .extended_dynamic_state3
                        .extended_dynamic_state3_rasterization_samples
                        == vk::TRUE
                    || available
                        .extended_dynamic_state3
                        .extended_dynamic_state3_color_blend_enable
                        == vk::TRUE;
                if !any {
                    return false;
                }
                self.extended_dynamic_state3
                    .extended_dynamic_state3_polygon_mode = available
                    .extended_dynamic_state3
                    .extended_dynamic_state3_polygon_mode;
                self.extended_dynamic_state3
                    .extended_dynamic_state3_rasterization_samples = available
                    .extended_dynamic_state3
                    .extended_dynamic_state3_rasterization_samples;
                self.extended_dynamic_state3
                    .extended_dynamic_state3_color_blend_enable = available
                    .extended_dynamic_state3
                    .extended_dynamic_state3_color_blend_enable;
                self.extended_dynamic_state3
                    .extended_dynamic_state3_color_blend_equation = available
                    .extended_dynamic_state3
                    .extended_dynamic_state3_color_blend_equation;
                true
            }
            "vertex_input_dynamic_state" => {
                if available
                    .vertex_input_dynamic_state
                    .vertex_input_dynamic_state
                    != vk::TRUE
                {
                    return false;
                }
                self.vertex_input_dynamic_state.vertex_input_dynamic_state = vk::TRUE;
                true
            }
            "shader_object" => {
                if available.shader_object.shader_object != vk::TRUE {
                    return false;
                }
                self.shader_object.shader_object = vk::TRUE;
                true
            }
            "optical_flow" | "optical_flow_nv" => {
                if available.optical_flow.optical_flow != vk::TRUE {
                    return false;
                }
                self.optical_flow.optical_flow = vk::TRUE;
                true
            }
            "graphics_pipeline_library" => {
                if available
                    .graphics_pipeline_library
                    .graphics_pipeline_library
                    != vk::TRUE
                {
                    return false;
                }
                self.graphics_pipeline_library.graphics_pipeline_library = vk::TRUE;
                true
            }
            _ => self.enable_descriptor_indexing_field(name, &available.descriptor_indexing),
        }
    }

    fn enable_descriptor_indexing_field(
        &mut self,
        name: &str,
        available: &vk::PhysicalDeviceDescriptorIndexingFeatures<'_>,
    ) -> bool {
        macro_rules! enable {
            ($field:ident) => {
                if name == stringify!($field) {
                    if available.$field != vk::TRUE {
                        return false;
                    }
                    self.descriptor_indexing.$field = vk::TRUE;
                    return true;
                }
            };
        }

        enable!(shader_input_attachment_array_dynamic_indexing);
        enable!(shader_uniform_texel_buffer_array_dynamic_indexing);
        enable!(shader_storage_texel_buffer_array_dynamic_indexing);
        enable!(shader_uniform_buffer_array_non_uniform_indexing);
        enable!(shader_sampled_image_array_non_uniform_indexing);
        enable!(shader_storage_buffer_array_non_uniform_indexing);
        enable!(shader_storage_image_array_non_uniform_indexing);
        enable!(shader_input_attachment_array_non_uniform_indexing);
        enable!(shader_uniform_texel_buffer_array_non_uniform_indexing);
        enable!(shader_storage_texel_buffer_array_non_uniform_indexing);
        enable!(descriptor_binding_uniform_buffer_update_after_bind);
        enable!(descriptor_binding_sampled_image_update_after_bind);
        enable!(descriptor_binding_storage_image_update_after_bind);
        enable!(descriptor_binding_storage_buffer_update_after_bind);
        enable!(descriptor_binding_uniform_texel_buffer_update_after_bind);
        enable!(descriptor_binding_storage_texel_buffer_update_after_bind);
        enable!(descriptor_binding_update_unused_while_pending);
        enable!(descriptor_binding_partially_bound);
        enable!(descriptor_binding_variable_descriptor_count);
        enable!(runtime_descriptor_array);
        false
    }

    fn require_feature_extensions(
        &mut self,
        name: &str,
        available_extensions: &HashSet<String>,
        api_version: u32,
    ) -> Result<()> {
        match name {
            "descriptor_indexing" | "bindless_resources" if api_version < vk::API_VERSION_1_2 => {
                self.require_extension(ash::ext::descriptor_indexing::NAME, available_extensions)?
            }
            "timeline_semaphore" | "timeline_semaphores" if api_version < vk::API_VERSION_1_2 => {
                self.require_extension(ash::khr::timeline_semaphore::NAME, available_extensions)?
            }
            "dynamic_rendering" if api_version < vk::API_VERSION_1_3 => {
                self.require_extension(ash::khr::dynamic_rendering::NAME, available_extensions)?
            }
            "synchronization2" if api_version < vk::API_VERSION_1_3 => {
                self.require_extension(ash::khr::synchronization2::NAME, available_extensions)?
            }
            "mesh_shading" | "mesh_shader" | "task_shader" => {
                self.require_extension(ash::ext::mesh_shader::NAME, available_extensions)?
            }
            "ray_tracing" => {
                self.require_extension(
                    ash::khr::acceleration_structure::NAME,
                    available_extensions,
                )?;
                self.require_extension(ash::khr::ray_tracing_pipeline::NAME, available_extensions)?;
                self.require_extension(
                    ash::khr::deferred_host_operations::NAME,
                    available_extensions,
                )?;
            }
            "ray_tracing_pipeline" => {
                self.require_extension(ash::khr::ray_tracing_pipeline::NAME, available_extensions)?;
                self.require_extension(
                    ash::khr::deferred_host_operations::NAME,
                    available_extensions,
                )?;
            }
            "ray_query" => {
                self.require_extension(ash::khr::ray_query::NAME, available_extensions)?;
                self.require_extension(
                    ash::khr::acceleration_structure::NAME,
                    available_extensions,
                )?;
                self.require_extension(
                    ash::khr::deferred_host_operations::NAME,
                    available_extensions,
                )?;
            }
            "ray_tracing_position_fetch" => {
                self.require_extension(
                    ash::khr::ray_tracing_position_fetch::NAME,
                    available_extensions,
                )?;
            }
            "acceleration_structure" => {
                self.require_extension(
                    ash::khr::acceleration_structure::NAME,
                    available_extensions,
                )?;
                self.require_extension(
                    ash::khr::deferred_host_operations::NAME,
                    available_extensions,
                )?;
            }
            "variable_rate_shading"
            | "pipeline_fragment_shading_rate"
            | "primitive_fragment_shading_rate"
            | "attachment_fragment_shading_rate" => {
                self.require_extension(ash::khr::fragment_shading_rate::NAME, available_extensions)?
            }
            "memory_priority" => {
                self.require_extension(ash::ext::memory_priority::NAME, available_extensions)?
            }
            "conditional_rendering" => {
                self.require_extension(ash::ext::conditional_rendering::NAME, available_extensions)?
            }
            "custom_border_color" | "custom_border_colors" => {
                self.require_extension(ash::ext::custom_border_color::NAME, available_extensions)?
            }
            "push_descriptor" | "push_descriptors" => {
                self.require_extension(ash::khr::push_descriptor::NAME, available_extensions)?
            }
            "conservative_rasterization" => self.require_extension(
                ash::ext::conservative_rasterization::NAME,
                available_extensions,
            )?,
            "global_queue_priority" => {
                self.require_extension(ash::khr::global_priority::NAME, available_extensions)?
            }
            "extended_dynamic_state3" => self.require_extension(
                ash::ext::extended_dynamic_state3::NAME,
                available_extensions,
            )?,
            "vertex_input_dynamic_state" => self.require_extension(
                ash::ext::vertex_input_dynamic_state::NAME,
                available_extensions,
            )?,
            "shader_object" => {
                self.require_extension(ash::ext::shader_object::NAME, available_extensions)?
            }
            "optical_flow" | "optical_flow_nv" => {
                self.require_extension(ash::nv::optical_flow::NAME, available_extensions)?
            }
            "graphics_pipeline_library" => self.require_extension(
                ash::ext::graphics_pipeline_library::NAME,
                available_extensions,
            )?,
            "buffer_device_address" if api_version < vk::API_VERSION_1_2 => {
                self.require_extension(ash::khr::buffer_device_address::NAME, available_extensions)?
            }
            _ if is_descriptor_indexing_field(name) && api_version < vk::API_VERSION_1_2 => {
                self.require_extension(ash::ext::descriptor_indexing::NAME, available_extensions)?
            }
            _ => {}
        }
        Ok(())
    }

    fn require_extension(
        &mut self,
        extension: &'static CStr,
        available_extensions: &HashSet<String>,
    ) -> Result<()> {
        let name = extension.to_string_lossy().into_owned();
        if !available_extensions.contains(&name) {
            return Err(Error::InvalidInput(format!(
                "selected Vulkan physical device does not support extension {name} required by requested features"
            )));
        }
        if !self
            .required_extensions
            .iter()
            .any(|existing| *existing == extension)
        {
            self.required_extensions.push(extension);
        }
        Ok(())
    }

    fn rebuild_chain(&mut self) {
        self.features2.p_next = std::ptr::null_mut();
        self.use_feature_chain = false;

        if self.vulkan11.shader_draw_parameters == vk::TRUE {
            push_feature_chain(&mut self.features2, &mut self.vulkan11);
            self.use_feature_chain = true;
        }
        if self.has_descriptor_indexing_features() {
            push_feature_chain(&mut self.features2, &mut self.descriptor_indexing);
            self.use_feature_chain = true;
        }
        if self.timeline.timeline_semaphore == vk::TRUE {
            push_feature_chain(&mut self.features2, &mut self.timeline);
            self.use_feature_chain = true;
        }
        if self.dynamic_rendering.dynamic_rendering == vk::TRUE {
            push_feature_chain(&mut self.features2, &mut self.dynamic_rendering);
            self.use_feature_chain = true;
        }
        if self.synchronization2.synchronization2 == vk::TRUE {
            push_feature_chain(&mut self.features2, &mut self.synchronization2);
            self.use_feature_chain = true;
        }
        if self.buffer_device_address.buffer_device_address == vk::TRUE {
            push_feature_chain(&mut self.features2, &mut self.buffer_device_address);
            self.use_feature_chain = true;
        }
        if self.mesh_shader.mesh_shader == vk::TRUE || self.mesh_shader.task_shader == vk::TRUE {
            push_feature_chain(&mut self.features2, &mut self.mesh_shader);
            self.use_feature_chain = true;
        }
        if self.acceleration_structure.acceleration_structure == vk::TRUE {
            push_feature_chain(&mut self.features2, &mut self.acceleration_structure);
            self.use_feature_chain = true;
        }
        if self.ray_tracing.ray_tracing_pipeline == vk::TRUE {
            push_feature_chain(&mut self.features2, &mut self.ray_tracing);
            self.use_feature_chain = true;
        }
        if self.ray_query.ray_query == vk::TRUE {
            push_feature_chain(&mut self.features2, &mut self.ray_query);
            self.use_feature_chain = true;
        }
        if self.ray_tracing_position_fetch.ray_tracing_position_fetch == vk::TRUE {
            push_feature_chain(&mut self.features2, &mut self.ray_tracing_position_fetch);
            self.use_feature_chain = true;
        }
        if self.fragment_shading_rate.pipeline_fragment_shading_rate == vk::TRUE
            || self.fragment_shading_rate.primitive_fragment_shading_rate == vk::TRUE
            || self.fragment_shading_rate.attachment_fragment_shading_rate == vk::TRUE
        {
            push_feature_chain(&mut self.features2, &mut self.fragment_shading_rate);
            self.use_feature_chain = true;
        }
        if self.memory_priority.memory_priority == vk::TRUE {
            push_feature_chain(&mut self.features2, &mut self.memory_priority);
            self.use_feature_chain = true;
        }
        if self.conditional_rendering.conditional_rendering == vk::TRUE {
            push_feature_chain(&mut self.features2, &mut self.conditional_rendering);
            self.use_feature_chain = true;
        }
        if self.custom_border_color.custom_border_colors == vk::TRUE {
            push_feature_chain(&mut self.features2, &mut self.custom_border_color);
            self.use_feature_chain = true;
        }
        if self
            .extended_dynamic_state3
            .extended_dynamic_state3_polygon_mode
            == vk::TRUE
            || self
                .extended_dynamic_state3
                .extended_dynamic_state3_rasterization_samples
                == vk::TRUE
            || self
                .extended_dynamic_state3
                .extended_dynamic_state3_color_blend_enable
                == vk::TRUE
        {
            push_feature_chain(&mut self.features2, &mut self.extended_dynamic_state3);
            self.use_feature_chain = true;
        }
        if self.vertex_input_dynamic_state.vertex_input_dynamic_state == vk::TRUE {
            push_feature_chain(&mut self.features2, &mut self.vertex_input_dynamic_state);
            self.use_feature_chain = true;
        }
        if self.shader_object.shader_object == vk::TRUE {
            push_feature_chain(&mut self.features2, &mut self.shader_object);
            self.use_feature_chain = true;
        }
        if self.optical_flow.optical_flow == vk::TRUE {
            push_feature_chain(&mut self.features2, &mut self.optical_flow);
            self.use_feature_chain = true;
        }
        if self.graphics_pipeline_library.graphics_pipeline_library == vk::TRUE {
            push_feature_chain(&mut self.features2, &mut self.graphics_pipeline_library);
            self.use_feature_chain = true;
        }
    }

    fn has_descriptor_indexing_features(&self) -> bool {
        let f = &self.descriptor_indexing;
        f.shader_input_attachment_array_dynamic_indexing == vk::TRUE
            || f.shader_uniform_texel_buffer_array_dynamic_indexing == vk::TRUE
            || f.shader_storage_texel_buffer_array_dynamic_indexing == vk::TRUE
            || f.shader_uniform_buffer_array_non_uniform_indexing == vk::TRUE
            || f.shader_sampled_image_array_non_uniform_indexing == vk::TRUE
            || f.shader_storage_buffer_array_non_uniform_indexing == vk::TRUE
            || f.shader_storage_image_array_non_uniform_indexing == vk::TRUE
            || f.shader_input_attachment_array_non_uniform_indexing == vk::TRUE
            || f.shader_uniform_texel_buffer_array_non_uniform_indexing == vk::TRUE
            || f.shader_storage_texel_buffer_array_non_uniform_indexing == vk::TRUE
            || f.descriptor_binding_uniform_buffer_update_after_bind == vk::TRUE
            || f.descriptor_binding_sampled_image_update_after_bind == vk::TRUE
            || f.descriptor_binding_storage_image_update_after_bind == vk::TRUE
            || f.descriptor_binding_storage_buffer_update_after_bind == vk::TRUE
            || f.descriptor_binding_uniform_texel_buffer_update_after_bind == vk::TRUE
            || f.descriptor_binding_storage_texel_buffer_update_after_bind == vk::TRUE
            || f.descriptor_binding_update_unused_while_pending == vk::TRUE
            || f.descriptor_binding_partially_bound == vk::TRUE
            || f.descriptor_binding_variable_descriptor_count == vk::TRUE
            || f.runtime_descriptor_array == vk::TRUE
    }
}

fn push_feature_chain<T>(features2: &mut vk::PhysicalDeviceFeatures2<'_>, next: &mut T) {
    unsafe {
        let next_ptr = (next as *mut T).cast::<c_void>();
        let header = next_ptr.cast::<vk::BaseOutStructure<'_>>();
        (*header).p_next = features2.p_next.cast();
        features2.p_next = next_ptr;
    }
}

fn is_known_feature_name(name: &str) -> bool {
    is_descriptor_indexing_field(name)
        || matches!(
            name,
            "descriptor_indexing"
                | "bindless_resources"
                | "timeline_semaphore"
                | "timeline_semaphores"
                | "dynamic_rendering"
                | "synchronization2"
                | "buffer_device_address"
                | "shader_draw_parameters"
                | "mesh_shading"
                | "mesh_shader"
                | "task_shader"
                | "ray_tracing"
                | "ray_tracing_pipeline"
                | "ray_query"
                | "ray_tracing_position_fetch"
                | "acceleration_structure"
                | "variable_rate_shading"
                | "pipeline_fragment_shading_rate"
                | "primitive_fragment_shading_rate"
                | "attachment_fragment_shading_rate"
                | "memory_priority"
                | "conditional_rendering"
                | "custom_border_color"
                | "custom_border_colors"
                | "push_descriptor"
                | "push_descriptors"
                | "conservative_rasterization"
                | "global_queue_priority"
                | "extended_dynamic_state3"
                | "vertex_input_dynamic_state"
                | "shader_object"
                | "optical_flow"
                | "optical_flow_nv"
                | "graphics_pipeline_library"
        )
}

fn is_descriptor_indexing_field(name: &str) -> bool {
    matches!(
        name,
        "shader_input_attachment_array_dynamic_indexing"
            | "shader_uniform_texel_buffer_array_dynamic_indexing"
            | "shader_storage_texel_buffer_array_dynamic_indexing"
            | "shader_uniform_buffer_array_non_uniform_indexing"
            | "shader_sampled_image_array_non_uniform_indexing"
            | "shader_storage_buffer_array_non_uniform_indexing"
            | "shader_storage_image_array_non_uniform_indexing"
            | "shader_input_attachment_array_non_uniform_indexing"
            | "shader_uniform_texel_buffer_array_non_uniform_indexing"
            | "shader_storage_texel_buffer_array_non_uniform_indexing"
            | "descriptor_binding_uniform_buffer_update_after_bind"
            | "descriptor_binding_sampled_image_update_after_bind"
            | "descriptor_binding_storage_image_update_after_bind"
            | "descriptor_binding_storage_buffer_update_after_bind"
            | "descriptor_binding_uniform_texel_buffer_update_after_bind"
            | "descriptor_binding_storage_texel_buffer_update_after_bind"
            | "descriptor_binding_update_unused_while_pending"
            | "descriptor_binding_partially_bound"
            | "descriptor_binding_variable_descriptor_count"
            | "runtime_descriptor_array"
    )
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn required_device_extensions() -> Vec<&'static CStr> {
    #[cfg(target_os = "macos")]
    {
        vec![ash::khr::swapchain::NAME, c"VK_KHR_portability_subset"]
    }
    #[cfg(not(target_os = "macos"))]
    {
        vec![ash::khr::swapchain::NAME]
    }
}

#[cfg(test)]
#[path = "device_tests.rs"]
mod tests;
