mod adapter;
mod alias_heaps;
mod allocator;
pub(crate) mod bindless;
mod caps;
mod commands;
mod config;
mod debug;
mod descriptors;
mod device;
mod instance;
mod pipelines;
mod queues;
mod resources;
mod shaders;
mod surfaces;

use std::collections::HashMap;

use ash::{Device as AshDevice, Entry, Instance, vk};
use std::sync::{Mutex, RwLock};
use std::{fs, path::PathBuf};

use crate::backend::{Backend, BackendKind};
use crate::{
    AccelerationStructureBuildMode, AccelerationStructureBuildSizes, AccelerationStructureDesc,
    AccelerationStructureHandle, AccelerationStructureKind, AdapterInfo, AntiLagMode,
    BindGroupDesc, BindGroupHandle, BlasBuildDesc, BorderColor, BufferDesc, BufferHandle,
    CanonicalPipelineLayout, Caps, CompiledGraph, ComputePipelineDesc, Error, ExternalBufferDesc,
    ExternalBufferHandle, ExternalImageDesc, ExternalImageHandle, FilterMode, Format,
    FormatCapabilities, GraphicsPipelineDesc, HdrMetadata, ImageDesc, ImageHandle, IndexFormat,
    LatencyMode, NativeSurfaceDesc, PipelineHandle, PipelineLayoutHandle, RayTracingPipelineDesc,
    ReflexMode, Result, SamplerDesc, SamplerHandle, SamplerReductionMode,
    ShaderBindingTableProperties, ShaderDesc, ShaderHandle, SubmissionHandle, SurfaceCapabilities,
    SurfaceHandle, SurfaceInfo, SurfaceRecreateDesc, SurfaceSize, TlasBuildDesc, VertexFormat,
    VideoSessionDesc, VideoSessionHandle,
};

pub use bindless::BindlessVkInfo;
pub use config::VulkanBackendConfig;
use device::{DeviceSelection, create_logical_device};
use instance::{create_instance, load_entry};
use queues::{QueueFamilyMap, VulkanQueues};

pub const KIND: BackendKind = BackendKind::Vulkan;

pub struct VulkanBackend {
    _entry: Entry,
    instance: Instance,
    physical_device: vk::PhysicalDevice,
    device: AshDevice,
    queue_families: QueueFamilyMap,
    queues: VulkanQueues,
    caps: Caps,
    debug: debug::DebugUtils,
    commands: Mutex<commands::FramedCommands>,
    descriptors: RwLock<descriptors::DescriptorRegistry>,
    pipelines: Mutex<pipelines::PipelineRegistry>,
    resources: RwLock<resources::ResourceRegistry>,
    shaders: Mutex<shaders::ShaderRegistry>,
    surfaces: Mutex<surfaces::SurfaceRegistry>,
    /// Persistent alias heaps: one `VkDeviceMemory` per alias slot, reused each frame.
    alias_heaps: Mutex<alias_heaps::AliasHeapRegistry>,
    /// Surface whose image was most recently acquired; cleared after present.
    active_surface: Mutex<Option<SurfaceHandle>>,
    /// Global bindless descriptor heap. `None` when `Caps::supports_bindless` is false.
    bindless_heap: Option<bindless::BindlessHeap>,
    /// VK_EXT_mesh_shader commands. Present only when the mesh shader feature was enabled.
    mesh_shader_ext: Option<ash::ext::mesh_shader::Device>,
    /// VK_KHR_synchronization2 commands. Present when sync2 is enabled.
    synchronization2_khr: Option<ash::khr::synchronization2::Device>,
    /// VK_KHR_dynamic_rendering commands. Present when dynamic rendering is enabled.
    dynamic_rendering_khr: Option<ash::khr::dynamic_rendering::Device>,
    /// VK_EXT_device_fault commands. Present when device fault extension is available.
    device_fault_ext: Option<ash::ext::device_fault::Device>,
    /// VK_KHR_push_descriptor commands. Present when push_descriptors is enabled.
    push_descriptor_khr: Option<ash::khr::push_descriptor::Device>,
    /// VK_EXT_conditional_rendering commands. Present when conditional_rendering is enabled.
    conditional_rendering_ext: Option<ash::ext::conditional_rendering::Device>,
    /// VK_KHR_fragment_shading_rate commands. Present when any VRS tier is enabled.
    fragment_shading_rate_khr: Option<ash::khr::fragment_shading_rate::Device>,
    /// Whether VK_EXT_conservative_rasterization is available.
    conservative_rasterization_enabled: bool,
    /// VK_KHR_acceleration_structure commands. Present when AS is enabled.
    acceleration_structure_khr: Option<ash::khr::acceleration_structure::Device>,
    /// VK_KHR_ray_tracing_pipeline commands. Present when RT pipeline is enabled.
    ray_tracing_pipeline_khr: Option<ash::khr::ray_tracing_pipeline::Device>,
    ray_tracing_sbt_properties: Option<ShaderBindingTableProperties>,
    /// VK_NV_low_latency2 commands. Present when reflex is available.
    reflex_nv: Option<ash::nv::low_latency2::Device>,
    /// Desired Reflex mode; applied per-swapchain during flush when a swapchain is active.
    reflex_mode: std::sync::atomic::AtomicU8,
    /// VK_EXT_hdr_metadata commands. Present when hdr_output is available.
    hdr_metadata_ext: Option<ash::ext::hdr_metadata::Device>,
    /// VK_EXT_extended_dynamic_state3 commands. Present when the feature is enabled.
    extended_dynamic_state3_ext: Option<ash::ext::extended_dynamic_state3::Device>,
    /// VK_EXT_vertex_input_dynamic_state commands. Present when the feature is enabled.
    vertex_input_dynamic_state_ext: Option<ash::ext::vertex_input_dynamic_state::Device>,
    /// VK_EXT_shader_object commands. Present when the feature is enabled.
    shader_object_ext: Option<ash::ext::shader_object::Device>,
}

impl VulkanBackend {
    /// Enumerate all Vulkan physical adapters without creating a logical device.
    pub fn enumerate_adapters(config: &VulkanBackendConfig) -> Result<Vec<AdapterInfo>> {
        let entry = load_entry()?;
        let instance = create_instance(&entry, config)?;
        let adapters = adapter::enumerate(&instance);
        unsafe { instance.destroy_instance(None) };
        Ok(adapters)
    }

    pub fn create(config: VulkanBackendConfig) -> Result<Self> {
        let entry = load_entry()?;
        let instance = create_instance(&entry, &config)?;
        let selection = DeviceSelection::pick(&instance, &config.adapter_selection)?;
        let logical = create_logical_device(&instance, &selection, &config)?;
        let mut caps = caps::query_caps(&instance, selection.physical_device);
        // Override feature flags with what was actually enabled in the logical device.
        caps.features.synchronization2 = logical.synchronization2_enabled;
        caps.features.dynamic_rendering = logical.dynamic_rendering_enabled;
        caps.features.timeline_semaphores = logical.timeline_semaphores_enabled;
        caps.features.buffer_device_address = logical.buffer_device_address_enabled;
        caps.features.memory_priority = logical.memory_priority_enabled;
        caps.features.push_descriptors = logical.push_descriptors_enabled;
        caps.features.conditional_rendering = logical.conditional_rendering_enabled;
        caps.features.custom_border_color = logical.custom_border_color_enabled;
        caps.features.vrs_pipeline = logical.vrs_pipeline_enabled;
        caps.features.vrs_primitive = logical.vrs_primitive_enabled;
        caps.features.vrs_attachment = logical.vrs_attachment_enabled;
        caps.features.variable_rate_shading = logical.vrs_pipeline_enabled
            || logical.vrs_primitive_enabled
            || logical.vrs_attachment_enabled;
        caps.features.global_queue_priority = logical.global_queue_priority_enabled;
        caps.features.ray_tracing = logical.ray_tracing_pipeline_enabled;
        caps.features.ray_query = logical.ray_query_enabled;
        caps.features.ray_tracing_position_fetch = logical.ray_tracing_position_fetch_enabled;
        caps.features.conservative_rasterization_overestimate = logical
            .conservative_rasterization_enabled
            && caps.features.conservative_rasterization_overestimate;
        caps.features.conservative_rasterization_underestimate = logical
            .conservative_rasterization_enabled
            && caps.features.conservative_rasterization_underestimate;
        caps.features.extended_dynamic_state3 =
            logical.extended_dynamic_state3_enabled && caps.features.extended_dynamic_state3;
        caps.features.extended_dynamic_state3_polygon_mode = logical
            .extended_dynamic_state3_enabled
            && caps.features.extended_dynamic_state3_polygon_mode;
        caps.features.extended_dynamic_state3_color_blend = logical.extended_dynamic_state3_enabled
            && caps.features.extended_dynamic_state3_color_blend;
        caps.features.vertex_input_dynamic_state = logical.vertex_input_dynamic_state_enabled;
        caps.features.shader_object = logical.shader_object_enabled && caps.features.shader_object;
        let props = unsafe { instance.get_physical_device_properties(selection.physical_device) };
        let timestamp_period_ns = props.limits.timestamp_period;
        let memory_properties =
            unsafe { instance.get_physical_device_memory_properties(selection.physical_device) };
        let resource_registry = resources::ResourceRegistry::new(memory_properties);
        let commands = commands::FramedCommands::create(
            &logical.device,
            logical.queue_families,
            timestamp_period_ns,
        )?;
        let cache_data = load_pipeline_cache_file();
        let mut pipeline_registry =
            pipelines::PipelineRegistry::create(&logical.device, cache_data.as_deref())?;
        pipeline_registry.dynamic_rendering_enabled = logical.dynamic_rendering_enabled;
        pipeline_registry.vrs_pipeline_enabled = caps.features.vrs_pipeline;
        pipeline_registry.conservative_rasterization_overestimate_enabled =
            caps.features.conservative_rasterization_overestimate;
        pipeline_registry.conservative_rasterization_underestimate_enabled =
            caps.features.conservative_rasterization_underestimate;

        let debug_utils = debug::DebugUtils::new(&instance, &logical.device);
        let mesh_shader_ext = if logical.mesh_shader_enabled {
            Some(ash::ext::mesh_shader::Device::new(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let synchronization2_khr = if logical.synchronization2_enabled {
            Some(ash::khr::synchronization2::Device::new(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let dynamic_rendering_khr = if logical.dynamic_rendering_enabled {
            Some(ash::khr::dynamic_rendering::Device::new(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let device_fault_ext = if caps.features.device_fault {
            Some(ash::ext::device_fault::Device::new(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let push_descriptor_khr = if logical.push_descriptors_enabled {
            Some(ash::khr::push_descriptor::Device::new(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let conditional_rendering_ext = if logical.conditional_rendering_enabled {
            Some(ash::ext::conditional_rendering::Device::new(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let fragment_shading_rate_khr = if logical.vrs_pipeline_enabled
            || logical.vrs_primitive_enabled
            || logical.vrs_attachment_enabled
        {
            Some(ash::khr::fragment_shading_rate::Device::new(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let conservative_rasterization_enabled = logical.conservative_rasterization_enabled;
        let acceleration_structure_khr = if logical.acceleration_structure_enabled {
            Some(ash::khr::acceleration_structure::Device::new(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let ray_tracing_pipeline_khr = if logical.ray_tracing_pipeline_enabled {
            Some(ash::khr::ray_tracing_pipeline::Device::new(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let ray_tracing_sbt_properties = logical
            .ray_tracing_pipeline_enabled
            .then(|| query_ray_tracing_sbt_properties(&instance, selection.physical_device));
        let reflex_nv = if caps.features.reflex {
            Some(ash::nv::low_latency2::Device::new(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let hdr_metadata_ext = if caps.features.hdr_output {
            Some(ash::ext::hdr_metadata::Device::new(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let extended_dynamic_state3_ext = if logical.extended_dynamic_state3_enabled {
            Some(ash::ext::extended_dynamic_state3::Device::new(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let vertex_input_dynamic_state_ext = if logical.vertex_input_dynamic_state_enabled {
            Some(ash::ext::vertex_input_dynamic_state::Device::new(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let shader_object_ext = if logical.shader_object_enabled {
            Some(ash::ext::shader_object::Device::new(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };

        // Create the bindless heap if the device supports descriptor_indexing.
        let bindless_heap = if caps.supports_bindless {
            match bindless::BindlessHeap::create(&logical.device) {
                Ok(heap) => Some(heap),
                Err(e) => {
                    eprintln!(
                        "[SturdyEngine] bindless heap creation failed (grouped-descriptor fallback): {e}"
                    );
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            _entry: entry,
            instance,
            physical_device: selection.physical_device,
            device: logical.device,
            queue_families: logical.queue_families,
            queues: logical.queues,
            caps,
            debug: debug_utils,
            commands: Mutex::new(commands),
            descriptors: RwLock::new(descriptors::DescriptorRegistry::default()),
            pipelines: Mutex::new(pipeline_registry),
            resources: RwLock::new(resource_registry),
            shaders: Mutex::new(shaders::ShaderRegistry::default()),
            surfaces: Mutex::new(surfaces::SurfaceRegistry::default()),
            alias_heaps: Mutex::new(alias_heaps::AliasHeapRegistry::default()),
            active_surface: Mutex::new(None),
            bindless_heap,
            mesh_shader_ext,
            synchronization2_khr,
            dynamic_rendering_khr,
            device_fault_ext,
            push_descriptor_khr,
            conditional_rendering_ext,
            fragment_shading_rate_khr,
            conservative_rasterization_enabled,
            acceleration_structure_khr,
            ray_tracing_pipeline_khr,
            ray_tracing_sbt_properties,
            reflex_nv,
            reflex_mode: std::sync::atomic::AtomicU8::new(0),
            hdr_metadata_ext,
            extended_dynamic_state3_ext,
            vertex_input_dynamic_state_ext,
            shader_object_ext,
        })
    }

    pub fn physical_device_name(&self) -> String {
        device::physical_device_name(&self.instance, self.physical_device)
    }

    pub fn graphics_queue_family(&self) -> u32 {
        self.queue_families.graphics
    }

    // ── Bindless registration ─────────────────────────────────────────────────

    /// Register a sampled image in the bindless heap.
    ///
    /// Returns the stable `u32` index to embed in push constants or a draw-data
    /// buffer. Returns `None` when bindless is unsupported or capacity is full.
    pub fn register_bindless_sampled_image(&self, handle: ImageHandle) -> Option<u32> {
        let heap = self.bindless_heap.as_ref()?;
        let resources = self.resources.read().ok()?;
        let view = resources.image_view(handle).ok()?;
        heap.register_sampled_image(view)
    }

    /// Register a sampler in the bindless heap.
    pub fn register_bindless_sampler(&self, handle: SamplerHandle) -> Option<u32> {
        let heap = self.bindless_heap.as_ref()?;
        let resources = self.resources.read().ok()?;
        let sampler = resources.sampler(handle).ok()?;
        heap.register_sampler(sampler)
    }

    /// Register a storage image in the bindless heap.
    pub fn register_bindless_storage_image(&self, handle: ImageHandle) -> Option<u32> {
        let heap = self.bindless_heap.as_ref()?;
        let resources = self.resources.read().ok()?;
        let view = resources.image_view(handle).ok()?;
        heap.register_storage_image(view)
    }

    /// Register a storage buffer in the bindless heap.
    pub fn register_bindless_storage_buffer(&self, handle: BufferHandle) -> Option<u32> {
        let heap = self.bindless_heap.as_ref()?;
        let resources = self.resources.read().ok()?;
        let buf = resources.buffer(handle).ok()?;
        // VK_WHOLE_SIZE (u64::MAX) means "bind the full buffer from offset 0".
        heap.register_storage_buffer(buf, 0, u64::MAX)
    }

    /// Returns Vulkan-level info about the bindless heap for command binding.
    ///
    /// Used by the command recording layer to bind set 0 before draw calls
    /// that use bindless resources. `None` when not supported.
    pub fn bindless_vk_info(&self) -> Option<BindlessVkInfo> {
        let heap = self.bindless_heap.as_ref()?;
        Some(BindlessVkInfo {
            set: heap.set,
            set_layout: heap.set_layout,
        })
    }

    pub fn bindless_supported(&self) -> bool {
        self.bindless_heap.is_some()
    }
}

impl Backend for VulkanBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Vulkan
    }

    fn adapter_name(&self) -> Option<String> {
        Some(self.physical_device_name())
    }

    fn caps(&self) -> Caps {
        self.caps.clone()
    }

    fn format_capabilities(&self, format: Format) -> FormatCapabilities {
        caps::query_format_capabilities(&self.instance, self.physical_device, format)
    }

    fn memory_budget(&self) -> Option<crate::GpuMemoryBudget> {
        //panic allowed, reason = "poisoned vulkan resource registry is unrecoverable"
        let stats = self
            .resources
            .read()
            .expect("vulkan resource registry rwlock poisoned")
            .allocator_stats();
        Some(crate::GpuMemoryBudget {
            device_local_used_bytes: stats.device_local_used_bytes,
            device_local_capacity_bytes: stats.device_local_capacity_bytes,
            host_visible_used_bytes: stats.host_visible_used_bytes,
            host_visible_capacity_bytes: stats.host_visible_capacity_bytes,
            block_count: stats.block_count,
        })
    }

    fn register_bindless_sampled_image(&self, handle: ImageHandle) -> Option<u32> {
        VulkanBackend::register_bindless_sampled_image(self, handle)
    }

    fn register_bindless_sampler(&self, handle: SamplerHandle) -> Option<u32> {
        VulkanBackend::register_bindless_sampler(self, handle)
    }

    fn register_bindless_storage_image(&self, handle: ImageHandle) -> Option<u32> {
        VulkanBackend::register_bindless_storage_image(self, handle)
    }

    fn register_bindless_storage_buffer(&self, handle: BufferHandle) -> Option<u32> {
        VulkanBackend::register_bindless_storage_buffer(self, handle)
    }

    fn bindless_supported(&self) -> bool {
        VulkanBackend::bindless_supported(self)
    }

    fn create_image(&self, handle: ImageHandle, desc: ImageDesc) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.resources
            .write()
            .expect("vulkan resource registry rwlock poisoned")
            .create_image(&self.device, handle, desc)
    }

    unsafe fn import_external_image(
        &self,
        handle: ImageHandle,
        desc: ExternalImageDesc,
    ) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        match desc.handle {
            ExternalImageHandle::Vulkan(external) => self
                .resources
                .write()
                .expect("vulkan resource registry rwlock poisoned")
                .import_external_image(handle, external, desc.desc),
        }
    }

    fn create_transient_image(&self, handle: ImageHandle, desc: ImageDesc) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.resources
            .write()
            .expect("vulkan resource registry rwlock poisoned")
            .create_image_unbound(&self.device, handle, desc)
    }

    fn destroy_image(&self, handle: ImageHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut resources = self
            .resources
            .write()
            .expect("vulkan resource registry rwlock poisoned");
        let view = resources.image_view(handle)?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.pipelines
            .lock()
            .expect("vulkan pipeline registry mutex poisoned")
            .invalidate_framebuffers_for_view(&self.device, view);
        resources.destroy_image(&self.device, handle)
    }

    fn create_buffer(&self, handle: BufferHandle, desc: BufferDesc) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.resources
            .write()
            .expect("vulkan resource registry rwlock poisoned")
            .create_buffer(&self.device, handle, desc)
    }

    fn buffer_device_address(&self, handle: BufferHandle) -> Result<Option<u64>> {
        let resources = self
            .resources
            .read()
            .expect("vulkan resource registry rwlock poisoned");
        Ok(Some(
            resources.buffer_device_address_raw(&self.device, handle)?,
        ))
    }

    unsafe fn import_external_buffer(
        &self,
        handle: BufferHandle,
        desc: ExternalBufferDesc,
    ) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        match desc.handle {
            ExternalBufferHandle::Vulkan(external) => self
                .resources
                .write()
                .expect("vulkan resource registry rwlock poisoned")
                .import_external_buffer(handle, external),
        }
    }

    fn destroy_buffer(&self, handle: BufferHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.resources
            .write()
            .expect("vulkan resource registry rwlock poisoned")
            .destroy_buffer(&self.device, handle)
    }

    fn create_acceleration_structure(
        &self,
        handle: AccelerationStructureHandle,
        desc: AccelerationStructureDesc,
    ) -> Result<()> {
        let as_ext = self.acceleration_structure_khr.as_ref().ok_or_else(|| {
            Error::Unsupported("VK_KHR_acceleration_structure is not enabled".into())
        })?;
        let ty = match desc.kind {
            AccelerationStructureKind::BottomLevel => {
                vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL
            }
            AccelerationStructureKind::TopLevel => vk::AccelerationStructureTypeKHR::TOP_LEVEL,
        };
        self.resources
            .write()
            .expect("vulkan resource registry rwlock poisoned")
            .create_acceleration_structure(&self.device, handle, desc.size, as_ext, ty)
    }

    fn destroy_acceleration_structure(&self, handle: AccelerationStructureHandle) -> Result<()> {
        let as_ext = self.acceleration_structure_khr.as_ref().ok_or_else(|| {
            Error::Unsupported("VK_KHR_acceleration_structure is not enabled".into())
        })?;
        self.resources
            .write()
            .expect("vulkan resource registry rwlock poisoned")
            .destroy_acceleration_structure(&self.device, handle, as_ext)
    }

    fn blas_build_sizes(&self, desc: &BlasBuildDesc) -> Result<AccelerationStructureBuildSizes> {
        let as_ext = self.acceleration_structure_khr.as_ref().ok_or_else(|| {
            Error::Unsupported("VK_KHR_acceleration_structure is not enabled".into())
        })?;
        query_blas_build_sizes(as_ext, desc)
    }

    fn tlas_build_sizes(&self, desc: &TlasBuildDesc) -> Result<AccelerationStructureBuildSizes> {
        let as_ext = self.acceleration_structure_khr.as_ref().ok_or_else(|| {
            Error::Unsupported("VK_KHR_acceleration_structure is not enabled".into())
        })?;
        query_tlas_build_sizes(as_ext, desc)
    }

    fn create_sampler(&self, handle: SamplerHandle, desc: SamplerDesc) -> Result<()> {
        if (desc.mag_filter == FilterMode::Cubic || desc.min_filter == FilterMode::Cubic)
            && !self.caps.features.filter_cubic
        {
            return Err(Error::Unsupported(
                "cubic sampler filtering requires VK_EXT_filter_cubic".into(),
            ));
        }
        if matches!(desc.border_color, BorderColor::Custom(_))
            && !self.caps.features.custom_border_color
        {
            return Err(Error::Unsupported(
                "custom sampler border colors require VK_EXT_custom_border_color".into(),
            ));
        }
        if desc.reduction_mode != SamplerReductionMode::WeightedAverage
            && !self.caps.features.sampler_filter_minmax
        {
            return Err(Error::Unsupported(
                "sampler min/max reduction requires VK_EXT_sampler_filter_minmax".into(),
            ));
        }
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.resources
            .write()
            .expect("vulkan resource registry rwlock poisoned")
            .create_sampler(&self.device, handle, desc)
    }

    fn destroy_sampler(&self, handle: SamplerHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.resources
            .write()
            .expect("vulkan resource registry rwlock poisoned")
            .destroy_sampler(&self.device, handle)
    }

    fn write_buffer(&self, handle: BufferHandle, offset: u64, data: &[u8]) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.resources
            .write()
            .expect("vulkan resource registry rwlock poisoned")
            .write_buffer(handle, offset, data)
    }

    fn read_buffer(&self, handle: BufferHandle, offset: u64, out: &mut [u8]) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.resources
            .read()
            .expect("vulkan resource registry rwlock poisoned")
            .read_buffer(handle, offset, out)
    }

    fn create_shader(&self, handle: ShaderHandle, desc: &ShaderDesc) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shaders
            .lock()
            .expect("vulkan shader registry mutex poisoned")
            .create_shader(&self.device, handle, desc)
    }

    fn destroy_shader(&self, handle: ShaderHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shaders
            .lock()
            .expect("vulkan shader registry mutex poisoned")
            .destroy_shader(&self.device, handle)
    }

    fn create_pipeline_layout(
        &self,
        handle: PipelineLayoutHandle,
        layout: &CanonicalPipelineLayout,
    ) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.descriptors
            .write()
            .expect("vulkan descriptor registry rwlock poisoned")
            .create_pipeline_layout(
                &self.device,
                handle,
                layout,
                self.bindless_vk_info().map(|info| info.set_layout),
                &self.caps.limits,
            )
    }

    fn destroy_pipeline_layout(&self, handle: PipelineLayoutHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.descriptors
            .write()
            .expect("vulkan descriptor registry rwlock poisoned")
            .destroy_pipeline_layout(&self.device, handle)
    }

    fn create_bind_group(&self, handle: BindGroupHandle, desc: &BindGroupDesc) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let resources = self
            .resources
            .read()
            .expect("vulkan resource registry rwlock poisoned");
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.descriptors
            .write()
            .expect("vulkan descriptor registry rwlock poisoned")
            .create_bind_group(&self.device, handle, desc, &resources)
    }

    fn destroy_bind_group(&self, handle: BindGroupHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.descriptors
            .write()
            .expect("vulkan descriptor registry rwlock poisoned")
            .destroy_bind_group(&self.device, handle)
    }

    fn create_compute_pipeline(
        &self,
        handle: PipelineHandle,
        desc: ComputePipelineDesc,
    ) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let shaders = self
            .shaders
            .lock()
            .expect("vulkan shader registry mutex poisoned");
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let descriptors = self
            .descriptors
            .read()
            .expect("vulkan descriptor registry rwlock poisoned");
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.pipelines
            .lock()
            .expect("vulkan pipeline registry mutex poisoned")
            .create_compute_pipeline(&self.device, handle, desc, &shaders, &descriptors)
    }

    fn create_graphics_pipeline(
        &self,
        handle: PipelineHandle,
        desc: &GraphicsPipelineDesc,
    ) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let shaders = self
            .shaders
            .lock()
            .expect("vulkan shader registry mutex poisoned");
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let descriptors = self
            .descriptors
            .read()
            .expect("vulkan descriptor registry rwlock poisoned");
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.pipelines
            .lock()
            .expect("vulkan pipeline registry mutex poisoned")
            .create_graphics_pipeline(&self.device, handle, desc, &shaders, &descriptors)
    }

    fn destroy_pipeline(&self, handle: PipelineHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.pipelines
            .lock()
            .expect("vulkan pipeline registry mutex poisoned")
            .destroy_pipeline(&self.device, handle)
    }

    fn create_ray_tracing_pipeline(
        &self,
        handle: PipelineHandle,
        desc: &RayTracingPipelineDesc,
    ) -> Result<()> {
        let rt_ext = self.ray_tracing_pipeline_khr.as_ref().ok_or_else(|| {
            Error::Unsupported("VK_KHR_ray_tracing_pipeline is not enabled".into())
        })?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut pipelines = self
            .pipelines
            .lock()
            .expect("vulkan pipeline registry mutex poisoned");
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let shaders = self
            .shaders
            .lock()
            .expect("vulkan shader registry mutex poisoned");
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let descriptors = self
            .descriptors
            .read()
            .expect("vulkan descriptor registry rwlock poisoned");
        pipelines.create_ray_tracing_pipeline(
            &self.device,
            handle,
            desc,
            &shaders,
            &descriptors,
            rt_ext,
        )
    }

    fn shader_binding_table_properties(&self) -> Result<ShaderBindingTableProperties> {
        self.ray_tracing_sbt_properties
            .ok_or_else(|| Error::Unsupported("VK_KHR_ray_tracing_pipeline is not enabled".into()))
    }

    fn ray_tracing_shader_group_handles(
        &self,
        pipeline: PipelineHandle,
        first_group: u32,
        group_count: u32,
    ) -> Result<Vec<u8>> {
        let rt_ext = self.ray_tracing_pipeline_khr.as_ref().ok_or_else(|| {
            Error::Unsupported("VK_KHR_ray_tracing_pipeline is not enabled".into())
        })?;
        let props = self.shader_binding_table_properties()?;
        let pipeline = self
            .pipelines
            .lock()
            .expect("vulkan pipeline registry mutex poisoned")
            .pipeline(pipeline)?;
        if pipeline.bind_point != vk::PipelineBindPoint::RAY_TRACING_KHR {
            return Err(Error::InvalidInput(
                "shader group handles require a ray-tracing pipeline".into(),
            ));
        }
        let data_size = props
            .shader_group_handle_size
            .checked_mul(group_count)
            .ok_or_else(|| {
                Error::InvalidInput("ray-tracing shader group handle size overflowed".into())
            })? as usize;
        unsafe {
            rt_ext
                .get_ray_tracing_shader_group_handles(
                    pipeline.pipeline,
                    first_group,
                    group_count,
                    data_size,
                )
                .map_err(|error| {
                    Error::Backend(format!(
                        "vkGetRayTracingShaderGroupHandlesKHR failed: {error:?}"
                    ))
                })
        }
    }

    fn create_surface(
        &self,
        handle: SurfaceHandle,
        desc: NativeSurfaceDesc,
    ) -> Result<SurfaceInfo> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.surfaces
            .lock()
            .expect("vulkan surface registry mutex poisoned")
            .create_surface(
                &self._entry,
                &self.instance,
                &self.device,
                self.physical_device,
                self.queue_families.graphics,
                handle,
                desc,
                self.caps.features.timeline_semaphores,
            )
    }

    fn resize_surface(&self, handle: SurfaceHandle, size: SurfaceSize) -> Result<SurfaceInfo> {
        // Wait only on submitted frames, not all GPU work — avoids vkDeviceWaitIdle stall.
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.commands
            .lock()
            .expect("vulkan command context mutex poisoned")
            .wait_all(&self.device)?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.pipelines
            .lock()
            .expect("vulkan pipeline registry mutex poisoned")
            .clear_all_framebuffers(&self.device);
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.surfaces
            .lock()
            .expect("vulkan surface registry mutex poisoned")
            .resize_surface(&self.device, self.physical_device, handle, size)
    }

    fn recreate_surface(
        &self,
        handle: SurfaceHandle,
        desc: SurfaceRecreateDesc,
        _current: SurfaceInfo,
    ) -> Result<SurfaceInfo> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.commands
            .lock()
            .expect("vulkan command context mutex poisoned")
            .wait_all(&self.device)?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.pipelines
            .lock()
            .expect("vulkan pipeline registry mutex poisoned")
            .clear_all_framebuffers(&self.device);
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.surfaces
            .lock()
            .expect("vulkan surface registry mutex poisoned")
            .recreate_surface(&self.device, self.physical_device, handle, desc)
    }

    fn destroy_surface(&self, handle: SurfaceHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.commands
            .lock()
            .expect("vulkan command context mutex poisoned")
            .wait_all(&self.device)?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        *self
            .active_surface
            .lock()
            .expect("vulkan active surface mutex poisoned") = None;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.pipelines
            .lock()
            .expect("vulkan pipeline registry mutex poisoned")
            .clear_all_framebuffers(&self.device);
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.surfaces
            .lock()
            .expect("vulkan surface registry mutex poisoned")
            .destroy_surface(&self.device, handle)?;
        Ok(())
    }

    fn query_surface_capabilities(&self, handle: SurfaceHandle) -> Result<SurfaceCapabilities> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.surfaces
            .lock()
            .expect("vulkan surface registry mutex poisoned")
            .query_surface_capabilities(self.physical_device, handle)
    }

    fn set_image_debug_name(&self, handle: ImageHandle, name: &str) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        if let Ok(image) = self
            .resources
            .read()
            .expect("vulkan resource registry rwlock poisoned")
            .image(handle)
        {
            self.debug.set_name(&self.device, image, name);
        }
    }

    fn set_buffer_debug_name(&self, handle: BufferHandle, name: &str) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        if let Ok(buffer) = self
            .resources
            .read()
            .expect("vulkan resource registry rwlock poisoned")
            .buffer(handle)
        {
            self.debug.set_name(&self.device, buffer, name);
        }
    }

    fn set_pipeline_debug_name(&self, handle: PipelineHandle, name: &str) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        if let Ok(pipeline) = self
            .pipelines
            .lock()
            .expect("vulkan pipeline registry mutex poisoned")
            .pipeline(handle)
        {
            self.debug.set_name(&self.device, pipeline.pipeline, name);
        }
    }

    fn acquire_surface_image(
        &self,
        surface: SurfaceHandle,
        image: ImageHandle,
    ) -> Result<(ImageDesc, u64)> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let acquired = self
            .surfaces
            .lock()
            .expect("vulkan surface registry mutex poisoned")
            .acquire_image(surface)?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.resources
            .write()
            .expect("vulkan resource registry rwlock poisoned")
            .import_image(image, acquired.image, acquired.image_view, acquired.desc)?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        *self
            .active_surface
            .lock()
            .expect("vulkan active surface mutex poisoned") = Some(surface);
        Ok((acquired.desc, acquired.image_index as u64))
    }

    fn present_surface(&self, surface: SurfaceHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let result = self
            .surfaces
            .lock()
            .expect("vulkan surface registry mutex poisoned")
            .present(self.queues.graphics, surface);
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        *self
            .active_surface
            .lock()
            .expect("vulkan active surface mutex poisoned") = None;
        result
    }

    fn flush(&self, graph: &CompiledGraph) -> Result<SubmissionHandle> {
        // Resolve per-surface semaphores if a swapchain image was acquired.
        let (wait_sem, signal_sem) = {
            //panic allowed, reason = "poisoned mutex is unrecoverable"
            let active = *self
                .active_surface
                .lock()
                .expect("vulkan active surface mutex poisoned");
            if let Some(sh) = active {
                //panic allowed, reason = "poisoned mutex is unrecoverable"
                let sems = self
                    .surfaces
                    .lock()
                    .expect("vulkan surface registry mutex poisoned")
                    .frame_semaphores(sh)?;
                (Some(sems.0), Some(sems.1))
            } else {
                (None, None)
            }
        };

        // Bind transient images to alias heap memories before recording begins.
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        bind_transient_images_to_alias_heaps(
            &self.device,
            &self.instance,
            self.physical_device,
            &mut self
                .resources
                .write()
                .expect("vulkan resource registry rwlock poisoned"),
            &mut self
                .alias_heaps
                .lock()
                .expect("vulkan alias heap registry mutex poisoned"),
            graph,
        )?;

        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut resources = self
            .resources
            .write()
            .expect("vulkan resource registry rwlock poisoned");
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let descriptors = self
            .descriptors
            .read()
            .expect("vulkan descriptor registry rwlock poisoned");
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut pipelines = self
            .pipelines
            .lock()
            .expect("vulkan pipeline registry mutex poisoned");
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut commands = self
            .commands
            .lock()
            .expect("vulkan command context mutex poisoned");
        let submit_result = commands.submit(
            &self.device,
            self.queues,
            self.queue_families,
            graph,
            &mut resources,
            &descriptors,
            &mut pipelines,
            &self.debug,
            self.bindless_vk_info(),
            self.mesh_shader_ext.as_ref(),
            self.synchronization2_khr.as_ref(),
            self.dynamic_rendering_khr.as_ref(),
            self.push_descriptor_khr.as_ref(),
            self.conditional_rendering_ext.as_ref(),
            self.fragment_shading_rate_khr.as_ref(),
            self.acceleration_structure_khr.as_ref(),
            self.ray_tracing_pipeline_khr.as_ref(),
            self.shader_object_ext.as_ref(),
            self.caps.features.ray_tracing_position_fetch,
            wait_sem,
            signal_sem,
        );
        let handle = match submit_result {
            Ok(h) => h,
            Err(Error::DeviceLost(msg)) => {
                // Attempt to enrich with VK_EXT_device_fault breadcrumbs.
                if let Some(fault_ext) = self.device_fault_ext.as_ref() {
                    let enriched = gather_device_fault_info(fault_ext, &msg);
                    return Err(Error::DeviceLost(enriched));
                }
                return Err(Error::DeviceLost(msg));
            }
            Err(e) => return Err(e),
        };

        // Incrementally save the pipeline cache after enough new pipelines have
        // been compiled, so data is not lost if the process is killed before shutdown.
        let checkpoint = pipelines.maybe_checkpoint(&self.device);
        drop(pipelines); // release lock before disk I/O
        if let Some(data) = checkpoint {
            save_pipeline_cache_file(&data);
        }

        Ok(handle)
    }

    fn pass_timings(&self) -> Vec<(String, f32)> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.commands
            .lock()
            .expect("vulkan command context mutex poisoned")
            .pass_timings()
            .to_vec()
    }

    fn wait_submission(&self, token: SubmissionHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.commands
            .lock()
            .expect("vulkan command context mutex poisoned")
            .wait_for_submission(&self.device, token)
    }

    fn present(&self) -> Result<()> {
        Err(Error::Unsupported(
            "Vulkan presentation requires a Surface; use Surface::present after acquiring and rendering a surface image",
        ))
    }

    fn wait_idle(&self) -> Result<()> {
        unsafe {
            self.device
                .device_wait_idle()
                .map_err(|error| Error::Backend(format!("vkDeviceWaitIdle failed: {error:?}")))
        }
    }

    // ── GFX-4: Video encode/decode ────────────────────────────────────────────

    fn create_video_session(
        &self,
        _handle: VideoSessionHandle,
        _desc: VideoSessionDesc,
    ) -> Result<()> {
        Err(Error::Unsupported(
            "Vulkan video session creation is not yet implemented".into(),
        ))
    }

    fn destroy_video_session(&self, _handle: VideoSessionHandle) -> Result<()> {
        Ok(())
    }

    // ── GFX-6b: Latency reduction ─────────────────────────────────────────────

    fn latency_mode(&self) -> Option<LatencyMode> {
        if self.caps.features.reflex {
            return Some(LatencyMode::Reflex(ReflexMode::Off));
        }
        if self.caps.features.anti_lag {
            return Some(LatencyMode::AntiLag(AntiLagMode::Off));
        }
        None
    }

    fn set_reflex_mode(&self, mode: ReflexMode) -> Result<()> {
        if self.reflex_nv.is_none() {
            return Err(Error::Unsupported(
                "NVIDIA Reflex is not available on this device".into(),
            ));
        }
        // Encode mode as a u8: 0=Off, 1=On, 2=OnPlusBoost
        let encoded: u8 = match mode {
            ReflexMode::Off => 0,
            ReflexMode::On => 1,
            ReflexMode::OnPlusBoost => 2,
        };
        self.reflex_mode
            .store(encoded, std::sync::atomic::Ordering::Relaxed);
        // TODO: apply per-swapchain when VkSwapchainKHR handles are accessible from here.
        // vkSetLatencySleepModeNV requires a swapchain handle; the desired mode is stored
        // above and should be applied in the present path once surface handles are available.
        Ok(())
    }

    fn create_shader_object(
        &self,
        handle: crate::shader_object::ShaderObjectHandle,
        desc: &crate::shader_object::ShaderObjectDesc,
    ) -> Result<()> {
        let ext = self
            .shader_object_ext
            .as_ref()
            .ok_or_else(|| Error::Unsupported("VK_EXT_shader_object is not enabled".into()))?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let shaders = self
            .shaders
            .lock()
            .expect("vulkan shader registry mutex poisoned");
        let stage = shaders.stage(desc.shader)?;
        let spirv_words = shaders.spirv_words(desc.shader)?;
        let spirv_bytes = unsafe {
            std::slice::from_raw_parts(
                spirv_words.as_ptr().cast::<u8>(),
                std::mem::size_of_val(spirv_words),
            )
        };
        let entry_cstr = std::ffi::CString::new(shaders.entry_point(desc.shader)?)
            .map_err(|_| Error::InvalidInput("shader entry point contains nul bytes".into()))?;
        let stage_flags = shaders::shader_stage_flags(stage);
        let (set_layouts, push_constant_ranges) = if let Some(layout) = desc.layout {
            //panic allowed, reason = "poisoned mutex is unrecoverable"
            self.descriptors
                .read()
                .expect("vulkan descriptor registry rwlock poisoned")
                .shader_object_layout_info(layout)?
        } else {
            (Vec::new(), Vec::new())
        };
        let create_info = vk::ShaderCreateInfoEXT::default()
            .stage(stage_flags)
            .code_type(vk::ShaderCodeTypeEXT::SPIRV)
            .code(spirv_bytes)
            .name(&entry_cstr)
            .set_layouts(&set_layouts)
            .push_constant_ranges(&push_constant_ranges);
        let shader_objs = unsafe {
            ext.create_shaders(&[create_info], None)
                .map_err(|e| Error::Backend(format!("vkCreateShadersEXT failed: {e:?}")))?
        };
        // Release the shaders lock before acquiring resources write lock.
        drop(shaders);
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.resources
            .write()
            .expect("vulkan resource registry rwlock poisoned")
            .register_shader_object(
                handle,
                resources::VulkanShaderObject {
                    shader: shader_objs[0],
                    stage: stage_flags,
                    layout: desc.layout,
                },
            );
        Ok(())
    }

    fn destroy_shader_object(
        &self,
        handle: crate::shader_object::ShaderObjectHandle,
    ) -> Result<()> {
        if let Some(ref ext) = self.shader_object_ext {
            //panic allowed, reason = "poisoned mutex is unrecoverable"
            if let Ok(mut resources) = self.resources.write() {
                resources.destroy_shader_object(ext, handle)?;
            }
        }
        Ok(())
    }

    fn set_surface_hdr_metadata(
        &self,
        surface: SurfaceHandle,
        metadata: HdrMetadata,
    ) -> Result<()> {
        let hdr_ext = match self.hdr_metadata_ext.as_ref() {
            Some(ext) => ext,
            None => return Ok(()), // HDR not available; silently ignore
        };
        let surfaces = self
            .surfaces
            .lock()
            .expect("surface registry mutex poisoned");
        let swapchain = surfaces.swapchain_handle(surface)?;
        let primaries = metadata.display_primaries;
        let vk_meta = ash::vk::HdrMetadataEXT::default()
            .display_primary_red(ash::vk::XYColorEXT {
                x: primaries[0][0],
                y: primaries[0][1],
            })
            .display_primary_green(ash::vk::XYColorEXT {
                x: primaries[1][0],
                y: primaries[1][1],
            })
            .display_primary_blue(ash::vk::XYColorEXT {
                x: primaries[2][0],
                y: primaries[2][1],
            })
            .white_point(ash::vk::XYColorEXT {
                x: metadata.white_point[0],
                y: metadata.white_point[1],
            })
            .max_luminance(metadata.max_luminance)
            .min_luminance(metadata.min_luminance)
            .max_content_light_level(metadata.max_content_light_level)
            .max_frame_average_light_level(metadata.max_frame_average_light_level);
        unsafe {
            hdr_ext.set_hdr_metadata(&[swapchain], &[vk_meta]);
        }
        Ok(())
    }
}

fn query_blas_build_sizes(
    as_ext: &ash::khr::acceleration_structure::Device,
    desc: &BlasBuildDesc,
) -> Result<AccelerationStructureBuildSizes> {
    if desc.mode == AccelerationStructureBuildMode::Compact {
        return Err(Error::Unsupported(
            "BLAS compaction size queries are not implemented yet".into(),
        ));
    }

    let geometries = desc
        .geometries
        .iter()
        .map(|geometry| {
            let triangles = vk::AccelerationStructureGeometryTrianglesDataKHR::default()
                .vertex_format(vk_vertex_format_for_as(geometry.vertex_format)?)
                .vertex_stride(geometry.vertex_stride as u64)
                .max_vertex(geometry.vertex_count.saturating_sub(1))
                .index_type(
                    geometry
                        .index_format
                        .map(vk_index_type)
                        .unwrap_or(vk::IndexType::NONE_KHR),
                );
            Ok(vk::AccelerationStructureGeometryKHR::default()
                .geometry_type(vk::GeometryTypeKHR::TRIANGLES)
                .geometry(vk::AccelerationStructureGeometryDataKHR { triangles }))
        })
        .collect::<Result<Vec<_>>>()?;

    let primitive_counts = desc
        .geometries
        .iter()
        .map(|geometry| {
            if geometry.index_buffer.is_some() {
                geometry.index_count / 3
            } else {
                geometry.vertex_count / 3
            }
        })
        .collect::<Vec<_>>();

    let build_info = vk::AccelerationStructureBuildGeometryInfoKHR::default()
        .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
        .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
        .mode(vk_build_mode(desc.mode))
        .geometries(&geometries);
    let mut sizes = vk::AccelerationStructureBuildSizesInfoKHR::default();
    unsafe {
        as_ext.get_acceleration_structure_build_sizes(
            vk::AccelerationStructureBuildTypeKHR::DEVICE,
            &build_info,
            &primitive_counts,
            &mut sizes,
        )
    };
    Ok(vk_build_sizes(sizes))
}

fn query_tlas_build_sizes(
    as_ext: &ash::khr::acceleration_structure::Device,
    desc: &TlasBuildDesc,
) -> Result<AccelerationStructureBuildSizes> {
    if desc.mode == AccelerationStructureBuildMode::Compact {
        return Err(Error::Unsupported(
            "TLAS compaction size queries are not implemented yet".into(),
        ));
    }

    let instances =
        vk::AccelerationStructureGeometryInstancesDataKHR::default().array_of_pointers(false);
    let geometry = vk::AccelerationStructureGeometryKHR::default()
        .geometry_type(vk::GeometryTypeKHR::INSTANCES)
        .geometry(vk::AccelerationStructureGeometryDataKHR { instances });
    let build_info = vk::AccelerationStructureBuildGeometryInfoKHR::default()
        .ty(vk::AccelerationStructureTypeKHR::TOP_LEVEL)
        .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
        .mode(vk_build_mode(desc.mode))
        .geometries(std::slice::from_ref(&geometry));
    let primitive_counts = [desc.instance_count];
    let mut sizes = vk::AccelerationStructureBuildSizesInfoKHR::default();
    unsafe {
        as_ext.get_acceleration_structure_build_sizes(
            vk::AccelerationStructureBuildTypeKHR::DEVICE,
            &build_info,
            &primitive_counts,
            &mut sizes,
        )
    };
    Ok(vk_build_sizes(sizes))
}

fn vk_build_sizes(
    sizes: vk::AccelerationStructureBuildSizesInfoKHR<'_>,
) -> AccelerationStructureBuildSizes {
    AccelerationStructureBuildSizes {
        acceleration_structure_size: sizes.acceleration_structure_size,
        build_scratch_size: sizes.build_scratch_size,
        update_scratch_size: sizes.update_scratch_size,
    }
}

fn vk_build_mode(mode: AccelerationStructureBuildMode) -> vk::BuildAccelerationStructureModeKHR {
    match mode {
        AccelerationStructureBuildMode::Build | AccelerationStructureBuildMode::Compact => {
            vk::BuildAccelerationStructureModeKHR::BUILD
        }
        AccelerationStructureBuildMode::Update => vk::BuildAccelerationStructureModeKHR::UPDATE,
    }
}

fn vk_index_type(format: IndexFormat) -> vk::IndexType {
    match format {
        IndexFormat::Uint16 => vk::IndexType::UINT16,
        IndexFormat::Uint32 => vk::IndexType::UINT32,
    }
}

fn vk_vertex_format_for_as(format: VertexFormat) -> Result<vk::Format> {
    match format {
        VertexFormat::Float32x2 => Ok(vk::Format::R32G32_SFLOAT),
        VertexFormat::Float32x3 => Ok(vk::Format::R32G32B32_SFLOAT),
        VertexFormat::Float32x4 => Ok(vk::Format::R32G32B32A32_SFLOAT),
    }
}

fn query_ray_tracing_sbt_properties(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
) -> ShaderBindingTableProperties {
    let mut rt_props = vk::PhysicalDeviceRayTracingPipelinePropertiesKHR::default();
    let mut props2 = vk::PhysicalDeviceProperties2::default().push_next(&mut rt_props);
    unsafe {
        instance.get_physical_device_properties2(physical_device, &mut props2);
    }
    ShaderBindingTableProperties {
        shader_group_handle_size: rt_props.shader_group_handle_size,
        shader_group_handle_alignment: rt_props.shader_group_handle_alignment,
        shader_group_base_alignment: rt_props.shader_group_base_alignment,
        max_shader_group_stride: rt_props.max_shader_group_stride,
    }
}

/// For each alias slot that has transient (unbound) images, allocate a shared
/// `VkDeviceMemory` and bind every image in the slot to it at offset 0.
///
/// Images that already have their own allocation (created via `create_image`)
/// are skipped — only unbound transient images produced by `create_transient_image`
/// are affected.
fn bind_transient_images_to_alias_heaps(
    device: &AshDevice,
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
    resources: &mut resources::ResourceRegistry,
    heaps: &mut alias_heaps::AliasHeapRegistry,
    graph: &CompiledGraph,
) -> Result<()> {
    if graph.alias_plan.image_lifetimes.is_empty() {
        return Ok(());
    }

    // Group image handles by alias slot.
    let mut slot_images: HashMap<u32, Vec<ImageHandle>> = HashMap::new();
    for (handle, lifetime) in &graph.alias_plan.image_lifetimes {
        slot_images
            .entry(lifetime.alias_slot)
            .or_default()
            .push(*handle);
    }

    let memory_properties =
        unsafe { instance.get_physical_device_memory_properties(physical_device) };

    for (slot_id, handles) in &slot_images {
        // Find the intersection of memory type bits and max size + alignment.
        let mut combined_type_bits: u32 = !0u32;
        let mut max_size: u64 = 0;
        let mut max_alignment: u64 = 1;

        for &handle in handles {
            let reqs = match resources.image_memory_requirements(device, handle) {
                Ok(r) => r,
                Err(_) => continue, // already-bound image; skip
            };
            combined_type_bits &= reqs.memory_type_bits;
            max_size = max_size.max(reqs.size);
            max_alignment = max_alignment.max(reqs.alignment);
        }

        if combined_type_bits == 0 || max_size == 0 {
            continue; // no compatible memory type or no unbound images in this slot
        }

        // Find a DEVICE_LOCAL memory type compatible with all images in this slot.
        let memory_type = resources
            .allocator()
            .find_memory_type(combined_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL)
            .or_else(|_| {
                // Fall back to any compatible type if device-local isn't available.
                find_any_memory_type(&memory_properties, combined_type_bits)
            })?;

        // Align size to the required alignment.
        let aligned_size = align_up(max_size, max_alignment);

        // Allocate (or reuse / grow) the heap for this slot.
        let memory = heaps.slot_memory(device, *slot_id, aligned_size, memory_type)?;

        // Bind all unbound images in this slot.
        for &handle in handles {
            resources.bind_image_to_memory_if_unbound(device, handle, memory, 0)?;
        }
    }

    Ok(())
}

fn find_any_memory_type(props: &vk::PhysicalDeviceMemoryProperties, type_bits: u32) -> Result<u32> {
    for index in 0..props.memory_type_count {
        if (type_bits & (1 << index)) != 0 {
            return Ok(index);
        }
    }
    Err(Error::Unsupported(
        "no compatible Vulkan memory type found for alias heap",
    ))
}

fn align_up(value: u64, alignment: u64) -> u64 {
    if alignment == 0 {
        return value;
    }
    (value + alignment - 1) & !(alignment - 1)
}

/// Collect VK_EXT_device_fault breadcrumbs and append them to an existing
/// device-lost message. Returns the original message if the call fails.
fn gather_device_fault_info(
    fault_ext: &ash::ext::device_fault::Device,
    original_msg: &str,
) -> String {
    let device_handle = fault_ext.device();
    let get_fault = fault_ext.fp().get_device_fault_info_ext;
    let mut fault_counts = ash::vk::DeviceFaultCountsEXT::default();
    // First call: get counts only (fault_info = null).
    let count_result = unsafe { get_fault(device_handle, &mut fault_counts, std::ptr::null_mut()) };
    if count_result != ash::vk::Result::SUCCESS {
        return original_msg.to_string();
    }
    // Second call: fill address and vendor info structs.
    let mut address_infos = vec![
        ash::vk::DeviceFaultAddressInfoEXT::default();
        fault_counts.address_info_count as usize
    ];
    let mut vendor_infos =
        vec![ash::vk::DeviceFaultVendorInfoEXT::default(); fault_counts.vendor_info_count as usize];
    let address_ptr = if address_infos.is_empty() {
        std::ptr::null_mut()
    } else {
        address_infos.as_mut_ptr()
    };
    let vendor_ptr = if vendor_infos.is_empty() {
        std::ptr::null_mut()
    } else {
        vendor_infos.as_mut_ptr()
    };
    let mut fault_info = ash::vk::DeviceFaultInfoEXT {
        p_address_infos: address_ptr,
        p_vendor_infos: vendor_ptr,
        ..Default::default()
    };
    let info_result = unsafe { get_fault(device_handle, &mut fault_counts, &mut fault_info) };
    if info_result != ash::vk::Result::SUCCESS {
        return original_msg.to_string();
    }

    format_device_fault_info(
        original_msg,
        &fault_info,
        &address_infos,
        &vendor_infos,
        fault_counts.vendor_binary_size,
    )
}

fn format_device_fault_info(
    original_msg: &str,
    fault_info: &ash::vk::DeviceFaultInfoEXT<'_>,
    address_infos: &[ash::vk::DeviceFaultAddressInfoEXT],
    vendor_infos: &[ash::vk::DeviceFaultVendorInfoEXT],
    vendor_binary_size: u64,
) -> String {
    let mut msg = original_msg.to_string();
    let description =
        unsafe { std::ffi::CStr::from_ptr(fault_info.description.as_ptr()) }.to_string_lossy();
    if !description.is_empty() {
        msg.push_str("\n[device_fault] ");
        msg.push_str(&description);
    }
    if !address_infos.is_empty() {
        msg.push_str("\n[device_fault address info]");
        for (i, ai) in address_infos.iter().enumerate() {
            msg.push_str(&format!(
                "\n  [{}] type={} address=0x{:x} precision={}",
                i,
                device_fault_address_type(ai.address_type),
                ai.reported_address,
                ai.address_precision
            ));
        }
    }
    if !vendor_infos.is_empty() {
        msg.push_str("\n[device_fault vendor info]");
        for (i, vi) in vendor_infos.iter().enumerate() {
            let desc = unsafe { std::ffi::CStr::from_ptr(vi.description.as_ptr()) };
            msg.push_str(&format!(
                "\n  [{}] code=0x{:x} data=0x{:x} desc={}",
                i,
                vi.vendor_fault_code,
                vi.vendor_fault_data,
                desc.to_string_lossy()
            ));
        }
    }
    if vendor_binary_size > 0 {
        msg.push_str(&format!(
            "\n[device_fault vendor binary] size={} bytes",
            vendor_binary_size
        ));
    }
    msg
}

fn device_fault_address_type(address_type: ash::vk::DeviceFaultAddressTypeEXT) -> &'static str {
    if address_type == ash::vk::DeviceFaultAddressTypeEXT::READ_INVALID {
        "read_invalid"
    } else if address_type == ash::vk::DeviceFaultAddressTypeEXT::WRITE_INVALID {
        "write_invalid"
    } else if address_type == ash::vk::DeviceFaultAddressTypeEXT::EXECUTE_INVALID {
        "execute_invalid"
    } else if address_type == ash::vk::DeviceFaultAddressTypeEXT::INSTRUCTION_POINTER_UNKNOWN {
        "instruction_pointer_unknown"
    } else if address_type == ash::vk::DeviceFaultAddressTypeEXT::INSTRUCTION_POINTER_INVALID {
        "instruction_pointer_invalid"
    } else if address_type == ash::vk::DeviceFaultAddressTypeEXT::INSTRUCTION_POINTER_FAULT {
        "instruction_pointer_fault"
    } else {
        "none"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_fault_report_includes_description_addresses_and_vendor_info() {
        let fault_description = std::ffi::CString::new("fault in test pass").unwrap();
        let fault_info = vk::DeviceFaultInfoEXT::default()
            .description(&fault_description)
            .unwrap();
        let address_info = vk::DeviceFaultAddressInfoEXT::default()
            .address_type(vk::DeviceFaultAddressTypeEXT::WRITE_INVALID)
            .reported_address(0xabc0)
            .address_precision(64);
        let vendor_description = std::ffi::CString::new("vendor detail").unwrap();
        let vendor_info = vk::DeviceFaultVendorInfoEXT::default()
            .description(&vendor_description)
            .unwrap()
            .vendor_fault_code(0x12)
            .vendor_fault_data(0x34);

        let report = format_device_fault_info(
            "device lost",
            &fault_info,
            &[address_info],
            &[vendor_info],
            8,
        );

        assert!(report.contains("[device_fault] fault in test pass"));
        assert!(report.contains("[device_fault address info]"));
        assert!(report.contains("type=write_invalid address=0xabc0 precision=64"));
        assert!(report.contains("[device_fault vendor info]"));
        assert!(report.contains("code=0x12 data=0x34 desc=vendor detail"));
        assert!(report.contains("[device_fault vendor binary] size=8 bytes"));
    }
}

impl Drop for VulkanBackend {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            if let Ok(commands) = self.commands.lock() {
                commands.destroy(&self.device);
            }
            if let Ok(pipelines) = self.pipelines.lock() {
                if let Ok(data) = pipelines.serialize_cache(&self.device) {
                    save_pipeline_cache_file(&data);
                }
            }
            if let Ok(mut pipelines) = self.pipelines.lock() {
                pipelines.destroy_all(&self.device);
            }
            if let Ok(mut descriptors) = self.descriptors.write() {
                descriptors.destroy_all(&self.device);
            }
            if let Ok(mut shaders) = self.shaders.lock() {
                shaders.destroy_all(&self.device);
            }
            if let Ok(mut resources) = self.resources.write() {
                resources.destroy_all(
                    &self.device,
                    self.acceleration_structure_khr.as_ref(),
                    self.shader_object_ext.as_ref(),
                );
            }
            if let Ok(mut surfaces) = self.surfaces.lock() {
                surfaces.destroy_all(&self.device);
            }
            if let Ok(mut heaps) = self.alias_heaps.lock() {
                heaps.destroy_all(&self.device);
            }
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

fn pipeline_cache_path() -> PathBuf {
    dirs_next().join("sturdy-engine").join("pipeline_cache.bin")
}

fn dirs_next() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        return PathBuf::from(xdg);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".cache");
    }
    PathBuf::from("/tmp")
}

fn load_pipeline_cache_file() -> Option<Vec<u8>> {
    fs::read(pipeline_cache_path()).ok()
}

fn save_pipeline_cache_file(data: &[u8]) {
    let path = pipeline_cache_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("bin.tmp");
    if fs::write(&tmp, data).is_ok() {
        let _ = fs::rename(&tmp, &path);
    }
}
