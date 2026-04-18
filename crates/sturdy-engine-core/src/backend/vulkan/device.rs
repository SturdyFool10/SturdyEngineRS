use std::collections::HashSet;
use std::ffi::{CStr, CString, c_void};

use ash::{Device as AshDevice, Instance, vk};

use crate::{AdapterSelection, Error, Result};

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
        Ok(Self {
            physical_device,
            queue_families: QueueFamilyMap::unified(graphics),
        })
    }
}

pub fn create_logical_device(
    instance: &Instance,
    selection: &DeviceSelection,
    config: &VulkanBackendConfig,
) -> Result<LogicalDevice> {
    let priority = [1.0f32];
    let mut unique_families = vec![
        selection.queue_families.graphics,
        selection.queue_families.compute,
        selection.queue_families.transfer,
    ];
    unique_families.sort_unstable();
    unique_families.dedup();
    let queue_info = unique_families
        .iter()
        .map(|family| {
            vk::DeviceQueueCreateInfo::default()
                .queue_family_index(*family)
                .queue_priorities(&priority)
        })
        .collect::<Vec<_>>();
    let mut feature_request = FeatureRequest::resolve(instance, selection.physical_device, config)?;
    let extension_request = ExtensionRequest::resolve(
        instance,
        selection.physical_device,
        config,
        &feature_request.required_extensions,
    )?;
    let device_extension_ptrs = extension_request
        .names
        .iter()
        .map(|extension| extension.as_ptr())
        .collect::<Vec<_>>();
    let device_info_base = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queue_info)
        .enabled_extension_names(&device_extension_ptrs);
    let device_info = feature_request.apply_to(device_info_base);

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
        }
    };

    Ok(LogicalDevice {
        device,
        queue_families: selection.queue_families,
        queues,
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
    descriptor_indexing: vk::PhysicalDeviceDescriptorIndexingFeatures<'a>,
    timeline: vk::PhysicalDeviceTimelineSemaphoreFeatures<'a>,
    dynamic_rendering: vk::PhysicalDeviceDynamicRenderingFeatures<'a>,
    synchronization2: vk::PhysicalDeviceSynchronization2Features<'a>,
    mesh_shader: vk::PhysicalDeviceMeshShaderFeaturesEXT<'a>,
    acceleration_structure: vk::PhysicalDeviceAccelerationStructureFeaturesKHR<'a>,
    ray_tracing: vk::PhysicalDeviceRayTracingPipelineFeaturesKHR<'a>,
    fragment_shading_rate: vk::PhysicalDeviceFragmentShadingRateFeaturesKHR<'a>,
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
            descriptor_indexing: vk::PhysicalDeviceDescriptorIndexingFeatures::default(),
            timeline: vk::PhysicalDeviceTimelineSemaphoreFeatures::default(),
            dynamic_rendering: vk::PhysicalDeviceDynamicRenderingFeatures::default(),
            synchronization2: vk::PhysicalDeviceSynchronization2Features::default(),
            mesh_shader: vk::PhysicalDeviceMeshShaderFeaturesEXT::default(),
            acceleration_structure: vk::PhysicalDeviceAccelerationStructureFeaturesKHR::default(),
            ray_tracing: vk::PhysicalDeviceRayTracingPipelineFeaturesKHR::default(),
            fragment_shading_rate: vk::PhysicalDeviceFragmentShadingRateFeaturesKHR::default(),
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
            request.enable_feature(
                name,
                false,
                &available_core,
                &available_chain,
                &available_extensions,
                properties.api_version,
            )?;
        }

        request.rebuild_chain();
        Ok(request)
    }

    fn apply_to<'a>(&'a mut self, info: vk::DeviceCreateInfo<'a>) -> vk::DeviceCreateInfo<'a> {
        if self.use_feature_chain {
            info.push_next(&mut self.features2)
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
                self.descriptor_indexing
                    .descriptor_binding_sampled_image_update_after_bind = available
                    .descriptor_indexing
                    .descriptor_binding_sampled_image_update_after_bind;
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
        if self.fragment_shading_rate.pipeline_fragment_shading_rate == vk::TRUE
            || self.fragment_shading_rate.primitive_fragment_shading_rate == vk::TRUE
            || self.fragment_shading_rate.attachment_fragment_shading_rate == vk::TRUE
        {
            push_feature_chain(&mut self.features2, &mut self.fragment_shading_rate);
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
                | "mesh_shading"
                | "mesh_shader"
                | "task_shader"
                | "ray_tracing"
                | "ray_tracing_pipeline"
                | "acceleration_structure"
                | "variable_rate_shading"
                | "pipeline_fragment_shading_rate"
                | "primitive_fragment_shading_rate"
                | "attachment_fragment_shading_rate"
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
        vec![
            ash::khr::swapchain::NAME,
            CStr::from_bytes_with_nul(b"VK_KHR_portability_subset\0")
                .expect("static extension name has nul terminator"),
        ]
    }
    #[cfg(not(target_os = "macos"))]
    {
        vec![ash::khr::swapchain::NAME]
    }
}
