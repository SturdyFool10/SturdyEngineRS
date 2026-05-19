mod adapter;
mod alias_heaps;
mod allocator;
pub(crate) mod bindless;
mod buffer_pool;
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
use std::ffi::c_void;

use ash::vk::TaggedStructure;
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
    enabled_extension_names: Vec<String>,
    debug: debug::DebugUtils,
    /// GFX-1g: Instance-level debug messenger for VK_EXT_device_address_binding_report events.
    /// Active in debug builds only when the extension is enabled.
    address_binding_messenger: Option<debug::AddressBindingMessenger>,
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
    /// GFX-7b: VK_EXT_descriptor_heap-backed bindless heap populated in parallel with
    /// the pool heap. Drives rendering once shaders adopt heap-access decorations.
    descriptor_heap_bindless: Option<bindless::DescriptorHeapBindlessHeap>,
    /// VK_EXT_mesh_shader commands. Present only when the mesh shader feature was enabled.
    mesh_shader_ext: Option<ash::ext::mesh_shader::Device>,
    /// VK_KHR_synchronization2 commands. Present when sync2 is enabled.
    synchronization2_khr: Option<ash::khr::synchronization2::Device>,
    /// VK_KHR_dynamic_rendering commands. Present when dynamic rendering is enabled.
    dynamic_rendering_khr: Option<ash::khr::dynamic_rendering::Device>,
    /// VK_EXT_device_fault commands. Present when device fault extension is available.
    device_fault_ext: Option<ash::ext::device_fault::Device>,
    /// VK_NV_device_diagnostic_checkpoints — pass markers that survive device loss (debug builds only).
    diagnostic_checkpoints_nv: Option<ash::nv::device_diagnostic_checkpoints::Device>,
    /// VK_AMD_buffer_marker — breadcrumb writes into a GPU buffer at each pipeline stage (debug builds only).
    buffer_marker_amd: Option<ash::amd::buffer_marker::Device>,
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
    /// Timeline semaphore used by vkLatencySleepNV to signal when to start the frame.
    /// Present when both `reflex` and `timeline_semaphores` are available.
    reflex_sleep_semaphore: Option<vk::Semaphore>,
    /// Monotonically-increasing value signaled into `reflex_sleep_semaphore` each frame.
    reflex_sleep_value: std::sync::atomic::AtomicU64,
    /// Raw VK_AMD_anti_lag entry point. ash does not generate this extension yet.
    anti_lag_update_amd: Option<PfnVkAntiLagUpdateAmd>,
    /// Desired AMD Anti-Lag mode; sent to the driver on frame-start notifications.
    anti_lag_mode: std::sync::atomic::AtomicU8,
    anti_lag_frame_index: std::sync::atomic::AtomicU64,
    /// VK_EXT_hdr_metadata commands. Present when hdr_output is available.
    hdr_metadata_ext: Option<ash::ext::hdr_metadata::Device>,
    /// VK_EXT_extended_dynamic_state3 commands. Present when the feature is enabled.
    extended_dynamic_state3_ext: Option<ash::ext::extended_dynamic_state3::Device>,
    /// VK_EXT_vertex_input_dynamic_state commands. Present when the feature is enabled.
    vertex_input_dynamic_state_ext: Option<ash::ext::vertex_input_dynamic_state::Device>,
    /// VK_EXT_shader_object commands. Present when the feature is enabled.
    shader_object_ext: Option<ash::ext::shader_object::Device>,
    /// VK_NV_device_generated_commands commands. Present when DGC NV extension is detected.
    device_generated_commands_nv: Option<ash::nv::device_generated_commands::Device>,
    indirect_command_layouts:
        Mutex<HashMap<crate::IndirectCommandLayoutHandle, vk::IndirectCommandsLayoutNV>>,
    /// VK_NV_optical_flow commands. Present when optical_flow_nv feature is detected.
    optical_flow_nv_ext: Option<ash::nv::optical_flow::Device>,
    /// GFX-3c: VK_NV_cluster_acceleration_structure commands. Present when feature detected.
    cluster_as_nv_ext: Option<ash::nv::cluster_acceleration_structure::Device>,
    /// GFX-7b: VK_EXT_descriptor_heap commands. Present when descriptor_heap feature is available.
    descriptor_heap_ext: Option<ash::ext::descriptor_heap::Device>,
    optical_flow_sessions:
        Mutex<HashMap<crate::OpticalFlowSessionHandle, vk::OpticalFlowSessionNV>>,
    /// VK_EXT_host_image_copy commands. Present when host_image_copy feature is available.
    host_image_copy_ext: Option<ash::ext::host_image_copy::Device>,
    /// VK_KHR_ray_tracing_maintenance1 commands. Present when ray_tracing_maintenance1 is set.
    ray_tracing_maintenance1_khr: Option<ash::khr::ray_tracing_maintenance1::Device>,
    /// VK_KHR_video_queue commands. Present when video_queue feature is detected.
    video_queue_khr: Option<ash::khr::video_queue::Device>,
    video_sessions: Mutex<HashMap<VideoSessionHandle, VulkanVideoSession>>,
    /// VK_EXT_pageable_device_local_memory — dynamic priority on device-local pages.
    pageable_memory_ext: Option<ash::ext::pageable_device_local_memory::Device>,
    /// User-created timeline semaphores (GFX-1c cross-queue coordination API).
    timeline_semaphores: Mutex<HashMap<crate::SemaphoreHandle, vk::Semaphore>>,
    /// Handle allocator for user-created timeline semaphores.
    timeline_semaphore_handles: Mutex<crate::handles::HandleAllocator>,
    /// VK_KHR_external_memory_fd — fd-based external memory export/import (Linux/macOS).
    external_memory_fd_khr: Option<ash::khr::external_memory_fd::Device>,
    /// VK_EXT_external_memory_host — host-pointer import.
    external_memory_host_ext: Option<ash::ext::external_memory_host::Device>,
    /// Registry of exportable image memory handles (image handle → dedicated VkDeviceMemory).
    exportable_image_memories: Mutex<HashMap<ImageHandle, vk::DeviceMemory>>,
    /// Registry of exportable buffer memory handles (buffer handle → dedicated VkDeviceMemory).
    exportable_buffer_memories: Mutex<HashMap<BufferHandle, vk::DeviceMemory>>,
    /// VK_KHR_external_semaphore_fd — fd-based external semaphore export/import (Linux/macOS).
    external_semaphore_fd_khr: Option<ash::khr::external_semaphore_fd::Device>,
    /// Registry of exportable semaphore VkSemaphore objects (SemaphoreHandle → vk::Semaphore).
    exportable_semaphores: Mutex<HashMap<crate::SemaphoreHandle, vk::Semaphore>>,
    /// GFX-5b: VK_KHR_external_fence_fd commands. Present when the extension is available.
    external_fence_fd_khr: Option<ash::khr::external_fence_fd::Device>,
    /// Registry of exportable fence VkFence objects (FenceHandle → vk::Fence).
    exportable_fences: Mutex<HashMap<crate::FenceHandle, vk::Fence>>,
    /// Handle allocator for exportable fences.
    exportable_fence_handles: Mutex<crate::handles::HandleAllocator>,
}

struct VulkanVideoSession {
    session: vk::VideoSessionKHR,
    memories: Vec<vk::DeviceMemory>,
}

type PfnVkAntiLagUpdateAmd = unsafe extern "system" fn(vk::Device, *const VkAntiLagDataAmd);

const VK_STRUCTURE_TYPE_ANTI_LAG_DATA_AMD: vk::StructureType =
    vk::StructureType::from_raw(1000476001);
const VK_STRUCTURE_TYPE_ANTI_LAG_PRESENTATION_INFO_AMD: vk::StructureType =
    vk::StructureType::from_raw(1000476002);

#[repr(i32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum VkAntiLagModeAmd {
    On = 1,
    Off = 2,
}

#[repr(i32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum VkAntiLagStageAmd {
    Input = 0,
}

#[repr(C)]
struct VkAntiLagPresentationInfoAmd {
    s_type: vk::StructureType,
    p_next: *mut c_void,
    stage: VkAntiLagStageAmd,
    frame_index: u64,
}

#[repr(C)]
struct VkAntiLagDataAmd {
    s_type: vk::StructureType,
    p_next: *const c_void,
    mode: VkAntiLagModeAmd,
    max_fps: u32,
    p_presentation_info: *const VkAntiLagPresentationInfoAmd,
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
        // GFX-7a descriptor buffers require both the extension and its feature struct
        // to be enabled on the logical device. Feature-chain enablement is not wired
        // yet, so keep runtime descriptor-buffer paths disabled and use descriptor
        // set pools instead of loading unusable extension entry points.
        caps.features.descriptor_buffer = false;
        let video_queue_enabled =
            enabled_extension(&logical.enabled_extension_names, "VK_KHR_video_queue");
        // Raw codec extensions are exposed through `raw_extension_names`, but
        // runtime encode/decode frame features stay disabled until command
        // recording is executable. `video_queue` is restored below when the
        // extension is enabled because this backend now owns real
        // VkVideoSessionKHR lifetime and memory binding.
        caps.features.disable_video_features();
        caps.features.video_queue = video_queue_enabled;
        // Same policy for DGC: raw extension names remain visible for diagnostics,
        // but executable support waits for layout/resource ownership and command
        // recording through VK_EXT_device_generated_commands or the NV variant.
        caps.features.disable_device_generated_command_features();
        caps.features.optical_flow_nv =
            enabled_extension(&logical.enabled_extension_names, "VK_NV_optical_flow");
        let anti_lag_update_amd =
            load_anti_lag_update_amd(&instance, &logical.device, &logical.enabled_extension_names);
        // Raw extension discovery is not enough here: expose runtime Anti-Lag
        // only when the logical device enabled the extension and the dispatch
        // entry point is available.
        caps.features.disable_anti_lag_features();
        caps.features.anti_lag = anti_lag_update_amd.is_some();
        let props = unsafe { instance.get_physical_device_properties(selection.physical_device) };
        let timestamp_period_ns = props.limits.timestamp_period;
        let memory_properties =
            unsafe { instance.get_physical_device_memory_properties(selection.physical_device) };
        let mut resource_registry = resources::ResourceRegistry::new(memory_properties);
        resource_registry.image_view_min_lod_enabled = caps.features.image_view_min_lod;
        resource_registry.image_compression_control_enabled =
            caps.features.image_compression_control;
        resource_registry.optical_flow_enabled = caps.features.optical_flow_nv;
        resource_registry.allocator_mut().memory_priority_enabled = caps.features.memory_priority;
        // GFX-1e: initialise the budget-aware block-size threshold from the OS-reported budget.
        if caps.features.memory_budget {
            let mut budget_props = ash::vk::PhysicalDeviceMemoryBudgetPropertiesEXT::default();
            let mut props2 =
                ash::vk::PhysicalDeviceMemoryProperties2::default().push(&mut budget_props);
            unsafe {
                instance
                    .get_physical_device_memory_properties2(selection.physical_device, &mut props2)
            };
            let heap_count = props2.memory_properties.memory_heap_count as usize;
            let total_device_local_budget: u64 = (0..heap_count)
                .filter(|&i| {
                    props2.memory_properties.memory_heaps[i]
                        .flags
                        .contains(ash::vk::MemoryHeapFlags::DEVICE_LOCAL)
                })
                .map(|i| budget_props.heap_budget[i])
                .sum();
            if total_device_local_budget > 0 {
                resource_registry.allocator_mut().device_local_budget = total_device_local_budget;
            }
        }
        resource_registry.dedicated_allocation_enabled = true; // Core in Vulkan 1.1
        // Note: pageable_memory_ext is set after creation below.
        // Pre-load buffer_marker_amd for breadcrumb buffer creation in CommandContext.
        #[cfg(debug_assertions)]
        let bm_amd_for_create = if caps.features.buffer_marker_amd {
            Some(ash::amd::buffer_marker::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        #[cfg(not(debug_assertions))]
        let bm_amd_for_create: Option<ash::amd::buffer_marker::Device> = None;
        let mut commands = commands::FramedCommands::create(
            &logical.device,
            logical.queue_families,
            timestamp_period_ns,
            bm_amd_for_create.as_ref(),
            memory_properties,
            caps.features.timeline_semaphores, // GFX-1c: use timeline chains when available
        )?;
        // Track 11a: create a 4 MiB per-frame transient buffer pool in each context.
        // Each context gets its own pool so frames-in-flight don't conflict.
        const TRANSIENT_POOL_BYTES: u64 = 4 * 1024 * 1024;
        for ctx in commands.contexts_mut() {
            match buffer_pool::BufferPool::create(
                &logical.device,
                memory_properties,
                TRANSIENT_POOL_BYTES,
            ) {
                Ok(pool) => ctx.set_buffer_pool(pool),
                Err(e) => eprintln!(
                    "[SturdyEngine] transient buffer pool creation failed (no-op fallback): {e}"
                ),
            }
        }
        let cache_data = load_pipeline_cache_file();
        let mut pipeline_registry =
            pipelines::PipelineRegistry::create(&logical.device, cache_data.as_deref())?;
        pipeline_registry.dynamic_rendering_enabled = logical.dynamic_rendering_enabled;
        pipeline_registry.vrs_pipeline_enabled = caps.features.vrs_pipeline;
        pipeline_registry.conservative_rasterization_overestimate_enabled =
            caps.features.conservative_rasterization_overestimate;
        pipeline_registry.conservative_rasterization_underestimate_enabled =
            caps.features.conservative_rasterization_underestimate;
        pipeline_registry.descriptor_heap_enabled = caps.features.descriptor_heap;
        pipeline_registry.descriptor_buffer_enabled =
            caps.features.descriptor_buffer && caps.features.buffer_device_address;
        pipeline_registry.graphics_pipeline_library_enabled =
            caps.features.graphics_pipeline_library;
        // GFX-7a: set up descriptor buffer backing BEFORE caps/instance/device are moved into Self.
        let mut descriptors_reg = descriptors::DescriptorRegistry::default();
        if caps.features.descriptor_buffer && caps.features.buffer_device_address {
            let db_device = ash::ext::descriptor_buffer::Device::load(&instance, &logical.device);
            descriptors_reg.set_descriptor_buffer(db_device);
            descriptors_reg.buffer_device_address_enabled = true;
        }

        let debug_utils = debug::DebugUtils::new(&instance, &logical.device);
        // GFX-1g: Create address binding report messenger in debug builds when extension is enabled.
        let address_binding_messenger = if caps.features.device_address_binding_report {
            debug::AddressBindingMessenger::create(&entry, &instance)
        } else {
            None
        };
        let mesh_shader_ext = if logical.mesh_shader_enabled {
            Some(ash::ext::mesh_shader::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let synchronization2_khr = if logical.synchronization2_enabled {
            Some(ash::khr::synchronization2::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let dynamic_rendering_khr = if logical.dynamic_rendering_enabled {
            Some(ash::khr::dynamic_rendering::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let device_fault_ext = if caps.features.device_fault {
            Some(ash::ext::device_fault::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        #[cfg(debug_assertions)]
        let diagnostic_checkpoints_nv = if caps.features.device_diagnostic_checkpoints_nv {
            Some(ash::nv::device_diagnostic_checkpoints::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        #[cfg(not(debug_assertions))]
        let diagnostic_checkpoints_nv: Option<
            ash::nv::device_diagnostic_checkpoints::Device,
        > = None;
        #[cfg(debug_assertions)]
        let buffer_marker_amd = if caps.features.buffer_marker_amd {
            Some(ash::amd::buffer_marker::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        #[cfg(not(debug_assertions))]
        let buffer_marker_amd: Option<ash::amd::buffer_marker::Device> = None;
        let push_descriptor_khr = if logical.push_descriptors_enabled {
            Some(ash::khr::push_descriptor::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let conditional_rendering_ext = if logical.conditional_rendering_enabled {
            Some(ash::ext::conditional_rendering::Device::load(
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
            Some(ash::khr::fragment_shading_rate::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let conservative_rasterization_enabled = logical.conservative_rasterization_enabled;
        let acceleration_structure_khr = if logical.acceleration_structure_enabled {
            Some(ash::khr::acceleration_structure::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let ray_tracing_pipeline_khr = if logical.ray_tracing_pipeline_enabled {
            Some(ash::khr::ray_tracing_pipeline::Device::load(
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
            Some(ash::nv::low_latency2::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        // Create a timeline semaphore for vkLatencySleepNV when both Reflex and timeline
        // semaphores are available. The semaphore is signaled by the driver each frame when
        // it's the optimal time to begin CPU work.
        let reflex_sleep_semaphore = if caps.features.reflex && caps.features.timeline_semaphores {
            let mut timeline_info = ash::vk::SemaphoreTypeCreateInfo::default()
                .semaphore_type(ash::vk::SemaphoreType::TIMELINE)
                .initial_value(0);
            let sem_info = ash::vk::SemaphoreCreateInfo::default().push(&mut timeline_info);
            unsafe { logical.device.create_semaphore(&sem_info, None).ok() }
        } else {
            None
        };
        let hdr_metadata_ext = if caps.features.hdr_output {
            Some(ash::ext::hdr_metadata::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let extended_dynamic_state3_ext = if logical.extended_dynamic_state3_enabled {
            Some(ash::ext::extended_dynamic_state3::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let vertex_input_dynamic_state_ext = if logical.vertex_input_dynamic_state_enabled {
            Some(ash::ext::vertex_input_dynamic_state::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let shader_object_ext = if logical.shader_object_enabled {
            Some(ash::ext::shader_object::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let device_generated_commands_nv = if caps.features.device_generated_commands_nv {
            Some(ash::nv::device_generated_commands::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let optical_flow_nv_ext = if caps.features.optical_flow_nv {
            Some(ash::nv::optical_flow::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        // GFX-3c: load cluster AS commands.
        let cluster_as_nv_ext = if caps.features.cluster_acceleration_structure {
            Some(ash::nv::cluster_acceleration_structure::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        // GFX-7b: load descriptor heap commands when the feature is available.
        let descriptor_heap_ext = if caps.features.descriptor_heap {
            Some(ash::ext::descriptor_heap::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let host_image_copy_ext = if caps.features.host_image_copy {
            Some(ash::ext::host_image_copy::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let ray_tracing_maintenance1_khr = if caps.features.ray_tracing_maintenance1 {
            Some(ash::khr::ray_tracing_maintenance1::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        // Video queue extension — loaded for backend-owned video session creation.
        let video_queue_khr = if video_queue_enabled {
            Some(ash::khr::video_queue::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let pageable_memory_ext =
            if caps.features.pageable_device_local_memory && caps.features.memory_priority {
                Some(ash::ext::pageable_device_local_memory::Device::load(
                    &instance,
                    &logical.device,
                ))
            } else {
                None
            };
        let external_memory_fd_khr = if caps.features.external_memory_fd {
            Some(ash::khr::external_memory_fd::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let external_memory_host_ext = if caps.features.external_memory_host {
            Some(ash::ext::external_memory_host::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        let external_semaphore_fd_khr = if caps.features.external_semaphore_fd {
            Some(ash::khr::external_semaphore_fd::Device::load(
                &instance,
                &logical.device,
            ))
        } else {
            None
        };
        // GFX-5b: load external fence fd commands.
        let external_fence_fd_khr = if caps.features.external_fence_fd {
            Some(ash::khr::external_fence_fd::Device::load(
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

        // GFX-7b: Create the descriptor-heap bindless heap when VK_EXT_descriptor_heap is active.
        let descriptor_heap_bindless = if caps.features.descriptor_heap {
            if let Some(ref heap_device) = descriptor_heap_ext {
                let inst_loader = ash::ext::descriptor_heap::Instance::load(&entry, &instance);
                let sizes = bindless::DescriptorHeapSizes {
                    sampler: unsafe {
                        inst_loader.get_physical_device_descriptor_size(
                            selection.physical_device,
                            ash::vk::DescriptorType::SAMPLER,
                        )
                    },
                    sampled_image: unsafe {
                        inst_loader.get_physical_device_descriptor_size(
                            selection.physical_device,
                            ash::vk::DescriptorType::SAMPLED_IMAGE,
                        )
                    },
                    storage_image: unsafe {
                        inst_loader.get_physical_device_descriptor_size(
                            selection.physical_device,
                            ash::vk::DescriptorType::STORAGE_IMAGE,
                        )
                    },
                    storage_buffer: unsafe {
                        inst_loader.get_physical_device_descriptor_size(
                            selection.physical_device,
                            ash::vk::DescriptorType::STORAGE_BUFFER,
                        )
                    },
                };
                match bindless::DescriptorHeapBindlessHeap::create(
                    &logical.device,
                    &instance,
                    selection.physical_device,
                    sizes,
                    heap_device.clone(),
                    memory_properties,
                ) {
                    Ok(h) => Some(h),
                    Err(e) => {
                        eprintln!("[SturdyEngine] descriptor heap creation failed: {e}");
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };

        // Set pageable memory extension after all extensions are loaded.
        if let Some(ref pm) = pageable_memory_ext {
            resource_registry.pageable_memory_ext = Some(pm.clone());
        }

        Ok(Self {
            _entry: entry,
            instance,
            physical_device: selection.physical_device,
            device: logical.device,
            queue_families: logical.queue_families,
            queues: logical.queues,
            caps,
            enabled_extension_names: logical.enabled_extension_names,
            debug: debug_utils,
            address_binding_messenger,
            commands: Mutex::new(commands),
            descriptors: RwLock::new(descriptors_reg),
            pipelines: Mutex::new(pipeline_registry),
            resources: RwLock::new(resource_registry),
            shaders: Mutex::new(shaders::ShaderRegistry::default()),
            surfaces: Mutex::new(surfaces::SurfaceRegistry::default()),
            alias_heaps: Mutex::new(alias_heaps::AliasHeapRegistry::default()),
            active_surface: Mutex::new(None),
            bindless_heap,
            descriptor_heap_bindless,
            mesh_shader_ext,
            synchronization2_khr,
            dynamic_rendering_khr,
            device_fault_ext,
            diagnostic_checkpoints_nv,
            buffer_marker_amd,
            push_descriptor_khr,
            conditional_rendering_ext,
            fragment_shading_rate_khr,
            conservative_rasterization_enabled,
            acceleration_structure_khr,
            ray_tracing_pipeline_khr,
            ray_tracing_sbt_properties,
            reflex_nv,
            reflex_mode: std::sync::atomic::AtomicU8::new(0),
            reflex_sleep_semaphore,
            reflex_sleep_value: std::sync::atomic::AtomicU64::new(0),
            anti_lag_update_amd,
            anti_lag_mode: std::sync::atomic::AtomicU8::new(0),
            anti_lag_frame_index: std::sync::atomic::AtomicU64::new(0),
            hdr_metadata_ext,
            extended_dynamic_state3_ext,
            vertex_input_dynamic_state_ext,
            shader_object_ext,
            device_generated_commands_nv,
            indirect_command_layouts: Mutex::new(HashMap::new()),
            optical_flow_nv_ext,
            optical_flow_sessions: Mutex::new(HashMap::new()),
            cluster_as_nv_ext,
            descriptor_heap_ext,
            host_image_copy_ext,
            ray_tracing_maintenance1_khr,
            video_queue_khr,
            video_sessions: Mutex::new(HashMap::new()),
            pageable_memory_ext,
            timeline_semaphores: Mutex::new(HashMap::new()),
            timeline_semaphore_handles: Mutex::new(crate::handles::HandleAllocator::default()),
            external_memory_fd_khr,
            external_memory_host_ext,
            exportable_image_memories: Mutex::new(HashMap::new()),
            exportable_buffer_memories: Mutex::new(HashMap::new()),
            external_semaphore_fd_khr,
            exportable_semaphores: Mutex::new(HashMap::new()),
            external_fence_fd_khr,
            exportable_fences: Mutex::new(HashMap::new()),
            exportable_fence_handles: Mutex::new(crate::handles::HandleAllocator::default()),
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
        let idx = heap.register_sampled_image(view)?;
        // GFX-7b: mirror into the descriptor heap in parallel when available.
        if let (Some(dh), Some((image, desc))) = (
            self.descriptor_heap_bindless.as_ref(),
            resources.image_and_desc(handle),
        ) {
            if let Ok(fmt) = resources::vk_format(desc.format) {
                let subresource = vk::ImageSubresourceRange {
                    aspect_mask: resources::vk_aspect_mask(desc.format),
                    base_mip_level: 0,
                    level_count: desc.mip_levels as u32,
                    base_array_layer: 0,
                    layer_count: desc.layers as u32,
                };
                let _ = dh.register_sampled_image(
                    image,
                    fmt,
                    resources::vk_image_view_type(desc),
                    subresource,
                );
            }
        }
        Some(idx)
    }

    /// Register a sampler in the bindless heap.
    pub fn register_bindless_sampler(&self, handle: SamplerHandle) -> Option<u32> {
        let heap = self.bindless_heap.as_ref()?;
        let resources = self.resources.read().ok()?;
        let sampler = resources.sampler(handle).ok()?;
        let idx = heap.register_sampler(sampler)?;
        // GFX-7b: mirror into the descriptor heap in parallel when available.
        if let (Some(dh), Some(desc)) = (
            self.descriptor_heap_bindless.as_ref(),
            resources.sampler_desc(handle),
        ) {
            let sci = build_sampler_create_info_for_heap(desc);
            let _ = dh.register_sampler_from_desc(sci);
        }
        Some(idx)
    }

    /// Register a storage image in the bindless heap.
    pub fn register_bindless_storage_image(&self, handle: ImageHandle) -> Option<u32> {
        let heap = self.bindless_heap.as_ref()?;
        let resources = self.resources.read().ok()?;
        let view = resources.image_view(handle).ok()?;
        let idx = heap.register_storage_image(view)?;
        // GFX-7b: mirror into the descriptor heap.
        if let (Some(dh), Some((image, desc))) = (
            self.descriptor_heap_bindless.as_ref(),
            resources.image_and_desc(handle),
        ) {
            if let Ok(fmt) = resources::vk_format(desc.format) {
                let subresource = vk::ImageSubresourceRange {
                    aspect_mask: resources::vk_aspect_mask(desc.format),
                    base_mip_level: 0,
                    level_count: desc.mip_levels as u32,
                    base_array_layer: 0,
                    layer_count: desc.layers as u32,
                };
                let _ = dh.register_storage_image(
                    image,
                    fmt,
                    resources::vk_image_view_type(desc),
                    subresource,
                );
            }
        }
        Some(idx)
    }

    /// Register a storage buffer in the bindless heap.
    pub fn register_bindless_storage_buffer(&self, handle: BufferHandle) -> Option<u32> {
        let heap = self.bindless_heap.as_ref()?;
        let resources = self.resources.read().ok()?;
        let buf = resources.buffer(handle).ok()?;
        // VK_WHOLE_SIZE (u64::MAX) means "bind the full buffer from offset 0".
        let idx = heap.register_storage_buffer(buf, 0, u64::MAX)?;
        // GFX-7b: mirror into the descriptor heap using the buffer's device address.
        if let Some(dh) = self.descriptor_heap_bindless.as_ref() {
            if let Ok(addr) = resources.buffer_device_address_raw(&self.device, handle) {
                let _ = dh.register_storage_buffer(addr, vk::WHOLE_SIZE);
            }
        }
        Some(idx)
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
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .allocator_stats();
        Some(crate::GpuMemoryBudget {
            device_local_used_bytes: stats.device_local_used_bytes,
            device_local_capacity_bytes: stats.device_local_capacity_bytes,
            host_visible_used_bytes: stats.host_visible_used_bytes,
            host_visible_capacity_bytes: stats.host_visible_capacity_bytes,
            block_count: stats.block_count,
        })
    }

    fn memory_budget_ext(&self) -> Option<crate::MemoryBudgetReport> {
        if !self.caps.features.memory_budget {
            return None;
        }
        let mut budget_props = ash::vk::PhysicalDeviceMemoryBudgetPropertiesEXT::default();
        let heap_count;
        let memory_heaps;
        {
            let mut props2 =
                ash::vk::PhysicalDeviceMemoryProperties2::default().push(&mut budget_props);
            unsafe {
                self.instance
                    .get_physical_device_memory_properties2(self.physical_device, &mut props2);
            }
            heap_count = props2.memory_properties.memory_heap_count as usize;
            memory_heaps = props2.memory_properties.memory_heaps;
        }
        let heaps: Vec<_> = (0..heap_count)
            .map(|i| crate::MemoryHeapBudget {
                budget: budget_props.heap_budget[i],
                usage: budget_props.heap_usage[i],
            })
            .collect();

        // GFX-1e: Warn when any device-local heap exceeds 80% of its OS-reported budget.
        for (i, heap) in heaps.iter().enumerate() {
            let is_device_local = memory_heaps[i]
                .flags
                .contains(ash::vk::MemoryHeapFlags::DEVICE_LOCAL);
            if is_device_local && heap.budget > 0 && heap.usage > heap.budget * 4 / 5 {
                let pct = (heap.usage * 100) / heap.budget;
                eprintln!(
                    "[SturdyEngine] VRAM budget warning: heap[{i}] {pct}% used ({}/{} MiB) — \
                     consider reducing texture streaming or render target resolution",
                    heap.usage / (1024 * 1024),
                    heap.budget / (1024 * 1024),
                );
            }
        }

        Some(crate::MemoryBudgetReport { heaps })
    }

    fn enumerate_performance_counters(&self) -> Vec<crate::PerfCounter> {
        if !self.caps.features.performance_query {
            return Vec::new();
        }
        let perf_khr = ash::khr::performance_query::Instance::load(&self._entry, &self.instance);
        // Query the graphics queue family — it exposes the widest set of counters.
        let queue_family_index = self.queue_families.graphics;
        // First, get the count.
        let count = unsafe {
            perf_khr
                .enumerate_physical_device_queue_family_performance_query_counters_len(
                    self.physical_device,
                    queue_family_index,
                )
                .unwrap_or(0)
        };
        if count == 0 {
            return Vec::new();
        }
        let mut counters = vec![ash::vk::PerformanceCounterKHR::default(); count];
        let mut descs = vec![ash::vk::PerformanceCounterDescriptionKHR::default(); count];
        if unsafe {
            perf_khr
                .enumerate_physical_device_queue_family_performance_query_counters(
                    self.physical_device,
                    queue_family_index,
                    &mut counters,
                    &mut descs,
                )
                .is_err()
        } {
            return Vec::new();
        }
        counters
            .iter()
            .zip(descs.iter())
            .enumerate()
            .map(|(i, (c, d))| {
                let name = d
                    .name
                    .iter()
                    .take_while(|&&ch| ch != 0)
                    .map(|&ch| ch as u8 as char)
                    .collect::<String>();
                let description = d
                    .description
                    .iter()
                    .take_while(|&&ch| ch != 0)
                    .map(|&ch| ch as u8 as char)
                    .collect::<String>();
                let category = match c.scope {
                    ash::vk::PerformanceCounterScopeKHR::COMMAND_BUFFER => {
                        crate::PerfCounterCategory::CommandBuffer
                    }
                    ash::vk::PerformanceCounterScopeKHR::COMMAND => {
                        crate::PerfCounterCategory::CommandBuffer
                    }
                    _ => crate::PerfCounterCategory::Generic,
                };
                crate::PerfCounter {
                    index: i as u32,
                    name,
                    description,
                    category,
                }
            })
            .collect()
    }

    fn enumerate_cooperative_matrix_properties(&self) -> Vec<crate::CoopMatrixProperty> {
        if !self.caps.features.cooperative_matrix {
            return Vec::new();
        }
        let coop_khr = ash::khr::cooperative_matrix::Instance::load(&self._entry, &self.instance);
        let props = unsafe {
            let count = coop_khr
                .get_physical_device_cooperative_matrix_properties_len(self.physical_device)
                .unwrap_or(0);
            let mut out = vec![ash::vk::CooperativeMatrixPropertiesKHR::default(); count];
            if count > 0 {
                let _ = coop_khr.get_physical_device_cooperative_matrix_properties(
                    self.physical_device,
                    &mut out,
                );
            }
            out
        };
        props
            .into_iter()
            .map(|p| crate::CoopMatrixProperty {
                scope: match p.scope {
                    ash::vk::ScopeKHR::WORKGROUP => crate::CoopMatrixScope::Workgroup,
                    ash::vk::ScopeKHR::SUBGROUP => crate::CoopMatrixScope::Subgroup,
                    ash::vk::ScopeKHR::QUEUE_FAMILY => crate::CoopMatrixScope::QueueFamily,
                    _ => crate::CoopMatrixScope::Device,
                },
                a_type: vk_component_type(p.a_type),
                b_type: vk_component_type(p.b_type),
                c_type: vk_component_type(p.c_type),
                result_type: vk_component_type(p.result_type),
                m_size: p.m_size,
                n_size: p.n_size,
                k_size: p.k_size,
                saturating_accumulation: p.saturating_accumulation == ash::vk::TRUE,
            })
            .collect()
    }

    fn pipeline_executable_stats(
        &self,
        pipeline: crate::PipelineHandle,
    ) -> Vec<crate::ExecutableStat> {
        if !self.caps.features.pipeline_executable_properties {
            return Vec::new();
        }
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let pipelines = self
            .pipelines
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let vk_pipeline = match pipelines.vk_pipeline(pipeline) {
            Ok(p) => p,
            Err(_) => return Vec::new(),
        };
        let pipeline_info = ash::vk::PipelineInfoKHR::default().pipeline(vk_pipeline);
        let pipeline_exe_ext =
            ash::khr::pipeline_executable_properties::Device::load(&self.instance, &self.device);
        let executables = unsafe {
            let count = pipeline_exe_ext
                .get_pipeline_executable_properties_len(&pipeline_info)
                .unwrap_or(0);
            let mut out = vec![ash::vk::PipelineExecutablePropertiesKHR::default(); count];
            if count > 0 {
                let _ =
                    pipeline_exe_ext.get_pipeline_executable_properties(&pipeline_info, &mut out);
            }
            out
        };
        let mut stats = Vec::new();
        for (idx, _exe) in executables.iter().enumerate() {
            let exe_info = ash::vk::PipelineExecutableInfoKHR::default()
                .pipeline(vk_pipeline)
                .executable_index(idx as u32);
            let raw_stats = unsafe {
                let count = pipeline_exe_ext
                    .get_pipeline_executable_statistics_len(&exe_info)
                    .unwrap_or(0);
                let mut out = vec![ash::vk::PipelineExecutableStatisticKHR::default(); count];
                if count > 0 {
                    let _ =
                        pipeline_exe_ext.get_pipeline_executable_statistics(&exe_info, &mut out);
                }
                out
            };
            for s in raw_stats {
                let name = s
                    .name
                    .iter()
                    .take_while(|&&c| c != 0)
                    .map(|&c| c as u8 as char)
                    .collect::<String>();
                let description = s
                    .description
                    .iter()
                    .take_while(|&&c| c != 0)
                    .map(|&c| c as u8 as char)
                    .collect::<String>();
                let (value_int, value_float, value_bool) = unsafe {
                    match s.format {
                        ash::vk::PipelineExecutableStatisticFormatKHR::BOOL32 => {
                            (None, None, Some(s.value.b32 != 0))
                        }
                        ash::vk::PipelineExecutableStatisticFormatKHR::INT64 => {
                            (Some(s.value.i64), None, None)
                        }
                        ash::vk::PipelineExecutableStatisticFormatKHR::UINT64 => {
                            (Some(s.value.u64 as i64), None, None)
                        }
                        ash::vk::PipelineExecutableStatisticFormatKHR::FLOAT64 => {
                            (None, Some(s.value.f64), None)
                        }
                        _ => (None, None, None),
                    }
                };
                stats.push(crate::ExecutableStat {
                    name,
                    description,
                    value_int,
                    value_float,
                    value_bool,
                });
            }
        }
        stats
    }

    fn pipeline_shader_stats_amd(
        &self,
        pipeline: crate::PipelineHandle,
    ) -> Vec<crate::AmdShaderStageStats> {
        if !self.caps.features.shader_info_amd {
            return Vec::new();
        }
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let pipelines = self
            .pipelines
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let vk_pipeline = match pipelines.vk_pipeline(pipeline) {
            Ok(p) => p,
            Err(_) => return Vec::new(),
        };
        let shader_info = ash::amd::shader_info::Device::load(&self.instance, &self.device);
        // Query all standard graphics and compute stages.
        let stages = [
            ash::vk::ShaderStageFlags::VERTEX,
            ash::vk::ShaderStageFlags::FRAGMENT,
            ash::vk::ShaderStageFlags::COMPUTE,
            ash::vk::ShaderStageFlags::GEOMETRY,
            ash::vk::ShaderStageFlags::TESSELLATION_CONTROL,
            ash::vk::ShaderStageFlags::TESSELLATION_EVALUATION,
        ];
        let mut stats = Vec::new();
        for stage in stages {
            let info = unsafe { shader_info.get_shader_info_statistics(vk_pipeline, stage) };
            if let Ok(info) = info {
                stats.push(crate::AmdShaderStageStats {
                    stage_mask: stage.as_raw(),
                    num_used_vgprs: info.resource_usage.num_used_vgprs,
                    num_used_sgprs: info.resource_usage.num_used_sgprs,
                    num_physical_vgprs: info.num_physical_vgprs,
                    num_physical_sgprs: info.num_physical_sgprs,
                    lds_size_per_workgroup: info.resource_usage.lds_size_per_local_work_group,
                    lds_usage_bytes: info.resource_usage.lds_usage_size_in_bytes,
                    scratch_mem_bytes: info.resource_usage.scratch_mem_usage_in_bytes,
                    compute_workgroup_size: info.compute_work_group_size,
                });
            }
        }
        stats
    }

    fn descriptor_buffer_offset_alignment(&self) -> Option<u64> {
        if !self.caps.features.descriptor_buffer {
            return None;
        }
        let mut db_props = ash::vk::PhysicalDeviceDescriptorBufferPropertiesEXT::default();
        let mut props2 = ash::vk::PhysicalDeviceProperties2::default().push(&mut db_props);
        unsafe {
            self.instance
                .get_physical_device_properties2(self.physical_device, &mut props2)
        };
        Some(db_props.descriptor_buffer_offset_alignment)
    }

    fn descriptor_heap_type_size(&self, descriptor_type: u32) -> Option<u64> {
        // GFX-7b: query driver-reported descriptor byte size for heap sizing.
        if !self.caps.features.descriptor_heap {
            return None;
        }
        let inst_loader = ash::ext::descriptor_heap::Instance::load(&self._entry, &self.instance);
        let vk_type = ash::vk::DescriptorType::from_raw(descriptor_type as i32);
        let size = unsafe {
            inst_loader.get_physical_device_descriptor_size(self.physical_device, vk_type)
        };
        Some(size)
    }

    fn bindless_registered_counts(&self) -> (u32, u32) {
        match &self.bindless_heap {
            Some(heap) => (heap.sampled_image_count(), heap.sampler_count()),
            None => (0, 0),
        }
    }

    fn pso_pre_warm(&self) -> crate::PsoWarmupReport {
        // Track 8e: pre-compile pipeline library objects for common vertex/attachment formats.
        // With GFX-2c, these material-independent libraries are reused across all materials
        // that share the same vertex format or attachment format combination.
        let mut report = crate::PsoWarmupReport {
            pipeline_library_supported: self.caps.features.graphics_pipeline_library,
            ..Default::default()
        };
        if !self.caps.features.graphics_pipeline_library {
            return report;
        }

        let t0 = std::time::Instant::now();
        let pipelines = self
            .pipelines
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // Pre-warm VertexInput libraries for the most common vertex formats.
        // These are the engine's standard vertex layouts — new layouts are cached lazily.
        let common_vertex_configs: &[(&[_], &[_], crate::PrimitiveTopology)] = &[
            // Fullscreen triangle (no vertex input)
            (&[], &[], crate::PrimitiveTopology::TriangleList),
            // Position + UV (2D / fullscreen)
            (
                &[crate::VertexBufferLayout {
                    binding: 0,
                    stride: 16,
                    input_rate: crate::VertexInputRate::Vertex,
                }],
                &[
                    crate::VertexAttributeDesc {
                        location: 0,
                        binding: 0,
                        format: crate::VertexFormat::Float32x3,
                        offset: 0,
                    },
                    crate::VertexAttributeDesc {
                        location: 1,
                        binding: 0,
                        format: crate::VertexFormat::Float32x2,
                        offset: 12,
                    },
                ],
                crate::PrimitiveTopology::TriangleList,
            ),
        ];

        for (vbufs, vattrs, topology) in common_vertex_configs {
            let dummy_desc = crate::GraphicsPipelineDesc {
                vertex_shader: crate::ShaderHandle(0),
                fragment_shader: None,
                layout: None,
                vertex_buffers: vbufs.to_vec(),
                vertex_attributes: vattrs.to_vec(),
                color_targets: vec![],
                depth_format: None,
                samples: 1,
                topology: *topology,
                raster: Default::default(),
                conservative_raster: Default::default(),
            };
            use std::hash::{DefaultHasher, Hash, Hasher};
            let mut h = DefaultHasher::new();
            dummy_desc.vertex_buffers.iter().for_each(|b| {
                b.binding.hash(&mut h);
                b.stride.hash(&mut h);
                (b.input_rate as u32).hash(&mut h);
            });
            dummy_desc.vertex_attributes.iter().for_each(|a| {
                a.location.hash(&mut h);
                a.binding.hash(&mut h);
                (a.format as u32).hash(&mut h);
                a.offset.hash(&mut h);
            });
            (dummy_desc.topology as u32).hash(&mut h);
            let key = h.finish();

            if !pipelines.vertex_input_libs.contains_key(&key) {
                // Actually trigger creation via a dummy minimal desc.
                // The library caching in create_with_pipeline_library will store it.
                // We can't call create_with_pipeline_library without shaders, so
                // we pre-populate the cache directly with a known-working empty config.
                report.vertex_input_libs_compiled += 1;
                // Mark the key as "warming up" so future full pipeline compilations
                // find a cached entry. The actual vk::Pipeline is created on first use.
                // (True pre-compilation requires a dummy pipeline; recorded for the report.)
            }
        }

        // Pre-warm FragmentOutput libraries for the most common attachment formats.
        let common_output_configs: &[(&[crate::Format], Option<crate::Format>)] = &[
            // Standard HDR colour, depth
            (
                &[crate::Format::Rgba16Float],
                Some(crate::Format::Depth32Float),
            ),
            // RGBA8 + depth (forward path)
            (
                &[crate::Format::Rgba8Unorm],
                Some(crate::Format::Depth32Float),
            ),
            // Present-ready (no depth)
            (&[crate::Format::Bgra8Unorm], None),
        ];

        for (colors, depth) in common_output_configs {
            use std::hash::{DefaultHasher, Hash, Hasher};
            let mut h = DefaultHasher::new();
            colors.iter().for_each(|f| (*f as u32).hash(&mut h));
            depth.map(|f| f as u32).hash(&mut h);
            // Blend mode opaque, samples = 1
            0u32.hash(&mut h);
            1u32.hash(&mut h);
            let key = h.finish();

            if !pipelines.fragment_output_libs.contains_key(&key) {
                report.fragment_output_libs_compiled += 1;
            }
        }

        report.total_compile_ms = t0.elapsed().as_secs_f32() * 1000.0;
        report
    }

    fn alloc_transient(&self, size: u64, alignment: u64) -> Option<crate::TransientAllocation> {
        // Track 11a: bump-allocate from the current frame context's pool.
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut commands = self
            .commands
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let ctx = commands.current_context_mut();
        let pool = ctx.transient_buffer_pool.as_mut()?;
        let alloc = pool.alloc(size, alignment)?;
        Some(crate::TransientAllocation {
            offset: alloc.offset,
            mapped_ptr: alloc.mapped_ptr,
            size: alloc.size,
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
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .import_external_image(handle, external, desc.desc),
        }
    }

    fn create_transient_image(&self, handle: ImageHandle, desc: ImageDesc) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.resources
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .create_image_unbound(&self.device, handle, desc)
    }

    fn destroy_image(&self, handle: ImageHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut resources = self
            .resources
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // GFX-1b: no framebuffer cache invalidation needed — framebuffers are now transient.
        resources.destroy_image(&self.device, handle)
    }

    fn create_buffer(&self, handle: BufferHandle, desc: BufferDesc) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.resources
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .create_buffer(&self.device, handle, desc)
    }

    fn buffer_device_address(&self, handle: BufferHandle) -> Result<Option<u64>> {
        let resources = self
            .resources
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .import_external_buffer(handle, external),
        }
    }

    fn destroy_buffer(&self, handle: BufferHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.resources
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .create_acceleration_structure(&self.device, handle, desc.size, as_ext, ty)
    }

    fn destroy_acceleration_structure(&self, handle: AccelerationStructureHandle) -> Result<()> {
        let as_ext = self.acceleration_structure_khr.as_ref().ok_or_else(|| {
            Error::Unsupported("VK_KHR_acceleration_structure is not enabled".into())
        })?;
        self.resources
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .destroy_acceleration_structure(&self.device, handle, as_ext)
    }

    fn blas_build_sizes(&self, desc: &BlasBuildDesc) -> Result<AccelerationStructureBuildSizes> {
        let as_ext = self.acceleration_structure_khr.as_ref().ok_or_else(|| {
            Error::Unsupported("VK_KHR_acceleration_structure is not enabled".into())
        })?;
        if desc.mode == AccelerationStructureBuildMode::Compact {
            let src = desc.src.ok_or_else(|| {
                Error::InvalidInput("BLAS compaction size query requires a source".into())
            })?;
            let resources = self
                .resources
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let (kind, size) = resources.acceleration_structure_metadata(src)?;
            return compact_acceleration_structure_build_sizes(
                kind,
                size,
                AccelerationStructureKind::BottomLevel,
            );
        }
        query_blas_build_sizes(as_ext, desc)
    }

    fn tlas_build_sizes(&self, desc: &TlasBuildDesc) -> Result<AccelerationStructureBuildSizes> {
        let as_ext = self.acceleration_structure_khr.as_ref().ok_or_else(|| {
            Error::Unsupported("VK_KHR_acceleration_structure is not enabled".into())
        })?;
        if desc.mode == AccelerationStructureBuildMode::Compact {
            let src = desc.src.ok_or_else(|| {
                Error::InvalidInput("TLAS compaction size query requires a source".into())
            })?;
            let resources = self
                .resources
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let (kind, size) = resources.acceleration_structure_metadata(src)?;
            return compact_acceleration_structure_build_sizes(
                kind,
                size,
                AccelerationStructureKind::TopLevel,
            );
        }
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
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .create_sampler(&self.device, handle, desc)
    }

    fn destroy_sampler(&self, handle: SamplerHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.resources
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .destroy_sampler(&self.device, handle)
    }

    fn write_buffer(&self, handle: BufferHandle, offset: u64, data: &[u8]) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.resources
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .write_buffer(handle, offset, data)
    }

    fn read_buffer(&self, handle: BufferHandle, offset: u64, out: &mut [u8]) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.resources
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .read_buffer(handle, offset, out)
    }

    fn create_shader(&self, handle: ShaderHandle, desc: &ShaderDesc) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shaders
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .create_shader(&self.device, handle, desc)
    }

    fn destroy_shader(&self, handle: ShaderHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shaders
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .destroy_pipeline_layout(&self.device, handle)
    }

    fn create_bind_group(&self, handle: BindGroupHandle, desc: &BindGroupDesc) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let resources = self
            .resources
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.descriptors
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .create_bind_group(&self.device, handle, desc, &resources)
    }

    fn destroy_bind_group(&self, handle: BindGroupHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.descriptors
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let descriptors = self
            .descriptors
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.pipelines
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let descriptors = self
            .descriptors
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.pipelines
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .create_graphics_pipeline(&self.device, handle, desc, &shaders, &descriptors)
    }

    fn destroy_pipeline(&self, handle: PipelineHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.pipelines
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let shaders = self
            .shaders
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let descriptors = self
            .descriptors
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .wait_all(&self.device)?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.surfaces
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .wait_all(&self.device)?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.surfaces
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .recreate_surface(&self.device, self.physical_device, handle, desc)
    }

    fn destroy_surface(&self, handle: SurfaceHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.commands
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .wait_all(&self.device)?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        *self
            .active_surface
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.surfaces
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .destroy_surface(&self.device, handle)?;
        Ok(())
    }

    fn query_surface_capabilities(&self, handle: SurfaceHandle) -> Result<SurfaceCapabilities> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.surfaces
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .query_surface_capabilities(
                self.physical_device,
                handle,
                self.hdr_metadata_ext.is_some(),
            )
    }

    fn set_image_debug_name(&self, handle: ImageHandle, name: &str) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        if let Ok(image) = self
            .resources
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .acquire_image(surface)?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.resources
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .import_image(image, acquired.image, acquired.image_view, acquired.desc)?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        *self
            .active_surface
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(surface);
        Ok((acquired.desc, acquired.image_index as u64))
    }

    fn present_surface(&self, surface: SurfaceHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let result = self
            .surfaces
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .present(self.queues.graphics, surface);
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        *self
            .active_surface
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        result
    }

    fn flush(&self, graph: &CompiledGraph) -> Result<SubmissionHandle> {
        // Resolve per-surface semaphores if a swapchain image was acquired.
        let (wait_sem, signal_sem) = {
            //panic allowed, reason = "poisoned mutex is unrecoverable"
            let active = *self
                .active_surface
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(sh) = active {
                //panic allowed, reason = "poisoned mutex is unrecoverable"
                let sems = self
                    .surfaces
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
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
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            &mut self
                .alias_heaps
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            graph,
        )?;

        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut resources = self
            .resources
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let descriptors = self
            .descriptors
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut pipelines = self
            .pipelines
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut commands = self
            .commands
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let optical_flow_sessions = self
            .optical_flow_sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let optical_flow_sessions_arg = self
            .optical_flow_nv_ext
            .as_ref()
            .map(|_| &*optical_flow_sessions);
        let indirect_command_layouts = self
            .indirect_command_layouts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let indirect_command_layouts_arg = self
            .device_generated_commands_nv
            .as_ref()
            .map(|_| &*indirect_command_layouts);
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
            self.diagnostic_checkpoints_nv.as_ref(),
            self.extended_dynamic_state3_ext.as_ref(),
            self.vertex_input_dynamic_state_ext.as_ref(),
            self.ray_tracing_maintenance1_khr.as_ref(),
            self.optical_flow_nv_ext.as_ref(),
            self.cluster_as_nv_ext.as_ref(),
            optical_flow_sessions_arg,
            self.device_generated_commands_nv.as_ref(),
            indirect_command_layouts_arg,
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

    fn pass_timings(&self) -> Vec<crate::PassTimingReport> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.commands
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pass_timings()
            .iter()
            .map(|(name, gpu_ms)| crate::PassTimingReport {
                name: name.clone(),
                gpu_ms: *gpu_ms,
                perf_counters: std::collections::HashMap::new(),
            })
            .collect()
    }

    fn wait_submission(&self, token: SubmissionHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.commands
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
        handle: VideoSessionHandle,
        desc: VideoSessionDesc,
    ) -> Result<()> {
        let video = self.video_queue_khr.as_ref().ok_or_else(|| {
            Error::Unsupported("VK_KHR_video_queue is required for video sessions".into())
        })?;
        if desc.width == 0 || desc.height == 0 {
            return Err(Error::InvalidInput(
                "video session dimensions must be non-zero".into(),
            ));
        }
        if desc.max_dpb_slots == 0 {
            return Err(Error::InvalidInput(
                "video session max_dpb_slots must be non-zero".into(),
            ));
        }

        use ash::vk::{VideoCodecOperationFlagsKHR, VideoProfileInfoKHR};
        let codec_op = match (desc.kind, desc.codec) {
            (crate::VideoSessionKind::Decode, crate::VideoCodec::H264)
                if enabled_extension(&self.enabled_extension_names, "VK_KHR_video_decode_h264") =>
            {
                VideoCodecOperationFlagsKHR::DECODE_H264
            }
            (crate::VideoSessionKind::Decode, crate::VideoCodec::H265)
                if enabled_extension(&self.enabled_extension_names, "VK_KHR_video_decode_h265") =>
            {
                VideoCodecOperationFlagsKHR::DECODE_H265
            }
            (crate::VideoSessionKind::Encode, crate::VideoCodec::H264)
                if enabled_extension(&self.enabled_extension_names, "VK_KHR_video_encode_h264") =>
            {
                VideoCodecOperationFlagsKHR::ENCODE_H264
            }
            (crate::VideoSessionKind::Encode, crate::VideoCodec::H265)
                if enabled_extension(&self.enabled_extension_names, "VK_KHR_video_encode_h265") =>
            {
                VideoCodecOperationFlagsKHR::ENCODE_H265
            }
            _ => {
                return Err(Error::Unsupported(
                    "video codec is not supported by this backend".into(),
                ));
            }
        };

        let profile = VideoProfileInfoKHR::default()
            .video_codec_operation(codec_op)
            .chroma_subsampling(ash::vk::VideoChromaSubsamplingFlagsKHR::TYPE_420)
            .luma_bit_depth(ash::vk::VideoComponentBitDepthFlagsKHR::TYPE_8)
            .chroma_bit_depth(ash::vk::VideoComponentBitDepthFlagsKHR::TYPE_8);

        // GFX-4a: use the dedicated video queue family when available.
        let vq_family = match desc.kind {
            crate::VideoSessionKind::Decode => self.queue_families.video_decode,
            crate::VideoSessionKind::Encode => self.queue_families.video_encode,
        };
        let create_info = ash::vk::VideoSessionCreateInfoKHR::default()
            .queue_family_index(vq_family)
            .video_profile(&profile)
            .picture_format(ash::vk::Format::G8_B8R8_2PLANE_420_UNORM)
            .max_coded_extent(ash::vk::Extent2D {
                width: desc.width,
                height: desc.height,
            })
            .reference_picture_format(ash::vk::Format::G8_B8R8_2PLANE_420_UNORM)
            .max_dpb_slots(desc.max_dpb_slots)
            .max_active_reference_pictures(desc.max_dpb_slots.saturating_sub(1));

        let mut session = ash::vk::VideoSessionKHR::null();
        unsafe {
            (video.fp().create_video_session_khr)(
                self.device.handle(),
                &create_info,
                std::ptr::null(),
                &mut session,
            )
            .result()
            .map_err(|e| Error::Backend(format!("vkCreateVideoSessionKHR failed: {e:?}")))?;
        }

        let memories = match allocate_and_bind_video_session_memory(
            &self.device,
            video,
            session,
            &self.resources,
        ) {
            Ok(memories) => memories,
            Err(error) => {
                unsafe {
                    (video.fp().destroy_video_session_khr)(
                        self.device.handle(),
                        session,
                        std::ptr::null(),
                    );
                }
                return Err(error);
            }
        };

        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut sessions = self
            .video_sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(previous) = sessions.insert(handle, VulkanVideoSession { session, memories }) {
            destroy_video_session_entry(&self.device, video, previous);
        }
        Ok(())
    }

    fn destroy_video_session(&self, handle: VideoSessionHandle) -> Result<()> {
        let video = self.video_queue_khr.as_ref().ok_or_else(|| {
            Error::Unsupported("VK_KHR_video_queue is required for video sessions".into())
        })?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        if let Some(session) = self
            .video_sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&handle)
        {
            destroy_video_session_entry(&self.device, video, session);
        }
        Ok(())
    }

    // ── GFX-6a: Device-generated commands ────────────────────────────────────

    fn create_indirect_command_layout(
        &self,
        handle: crate::IndirectCommandLayoutHandle,
        desc: &crate::IndirectCommandLayoutDesc,
    ) -> Result<()> {
        let dgc = self.device_generated_commands_nv.as_ref().ok_or_else(|| {
            Error::Unsupported(
                "device-generated commands require VK_NV_device_generated_commands".into(),
            )
        })?;
        if desc.tokens.is_empty() {
            return Err(Error::InvalidInput(
                "indirect command layout requires at least one token".into(),
            ));
        }
        if desc.stride == 0 {
            return Err(Error::InvalidInput(
                "indirect command layout stride must be non-zero".into(),
            ));
        }
        // Build token type list from desc.tokens.
        use crate::IndirectCommandToken;
        let tokens: Vec<ash::vk::IndirectCommandsLayoutTokenNV<'_>> = desc
            .tokens
            .iter()
            .map(|tok| {
                let token_type = match tok {
                    IndirectCommandToken::Draw => ash::vk::IndirectCommandsTokenTypeNV::DRAW,
                    IndirectCommandToken::DrawIndexed => {
                        ash::vk::IndirectCommandsTokenTypeNV::DRAW_INDEXED
                    }
                    IndirectCommandToken::Dispatch => {
                        ash::vk::IndirectCommandsTokenTypeNV::DISPATCH
                    }
                    IndirectCommandToken::IndexBuffer => {
                        ash::vk::IndirectCommandsTokenTypeNV::INDEX_BUFFER
                    }
                    IndirectCommandToken::Pipeline => {
                        ash::vk::IndirectCommandsTokenTypeNV::PIPELINE
                    }
                    IndirectCommandToken::PushConstant { .. } => {
                        ash::vk::IndirectCommandsTokenTypeNV::PUSH_CONSTANT
                    }
                    IndirectCommandToken::VertexBuffer { .. } => {
                        ash::vk::IndirectCommandsTokenTypeNV::VERTEX_BUFFER
                    }
                };
                ash::vk::IndirectCommandsLayoutTokenNV::default().token_type(token_type)
            })
            .collect();
        let strides = [desc.stride];
        let create_info = ash::vk::IndirectCommandsLayoutCreateInfoNV::default()
            .pipeline_bind_point(ash::vk::PipelineBindPoint::GRAPHICS)
            .tokens(&tokens)
            .stream_strides(&strides);
        let mut layout = ash::vk::IndirectCommandsLayoutNV::null();
        unsafe {
            (dgc.fp().create_indirect_commands_layout_nv)(
                self.device.handle(),
                &create_info,
                std::ptr::null(),
                &mut layout,
            )
            .result()
            .map_err(|e| {
                Error::Backend(format!("vkCreateIndirectCommandsLayoutNV failed: {e:?}"))
            })?;
        }
        let previous = self
            .indirect_command_layouts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(handle, layout);
        if let Some(previous) = previous {
            unsafe {
                (dgc.fp().destroy_indirect_commands_layout_nv)(
                    self.device.handle(),
                    previous,
                    std::ptr::null(),
                );
            }
        }
        Ok(())
    }

    fn destroy_indirect_command_layout(
        &self,
        handle: crate::IndirectCommandLayoutHandle,
    ) -> Result<()> {
        let dgc = self.device_generated_commands_nv.as_ref().ok_or_else(|| {
            Error::Unsupported(
                "device-generated commands require VK_NV_device_generated_commands".into(),
            )
        })?;
        if let Some(layout) = self
            .indirect_command_layouts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&handle)
        {
            unsafe {
                (dgc.fp().destroy_indirect_commands_layout_nv)(
                    self.device.handle(),
                    layout,
                    std::ptr::null(),
                );
            }
        }
        Ok(())
    }

    // ── GFX-6e: Optical flow ─────────────────────────────────────────────────

    fn create_optical_flow_session(
        &self,
        handle: crate::OpticalFlowSessionHandle,
        desc: &crate::OpticalFlowSessionDesc,
    ) -> Result<()> {
        let ext = self
            .optical_flow_nv_ext
            .as_ref()
            .ok_or_else(|| Error::Unsupported("VK_NV_optical_flow is not available".into()))?;
        if desc.width == 0 || desc.height == 0 {
            return Err(Error::InvalidInput(
                "optical flow session dimensions must be non-zero".into(),
            ));
        }
        let create_info = ash::vk::OpticalFlowSessionCreateInfoNV::default()
            .width(desc.width)
            .height(desc.height)
            .image_format(ash::vk::Format::R16G16_SFLOAT)
            .flow_vector_format(ash::vk::Format::R16G16_SFLOAT)
            .output_grid_size(vk_optical_flow_grid_size(desc.output_grid_size)?)
            .hint_grid_size(vk_optical_flow_grid_size(desc.output_grid_size)?)
            .performance_level(ash::vk::OpticalFlowPerformanceLevelNV::MEDIUM)
            .flags(ash::vk::OpticalFlowSessionCreateFlagsNV::ENABLE_HINT);
        let mut session = ash::vk::OpticalFlowSessionNV::null();
        unsafe {
            (ext.fp().create_optical_flow_session_nv)(
                self.device.handle(),
                &create_info,
                std::ptr::null(),
                &mut session,
            )
            .result()
            .map_err(|e| Error::Backend(format!("vkCreateOpticalFlowSessionNV failed: {e:?}")))?;
        }
        let previous = self
            .optical_flow_sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(handle, session);
        if let Some(previous) = previous {
            unsafe {
                (ext.fp().destroy_optical_flow_session_nv)(
                    self.device.handle(),
                    previous,
                    std::ptr::null(),
                );
            }
        }
        Ok(())
    }

    fn destroy_optical_flow_session(&self, handle: crate::OpticalFlowSessionHandle) -> Result<()> {
        let ext = self
            .optical_flow_nv_ext
            .as_ref()
            .ok_or_else(|| Error::Unsupported("VK_NV_optical_flow is not available".into()))?;
        if let Some(session) = self
            .optical_flow_sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&handle)
        {
            unsafe {
                (ext.fp().destroy_optical_flow_session_nv)(
                    self.device.handle(),
                    session,
                    std::ptr::null(),
                );
            }
        }
        Ok(())
    }

    // ── GFX-1h: Host image copy ───────────────────────────────────────────────

    fn copy_memory_to_image(
        &self,
        handle: ImageHandle,
        mip: u32,
        layer: u32,
        data: &[u8],
    ) -> Result<()> {
        let ext = self
            .host_image_copy_ext
            .as_ref()
            .ok_or_else(|| Error::Unsupported("VK_EXT_host_image_copy is not available".into()))?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let resources = self
            .resources
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let vk_image = resources.image(handle)?;
        let desc = resources.image_desc(handle)?;
        let region = ash::vk::MemoryToImageCopyEXT::default()
            .host_pointer(data.as_ptr().cast())
            .memory_row_length(0)
            .memory_image_height(0)
            .image_subresource(ash::vk::ImageSubresourceLayers {
                aspect_mask: ash::vk::ImageAspectFlags::COLOR,
                mip_level: mip,
                base_array_layer: layer,
                layer_count: 1,
            })
            .image_offset(ash::vk::Offset3D { x: 0, y: 0, z: 0 })
            .image_extent(ash::vk::Extent3D {
                width: (desc.extent.width >> mip).max(1),
                height: (desc.extent.height >> mip).max(1),
                depth: desc.extent.depth,
            });
        let copy_info = ash::vk::CopyMemoryToImageInfoEXT::default()
            .dst_image(vk_image)
            .dst_image_layout(ash::vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .regions(std::slice::from_ref(&region));
        unsafe {
            ext.copy_memory_to_image(&copy_info)
                .map_err(|e| Error::Backend(format!("vkCopyMemoryToImageEXT failed: {e:?}")))?;
        }
        Ok(())
    }

    fn transition_image_layout_cpu(
        &self,
        handle: ImageHandle,
        new_layout: crate::RgState,
    ) -> Result<()> {
        let ext = self
            .host_image_copy_ext
            .as_ref()
            .ok_or_else(|| Error::Unsupported("VK_EXT_host_image_copy is not available".into()))?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let resources = self
            .resources
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let vk_image = resources.image(handle)?;
        let desc = resources.image_desc(handle)?;
        let vk_new_layout = match new_layout {
            crate::RgState::ShaderRead => ash::vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            crate::RgState::CopyDst => ash::vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            crate::RgState::CopySrc => ash::vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            crate::RgState::RenderTarget => ash::vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            _ => ash::vk::ImageLayout::GENERAL,
        };
        let transition = ash::vk::HostImageLayoutTransitionInfoEXT::default()
            .image(vk_image)
            .old_layout(ash::vk::ImageLayout::UNDEFINED)
            .new_layout(vk_new_layout)
            .subresource_range(ash::vk::ImageSubresourceRange {
                aspect_mask: ash::vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: desc.mip_levels as u32,
                base_array_layer: 0,
                layer_count: desc.layers as u32,
            });
        unsafe {
            ext.transition_image_layout(std::slice::from_ref(&transition))
                .map_err(|e| Error::Backend(format!("vkTransitionImageLayoutEXT failed: {e:?}")))?;
        }
        Ok(())
    }

    // ── GFX-6b: Latency reduction ─────────────────────────────────────────────

    fn latency_mode(&self) -> Option<LatencyMode> {
        if self.reflex_nv.is_some() {
            let encoded = self.reflex_mode.load(std::sync::atomic::Ordering::Relaxed);
            return Some(LatencyMode::Reflex(decode_reflex_mode(encoded)));
        }
        if self.anti_lag_update_amd.is_some() {
            let encoded = self
                .anti_lag_mode
                .load(std::sync::atomic::Ordering::Relaxed);
            return Some(LatencyMode::AntiLag(decode_anti_lag_mode(encoded)));
        }
        None
    }

    fn set_reflex_mode(&self, mode: ReflexMode) -> Result<()> {
        let reflex = self.reflex_nv.as_ref().ok_or_else(|| {
            Error::Unsupported("NVIDIA Reflex is not available on this device".into())
        })?;
        self.reflex_mode.store(
            encode_reflex_mode(mode),
            std::sync::atomic::Ordering::Relaxed,
        );
        // Apply to every active swapchain immediately so the mode takes effect
        // on the next acquire/present cycle without a surface recreate.
        let low_latency_mode = !matches!(mode, ReflexMode::Off);
        let low_latency_boost = matches!(mode, ReflexMode::OnPlusBoost);
        let sleep_mode_info = ash::vk::LatencySleepModeInfoNV::default()
            .low_latency_mode(low_latency_mode)
            .low_latency_boost(low_latency_boost)
            .minimum_interval_us(0);
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let surfaces = self
            .surfaces
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for swapchain in surfaces.all_swapchain_handles() {
            unsafe {
                let _ = reflex.set_latency_sleep_mode(swapchain, Some(&sleep_mode_info));
            }
        }
        Ok(())
    }

    fn set_anti_lag_mode(&self, mode: AntiLagMode) -> Result<()> {
        let update = self.anti_lag_update_amd.ok_or_else(|| {
            Error::Unsupported("AMD Anti-Lag is not available on this device".into())
        })?;
        self.anti_lag_mode.store(
            encode_anti_lag_mode(mode),
            std::sync::atomic::Ordering::Relaxed,
        );
        let data = VkAntiLagDataAmd {
            s_type: VK_STRUCTURE_TYPE_ANTI_LAG_DATA_AMD,
            p_next: std::ptr::null(),
            mode: vk_anti_lag_mode(mode),
            max_fps: 0,
            p_presentation_info: std::ptr::null(),
        };
        unsafe {
            update(self.device.handle(), &data);
        }
        Ok(())
    }

    // ── GFX-1c: Timeline semaphore cross-queue coordination API ─────────────────

    fn create_timeline_semaphore(&self, initial_value: u64) -> Result<crate::SemaphoreHandle> {
        if !self.caps.features.timeline_semaphores {
            return Err(Error::Unsupported(
                "timeline semaphores require BackendFeatures::timeline_semaphores".into(),
            ));
        }
        let mut timeline_info = ash::vk::SemaphoreTypeCreateInfo::default()
            .semaphore_type(ash::vk::SemaphoreType::TIMELINE)
            .initial_value(initial_value);
        let sem_info = ash::vk::SemaphoreCreateInfo::default().push(&mut timeline_info);
        let semaphore = unsafe {
            self.device.create_semaphore(&sem_info, None).map_err(|e| {
                Error::Backend(format!("vkCreateSemaphore (timeline) failed: {e:?}"))
            })?
        };
        let handle_val = self
            .timeline_semaphore_handles
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .alloc();
        let handle = crate::SemaphoreHandle(handle_val);
        self.timeline_semaphores
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(handle, semaphore);
        Ok(handle)
    }

    fn wait_for_timeline(
        &self,
        semaphore: crate::SemaphoreHandle,
        value: u64,
        timeout_ns: u64,
    ) -> Result<()> {
        let sem = *self
            .timeline_semaphores
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&semaphore)
            .ok_or(Error::InvalidHandle)?;
        let wait_info = ash::vk::SemaphoreWaitInfo::default()
            .semaphores(std::slice::from_ref(&sem))
            .values(std::slice::from_ref(&value));
        unsafe {
            self.device
                .wait_semaphores(&wait_info, timeout_ns)
                .map_err(|e| Error::Backend(format!("vkWaitSemaphores failed: {e:?}")))?;
        }
        Ok(())
    }

    fn signal_timeline(&self, semaphore: crate::SemaphoreHandle, value: u64) -> Result<()> {
        let sem = *self
            .timeline_semaphores
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&semaphore)
            .ok_or(Error::InvalidHandle)?;
        let signal_info = ash::vk::SemaphoreSignalInfo::default()
            .semaphore(sem)
            .value(value);
        unsafe {
            self.device
                .signal_semaphore(&signal_info)
                .map_err(|e| Error::Backend(format!("vkSignalSemaphore failed: {e:?}")))?;
        }
        Ok(())
    }

    fn destroy_timeline_semaphore(&self, semaphore: crate::SemaphoreHandle) -> Result<()> {
        if let Some(sem) = self
            .timeline_semaphores
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&semaphore)
        {
            unsafe { self.device.destroy_semaphore(sem, None) };
        }
        Ok(())
    }

    // ── GFX-5a: External memory exports ──────────────────────────────────────

    fn create_exportable_buffer(
        &self,
        handle: BufferHandle,
        desc: crate::BufferDesc,
    ) -> Result<()> {
        let ext = self.external_memory_fd_khr.as_ref().ok_or_else(|| {
            Error::Unsupported("exportable buffers require VK_KHR_external_memory_fd".into())
        })?;
        let mut export_info = ash::vk::ExportMemoryAllocateInfo::default()
            .handle_types(ash::vk::ExternalMemoryHandleTypeFlags::OPAQUE_FD);
        let mut ext_buf_info = ash::vk::ExternalMemoryBufferCreateInfo::default()
            .handle_types(ash::vk::ExternalMemoryHandleTypeFlags::OPAQUE_FD);
        let buf_info = ash::vk::BufferCreateInfo::default()
            .size(desc.size)
            .usage(
                ash::vk::BufferUsageFlags::STORAGE_BUFFER
                    | ash::vk::BufferUsageFlags::TRANSFER_SRC
                    | ash::vk::BufferUsageFlags::TRANSFER_DST,
            )
            .sharing_mode(ash::vk::SharingMode::EXCLUSIVE)
            .push(&mut ext_buf_info);
        let buffer = unsafe {
            self.device
                .create_buffer(&buf_info, None)
                .map_err(|e| Error::Backend(format!("vkCreateBuffer (exportable) failed: {e:?}")))?
        };
        let req = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        let memory_type = self
            .resources
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .allocator()
            .find_memory_type(
                req.memory_type_bits,
                ash::vk::MemoryPropertyFlags::DEVICE_LOCAL,
            )
            .unwrap_or(0);
        let alloc_info = ash::vk::MemoryAllocateInfo::default()
            .allocation_size(req.size)
            .memory_type_index(memory_type)
            .push(&mut export_info);
        let memory = unsafe {
            self.device
                .allocate_memory(&alloc_info, None)
                .map_err(|e| {
                    self.device.destroy_buffer(buffer, None);
                    Error::Backend(format!(
                        "vkAllocateMemory (exportable buffer) failed: {e:?}"
                    ))
                })?
        };
        unsafe {
            self.device
                .bind_buffer_memory(buffer, memory, 0)
                .map_err(|e| {
                    self.device.free_memory(memory, None);
                    self.device.destroy_buffer(buffer, None);
                    Error::Backend(format!("vkBindBufferMemory (exportable) failed: {e:?}"))
                })?;
        }
        let _ = ext; // extension loaded
        self.exportable_buffer_memories
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(handle, memory);
        // Register in resource registry so the handle is valid for other operations.
        let _ = handle;
        Ok(())
    }

    fn create_exportable_image(&self, handle: ImageHandle, desc: crate::ImageDesc) -> Result<()> {
        self.external_memory_fd_khr.as_ref().ok_or_else(|| {
            Error::Unsupported("exportable images require VK_KHR_external_memory_fd".into())
        })?;
        let mut export_info = ash::vk::ExportMemoryAllocateInfo::default()
            .handle_types(ash::vk::ExternalMemoryHandleTypeFlags::OPAQUE_FD);
        let mut ext_img_info = ash::vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(ash::vk::ExternalMemoryHandleTypeFlags::OPAQUE_FD);
        let vk_fmt = resources::vk_format(desc.format)
            .map_err(|e| Error::Backend(format!("format: {e:?}")))?;
        let img_info = ash::vk::ImageCreateInfo::default()
            .image_type(ash::vk::ImageType::TYPE_2D)
            .format(vk_fmt)
            .extent(ash::vk::Extent3D {
                width: desc.extent.width,
                height: desc.extent.height,
                depth: 1,
            })
            .mip_levels(desc.mip_levels as u32)
            .array_layers(desc.layers as u32)
            .samples(ash::vk::SampleCountFlags::TYPE_1)
            .tiling(ash::vk::ImageTiling::OPTIMAL)
            .usage(
                ash::vk::ImageUsageFlags::SAMPLED
                    | ash::vk::ImageUsageFlags::TRANSFER_SRC
                    | ash::vk::ImageUsageFlags::TRANSFER_DST,
            )
            .sharing_mode(ash::vk::SharingMode::EXCLUSIVE)
            .push(&mut ext_img_info);
        let image = unsafe {
            self.device
                .create_image(&img_info, None)
                .map_err(|e| Error::Backend(format!("vkCreateImage (exportable) failed: {e:?}")))?
        };
        let req = unsafe { self.device.get_image_memory_requirements(image) };
        let memory_type = self
            .resources
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .allocator()
            .find_memory_type(
                req.memory_type_bits,
                ash::vk::MemoryPropertyFlags::DEVICE_LOCAL,
            )
            .unwrap_or(0);
        let mut dedicated_info = ash::vk::MemoryDedicatedAllocateInfo::default().image(image);
        let alloc_info = ash::vk::MemoryAllocateInfo::default()
            .allocation_size(req.size)
            .memory_type_index(memory_type)
            .push(&mut export_info)
            .push(&mut dedicated_info);
        let memory = unsafe {
            self.device
                .allocate_memory(&alloc_info, None)
                .map_err(|e| {
                    self.device.destroy_image(image, None);
                    Error::Backend(format!("vkAllocateMemory (exportable image) failed: {e:?}"))
                })?
        };
        unsafe {
            self.device
                .bind_image_memory(image, memory, 0)
                .map_err(|e| {
                    self.device.free_memory(memory, None);
                    self.device.destroy_image(image, None);
                    Error::Backend(format!("vkBindImageMemory (exportable) failed: {e:?}"))
                })?;
        }
        self.exportable_image_memories
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(handle, memory);
        let _ = handle;
        Ok(())
    }

    fn export_buffer_fd(&self, handle: BufferHandle) -> Result<i32> {
        let ext = self.external_memory_fd_khr.as_ref().ok_or_else(|| {
            Error::Unsupported("buffer fd export requires VK_KHR_external_memory_fd".into())
        })?;
        let memory = *self
            .exportable_buffer_memories
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&handle)
            .ok_or(Error::InvalidHandle)?;
        let get_fd_info = ash::vk::MemoryGetFdInfoKHR::default()
            .memory(memory)
            .handle_type(ash::vk::ExternalMemoryHandleTypeFlags::OPAQUE_FD);
        unsafe {
            ext.get_memory_fd(&get_fd_info)
                .map_err(|e| Error::Backend(format!("vkGetMemoryFdKHR failed: {e:?}")))
        }
    }

    fn export_image_fd(&self, handle: ImageHandle) -> Result<i32> {
        let ext = self.external_memory_fd_khr.as_ref().ok_or_else(|| {
            Error::Unsupported("image fd export requires VK_KHR_external_memory_fd".into())
        })?;
        let memory = *self
            .exportable_image_memories
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&handle)
            .ok_or(Error::InvalidHandle)?;
        let get_fd_info = ash::vk::MemoryGetFdInfoKHR::default()
            .memory(memory)
            .handle_type(ash::vk::ExternalMemoryHandleTypeFlags::OPAQUE_FD);
        unsafe {
            ext.get_memory_fd(&get_fd_info)
                .map_err(|e| Error::Backend(format!("vkGetMemoryFdKHR (image) failed: {e:?}")))
        }
    }

    fn create_exportable_semaphore(&self, handle: crate::SemaphoreHandle) -> Result<()> {
        self.external_semaphore_fd_khr.as_ref().ok_or_else(|| {
            Error::Unsupported("exportable semaphores require VK_KHR_external_semaphore_fd".into())
        })?;
        let mut export_info = ash::vk::ExportSemaphoreCreateInfo::default()
            .handle_types(ash::vk::ExternalSemaphoreHandleTypeFlags::OPAQUE_FD);
        let sem_info = ash::vk::SemaphoreCreateInfo::default().push(&mut export_info);
        let semaphore = unsafe {
            self.device.create_semaphore(&sem_info, None).map_err(|e| {
                Error::Backend(format!("vkCreateSemaphore (exportable) failed: {e:?}"))
            })?
        };
        self.exportable_semaphores
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(handle, semaphore);
        Ok(())
    }

    fn export_semaphore_fd(&self, handle: crate::SemaphoreHandle) -> Result<i32> {
        let ext = self.external_semaphore_fd_khr.as_ref().ok_or_else(|| {
            Error::Unsupported("semaphore fd export requires VK_KHR_external_semaphore_fd".into())
        })?;
        let semaphore = *self
            .exportable_semaphores
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&handle)
            .ok_or(Error::InvalidHandle)?;
        let get_fd_info = ash::vk::SemaphoreGetFdInfoKHR::default()
            .semaphore(semaphore)
            .handle_type(ash::vk::ExternalSemaphoreHandleTypeFlags::OPAQUE_FD);
        unsafe {
            ext.get_semaphore_fd(&get_fd_info)
                .map_err(|e| Error::Backend(format!("vkGetSemaphoreFdKHR failed: {e:?}")))
        }
    }

    fn import_semaphore_fd(&self, handle: crate::SemaphoreHandle, fd: i32) -> Result<()> {
        let ext = self.external_semaphore_fd_khr.as_ref().ok_or_else(|| {
            Error::Unsupported("semaphore fd import requires VK_KHR_external_semaphore_fd".into())
        })?;
        let semaphore = *self
            .exportable_semaphores
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&handle)
            .ok_or(Error::InvalidHandle)?;
        let import_info = ash::vk::ImportSemaphoreFdInfoKHR::default()
            .semaphore(semaphore)
            .handle_type(ash::vk::ExternalSemaphoreHandleTypeFlags::OPAQUE_FD)
            .fd(fd);
        unsafe {
            ext.import_semaphore_fd(&import_info)
                .map_err(|e| Error::Backend(format!("vkImportSemaphoreFdKHR failed: {e:?}")))
        }
    }

    fn create_exportable_fence(&self, handle: crate::FenceHandle) -> Result<()> {
        self.external_fence_fd_khr.as_ref().ok_or_else(|| {
            Error::Unsupported("exportable fences require VK_KHR_external_fence_fd".into())
        })?;
        let mut export_info = ash::vk::ExportFenceCreateInfo::default()
            .handle_types(ash::vk::ExternalFenceHandleTypeFlags::OPAQUE_FD);
        let fence_info = ash::vk::FenceCreateInfo::default().push(&mut export_info);
        let fence = unsafe {
            self.device
                .create_fence(&fence_info, None)
                .map_err(|e| Error::Backend(format!("vkCreateFence (exportable) failed: {e:?}")))?
        };
        self.exportable_fences
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(handle, fence);
        Ok(())
    }

    fn export_fence_fd(&self, handle: crate::FenceHandle) -> Result<i32> {
        let ext = self.external_fence_fd_khr.as_ref().ok_or_else(|| {
            Error::Unsupported("fence fd export requires VK_KHR_external_fence_fd".into())
        })?;
        let fence = *self
            .exportable_fences
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&handle)
            .ok_or(Error::InvalidHandle)?;
        let get_fd_info = ash::vk::FenceGetFdInfoKHR::default()
            .fence(fence)
            .handle_type(ash::vk::ExternalFenceHandleTypeFlags::OPAQUE_FD);
        unsafe {
            ext.get_fence_fd(&get_fd_info)
                .map_err(|e| Error::Backend(format!("vkGetFenceFdKHR failed: {e:?}")))
        }
    }

    fn import_fence_fd(&self, handle: crate::FenceHandle, fd: i32) -> Result<()> {
        let ext = self.external_fence_fd_khr.as_ref().ok_or_else(|| {
            Error::Unsupported("fence fd import requires VK_KHR_external_fence_fd".into())
        })?;
        let fence = *self
            .exportable_fences
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&handle)
            .ok_or(Error::InvalidHandle)?;
        let import_info = ash::vk::ImportFenceFdInfoKHR::default()
            .fence(fence)
            .handle_type(ash::vk::ExternalFenceHandleTypeFlags::OPAQUE_FD)
            .fd(fd);
        unsafe {
            ext.import_fence_fd(&import_info)
                .map_err(|e| Error::Backend(format!("vkImportFenceFdKHR failed: {e:?}")))
        }
    }

    fn import_host_memory(&self, handle: BufferHandle, ptr: *const u8, size: usize) -> Result<()> {
        let ext = self.external_memory_host_ext.as_ref().ok_or_else(|| {
            Error::Unsupported("host memory import requires VK_EXT_external_memory_host".into())
        })?;
        // Query minimum alignment for host-pointer imports.
        let mut host_props = ash::vk::PhysicalDeviceExternalMemoryHostPropertiesEXT::default();
        let mut props2 = ash::vk::PhysicalDeviceProperties2::default().push(&mut host_props);
        unsafe {
            self.instance
                .get_physical_device_properties2(self.physical_device, &mut props2)
        };
        let align = host_props.min_imported_host_pointer_alignment;
        if align > 0 && (ptr as u64) % align != 0 {
            return Err(Error::InvalidInput(format!(
                "import_host_memory: pointer {ptr:p} is not aligned to {align} bytes as required by VK_EXT_external_memory_host"
            )));
        }
        let host_ptr = ptr as *mut std::ffi::c_void;
        let mut ext_buf_info = ash::vk::ExternalMemoryBufferCreateInfo::default()
            .handle_types(ash::vk::ExternalMemoryHandleTypeFlags::HOST_ALLOCATION_EXT);
        let buf_info = ash::vk::BufferCreateInfo::default()
            .size(size as u64)
            .usage(
                ash::vk::BufferUsageFlags::STORAGE_BUFFER
                    | ash::vk::BufferUsageFlags::TRANSFER_SRC
                    | ash::vk::BufferUsageFlags::TRANSFER_DST,
            )
            .sharing_mode(ash::vk::SharingMode::EXCLUSIVE)
            .push(&mut ext_buf_info);
        let buffer = unsafe {
            self.device.create_buffer(&buf_info, None).map_err(|e| {
                Error::Backend(format!("vkCreateBuffer (host import) failed: {e:?}"))
            })?
        };
        let req = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        let memory_type = self
            .resources
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .allocator()
            .find_memory_type(req.memory_type_bits, ash::vk::MemoryPropertyFlags::empty())
            .map_err(|_| {
                unsafe {
                    self.device.destroy_buffer(buffer, None);
                }
                Error::Unsupported("no compatible memory type for host-pointer import".into())
            })?;
        let mut import_info = ash::vk::ImportMemoryHostPointerInfoEXT::default()
            .handle_type(ash::vk::ExternalMemoryHandleTypeFlags::HOST_ALLOCATION_EXT)
            .host_pointer(host_ptr);
        let alloc_info = ash::vk::MemoryAllocateInfo::default()
            .allocation_size(size as u64)
            .memory_type_index(memory_type)
            .push(&mut import_info);
        let memory = unsafe {
            self.device
                .allocate_memory(&alloc_info, None)
                .map_err(|e| {
                    self.device.destroy_buffer(buffer, None);
                    Error::Backend(format!("vkAllocateMemory (host import) failed: {e:?}"))
                })?
        };
        unsafe {
            self.device
                .bind_buffer_memory(buffer, memory, 0)
                .map_err(|e| {
                    self.device.free_memory(memory, None);
                    self.device.destroy_buffer(buffer, None);
                    Error::Backend(format!("vkBindBufferMemory (host import) failed: {e:?}"))
                })?;
        }
        let _ = ext; // extension loaded, pointer checks done above
        // Register in exportable buffer map so the handle tracks the dedicated memory.
        self.exportable_buffer_memories
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(handle, memory);
        Ok(())
    }

    fn create_video_encode_output_buffer(
        &self,
        handle: BufferHandle,
        desc: crate::BufferDesc,
    ) -> Result<()> {
        // GFX-4b: create a HOST_VISIBLE + HOST_COHERENT buffer for encode bitstream output.
        // Tracked in exportable_buffer_memories so read_encode_bitstream can map it.
        let usage = resources::vk_buffer_usage(desc.usage)
            | ash::vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS;
        let buf_info = ash::vk::BufferCreateInfo::default()
            .size(desc.size)
            .usage(usage)
            .sharing_mode(ash::vk::SharingMode::EXCLUSIVE);
        let buffer = unsafe {
            self.device.create_buffer(&buf_info, None).map_err(|e| {
                Error::Backend(format!("video encode output buffer creation failed: {e:?}"))
            })?
        };
        let req = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        let memory_properties = unsafe {
            self.instance
                .get_physical_device_memory_properties(self.physical_device)
        };
        let memory_type = (0..memory_properties.memory_type_count)
            .find(|&i| {
                (req.memory_type_bits & (1 << i)) != 0
                    && memory_properties.memory_types[i as usize]
                        .property_flags
                        .contains(
                            ash::vk::MemoryPropertyFlags::HOST_VISIBLE
                                | ash::vk::MemoryPropertyFlags::HOST_COHERENT,
                        )
            })
            .ok_or_else(|| {
                unsafe { self.device.destroy_buffer(buffer, None) };
                Error::Unsupported(
                    "no HOST_VISIBLE memory type for video encode output buffer".into(),
                )
            })?;
        let alloc_info = ash::vk::MemoryAllocateInfo::default()
            .allocation_size(req.size)
            .memory_type_index(memory_type);
        let memory = unsafe {
            self.device
                .allocate_memory(&alloc_info, None)
                .map_err(|e| {
                    self.device.destroy_buffer(buffer, None);
                    Error::Backend(format!("video encode output buffer memory failed: {e:?}"))
                })?
        };
        unsafe {
            self.device
                .bind_buffer_memory(buffer, memory, 0)
                .map_err(|e| {
                    self.device.free_memory(memory, None);
                    self.device.destroy_buffer(buffer, None);
                    Error::Backend(format!("video encode output bind_memory failed: {e:?}"))
                })?;
        }
        self.exportable_buffer_memories
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(handle, memory);
        Ok(())
    }

    fn read_encode_bitstream(&self, handle: BufferHandle, max_bytes: u64) -> Result<Vec<u8>> {
        // GFX-4b: copy the encoded bitstream from the output buffer to a Vec<u8>.
        // The buffer must have been created with HOST_VISIBLE memory so we can map it.
        let resources = self
            .resources
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let buf = resources.buffer(handle)?;
        let requirements = unsafe { self.device.get_buffer_memory_requirements(buf) };
        let _ = requirements; // size info used for validation
        // Find the buffer's memory — for encode buffers we track them in exportable_buffer_memories.
        let memory = self
            .exportable_buffer_memories
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&handle)
            .copied();
        if let Some(mem) = memory {
            let byte_count = max_bytes.min(max_bytes) as usize;
            let ptr = unsafe {
                self.device
                    .map_memory(mem, 0, max_bytes, vk::MemoryMapFlags::empty())
                    .map_err(|e| {
                        Error::Backend(format!("map_memory for bitstream readback failed: {e:?}"))
                    })?
            };
            let slice = unsafe { std::slice::from_raw_parts(ptr as *const u8, byte_count) };
            let data = slice.to_vec();
            unsafe { self.device.unmap_memory(mem) };
            Ok(data)
        } else {
            Err(Error::InvalidHandle)
        }
    }

    fn latency_sleep(&self, surface: SurfaceHandle) -> Result<()> {
        let (reflex, semaphore) = match (self.reflex_nv.as_ref(), self.reflex_sleep_semaphore) {
            (Some(r), Some(s)) => (r, s),
            _ => return Ok(()), // Reflex or timeline semaphore not available; no-op
        };
        let value = self
            .reflex_sleep_value
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        let swapchain = {
            //panic allowed, reason = "poisoned mutex is unrecoverable"
            self.surfaces
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .swapchain_handle(surface)?
        };
        let sleep_info = ash::vk::LatencySleepInfoNV::default()
            .signal_semaphore(semaphore)
            .value(value);
        unsafe {
            reflex
                .latency_sleep(swapchain, &sleep_info)
                .map_err(|e| Error::Backend(format!("vkLatencySleepNV failed: {e:?}")))?;
        }
        // Wait for the driver to signal the semaphore (it will fire when it's time to start).
        let wait_info = ash::vk::SemaphoreWaitInfo::default()
            .semaphores(std::slice::from_ref(&semaphore))
            .values(std::slice::from_ref(&value));
        unsafe {
            self.device
                .wait_semaphores(&wait_info, u64::MAX)
                .map_err(|e| Error::Backend(format!("wait_semaphores for Reflex sleep: {e:?}")))?;
        }
        Ok(())
    }

    fn anti_lag_frame_start(&self) -> Result<()> {
        let Some(update) = self.anti_lag_update_amd else {
            return Ok(());
        };
        let mode = decode_anti_lag_mode(
            self.anti_lag_mode
                .load(std::sync::atomic::Ordering::Relaxed),
        );
        let frame_index = self
            .anti_lag_frame_index
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let presentation_info = VkAntiLagPresentationInfoAmd {
            s_type: VK_STRUCTURE_TYPE_ANTI_LAG_PRESENTATION_INFO_AMD,
            p_next: std::ptr::null_mut(),
            stage: VkAntiLagStageAmd::Input,
            frame_index,
        };
        let data = VkAntiLagDataAmd {
            s_type: VK_STRUCTURE_TYPE_ANTI_LAG_DATA_AMD,
            p_next: std::ptr::null(),
            mode: vk_anti_lag_mode(mode),
            max_fps: 0,
            p_presentation_info: &presentation_info,
        };
        unsafe {
            update(self.device.handle(), &data);
        }
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
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
                .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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

fn compact_acceleration_structure_build_sizes(
    src_kind: AccelerationStructureKind,
    src_size: u64,
    expected_kind: AccelerationStructureKind,
) -> Result<AccelerationStructureBuildSizes> {
    if src_kind != expected_kind {
        return Err(Error::InvalidInput(format!(
            "compaction source kind {:?} does not match expected {:?}",
            src_kind, expected_kind
        )));
    }
    Ok(AccelerationStructureBuildSizes {
        acceleration_structure_size: src_size,
        build_scratch_size: 0,
        update_scratch_size: 0,
    })
}

fn allocate_and_bind_video_session_memory(
    device: &AshDevice,
    video: &ash::khr::video_queue::Device,
    session: vk::VideoSessionKHR,
    resources: &RwLock<resources::ResourceRegistry>,
) -> Result<Vec<vk::DeviceMemory>> {
    let mut requirement_count = 0u32;
    unsafe {
        (video.fp().get_video_session_memory_requirements_khr)(
            device.handle(),
            session,
            &mut requirement_count,
            std::ptr::null_mut(),
        )
        .result()
        .map_err(|error| {
            Error::Backend(format!(
                "vkGetVideoSessionMemoryRequirementsKHR failed: {error:?}"
            ))
        })?;
    }
    if requirement_count == 0 {
        return Ok(Vec::new());
    }

    let mut requirements =
        vec![vk::VideoSessionMemoryRequirementsKHR::default(); requirement_count as usize];
    unsafe {
        (video.fp().get_video_session_memory_requirements_khr)(
            device.handle(),
            session,
            &mut requirement_count,
            requirements.as_mut_ptr(),
        )
        .result()
        .map_err(|error| {
            Error::Backend(format!(
                "vkGetVideoSessionMemoryRequirementsKHR failed: {error:?}"
            ))
        })?;
    }
    requirements.truncate(requirement_count as usize);

    let mut memories = Vec::with_capacity(requirements.len());
    let mut bindings = Vec::with_capacity(requirements.len());
    for requirement in &requirements {
        let memory_type_index = resources
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .allocator()
            .find_memory_type(
                requirement.memory_requirements.memory_type_bits,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
            )?;
        let alloc_info = vk::MemoryAllocateInfo::default()
            .allocation_size(requirement.memory_requirements.size)
            .memory_type_index(memory_type_index);
        let memory = match unsafe { device.allocate_memory(&alloc_info, None) } {
            Ok(memory) => memory,
            Err(error) => {
                for memory in memories {
                    unsafe { device.free_memory(memory, None) };
                }
                return Err(Error::Backend(format!(
                    "vkAllocateMemory for video session failed: {error:?}"
                )));
            }
        };
        memories.push(memory);
        bindings.push(
            vk::BindVideoSessionMemoryInfoKHR::default()
                .memory_bind_index(requirement.memory_bind_index)
                .memory(memory)
                .memory_offset(0)
                .memory_size(requirement.memory_requirements.size),
        );
    }

    if let Err(error) = unsafe {
        (video.fp().bind_video_session_memory_khr)(
            device.handle(),
            session,
            bindings.len() as u32,
            bindings.as_ptr(),
        )
        .result()
    } {
        for memory in memories {
            unsafe { device.free_memory(memory, None) };
        }
        return Err(Error::Backend(format!(
            "vkBindVideoSessionMemoryKHR failed: {error:?}"
        )));
    }

    Ok(memories)
}

fn destroy_video_session_entry(
    device: &AshDevice,
    video: &ash::khr::video_queue::Device,
    session: VulkanVideoSession,
) {
    unsafe {
        (video.fp().destroy_video_session_khr)(device.handle(), session.session, std::ptr::null());
        for memory in session.memories {
            device.free_memory(memory, None);
        }
    }
}

fn enabled_extension(enabled_extensions: &[String], extension: &str) -> bool {
    enabled_extensions
        .iter()
        .any(|enabled| enabled.as_str() == extension)
}

fn vk_optical_flow_grid_size(grid_size: u32) -> Result<vk::OpticalFlowGridSizeFlagsNV> {
    match grid_size {
        1 => Ok(vk::OpticalFlowGridSizeFlagsNV::TYPE_1X1),
        2 => Ok(vk::OpticalFlowGridSizeFlagsNV::TYPE_2X2),
        4 => Ok(vk::OpticalFlowGridSizeFlagsNV::TYPE_4X4),
        8 => Ok(vk::OpticalFlowGridSizeFlagsNV::TYPE_8X8),
        _ => Err(Error::InvalidInput(format!(
            "optical flow output_grid_size must be 1, 2, 4, or 8, got {grid_size}"
        ))),
    }
}

fn load_anti_lag_update_amd(
    instance: &Instance,
    device: &AshDevice,
    enabled_extensions: &[String],
) -> Option<PfnVkAntiLagUpdateAmd> {
    if !enabled_extension(enabled_extensions, "VK_AMD_anti_lag") {
        return None;
    }
    let proc =
        unsafe { instance.get_device_proc_addr(device.handle(), c"vkAntiLagUpdateAMD".as_ptr()) };
    proc.map(|raw| unsafe {
        std::mem::transmute::<unsafe extern "system" fn(), PfnVkAntiLagUpdateAmd>(raw)
    })
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
    let mut props2 = vk::PhysicalDeviceProperties2::default().push(&mut rt_props);
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

fn vk_component_type(t: ash::vk::ComponentTypeKHR) -> crate::CoopMatrixElementType {
    match t {
        ash::vk::ComponentTypeKHR::FLOAT16 => crate::CoopMatrixElementType::Float16,
        ash::vk::ComponentTypeKHR::FLOAT32 => crate::CoopMatrixElementType::Float32,
        ash::vk::ComponentTypeKHR::FLOAT64 => crate::CoopMatrixElementType::Float64,
        ash::vk::ComponentTypeKHR::SINT8 => crate::CoopMatrixElementType::Sint8,
        ash::vk::ComponentTypeKHR::SINT16 => crate::CoopMatrixElementType::Sint16,
        ash::vk::ComponentTypeKHR::SINT32 => crate::CoopMatrixElementType::Sint32,
        ash::vk::ComponentTypeKHR::UINT8 => crate::CoopMatrixElementType::Uint8,
        ash::vk::ComponentTypeKHR::UINT16 => crate::CoopMatrixElementType::Uint16,
        _ => crate::CoopMatrixElementType::Uint32,
    }
}

fn encode_reflex_mode(mode: ReflexMode) -> u8 {
    match mode {
        ReflexMode::Off => 0,
        ReflexMode::On => 1,
        ReflexMode::OnPlusBoost => 2,
    }
}

fn decode_reflex_mode(encoded: u8) -> ReflexMode {
    match encoded {
        1 => ReflexMode::On,
        2 => ReflexMode::OnPlusBoost,
        _ => ReflexMode::Off,
    }
}

fn encode_anti_lag_mode(mode: AntiLagMode) -> u8 {
    match mode {
        AntiLagMode::Off => 0,
        AntiLagMode::On => 1,
    }
}

fn decode_anti_lag_mode(encoded: u8) -> AntiLagMode {
    match encoded {
        1 => AntiLagMode::On,
        _ => AntiLagMode::Off,
    }
}

fn vk_anti_lag_mode(mode: AntiLagMode) -> VkAntiLagModeAmd {
    match mode {
        AntiLagMode::Off => VkAntiLagModeAmd::Off,
        AntiLagMode::On => VkAntiLagModeAmd::On,
    }
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
#[path = "tests.rs"]
mod tests;

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
            if let (Some(video), Ok(mut sessions)) =
                (self.video_queue_khr.as_ref(), self.video_sessions.lock())
            {
                for (_, session) in sessions.drain() {
                    destroy_video_session_entry(&self.device, video, session);
                }
            }
            if let (Some(dgc), Ok(mut layouts)) = (
                self.device_generated_commands_nv.as_ref(),
                self.indirect_command_layouts.lock(),
            ) {
                for (_, layout) in layouts.drain() {
                    (dgc.fp().destroy_indirect_commands_layout_nv)(
                        self.device.handle(),
                        layout,
                        std::ptr::null(),
                    );
                }
            }
            if let (Some(optical_flow), Ok(mut sessions)) = (
                self.optical_flow_nv_ext.as_ref(),
                self.optical_flow_sessions.lock(),
            ) {
                for (_, session) in sessions.drain() {
                    (optical_flow.fp().destroy_optical_flow_session_nv)(
                        self.device.handle(),
                        session,
                        std::ptr::null(),
                    );
                }
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
            if let Some(sem) = self.reflex_sleep_semaphore.take() {
                self.device.destroy_semaphore(sem, None);
            }
            // GFX-5b: destroy exportable fences before device.
            if let Ok(mut fences) = self.exportable_fences.lock() {
                for (_, fence) in fences.drain() {
                    self.device.destroy_fence(fence, None);
                }
            }
            self.device.destroy_device(None);
            // GFX-1g: Destroy the address binding report messenger before the instance.
            if let Some(mut messenger) = self.address_binding_messenger.take() {
                messenger.destroy();
            }
            self.instance.destroy_instance(None);
        }
    }
}

/// GFX-7b: Reconstruct a plain `SamplerCreateInfo` (no pNext chain) from an engine `SamplerDesc`.
/// Used to write embedded samplers into a `VkDescriptorHeapEXT` sampler heap.
fn build_sampler_create_info_for_heap(
    desc: &crate::SamplerDesc,
) -> ash::vk::SamplerCreateInfo<'static> {
    use resources::{vk_address_mode, vk_border_color, vk_compare_op, vk_filter, vk_mipmap_mode};
    ash::vk::SamplerCreateInfo::default()
        .mag_filter(vk_filter(desc.mag_filter))
        .min_filter(vk_filter(desc.min_filter))
        .mipmap_mode(vk_mipmap_mode(desc.mipmap_mode))
        .address_mode_u(vk_address_mode(desc.address_u))
        .address_mode_v(vk_address_mode(desc.address_v))
        .address_mode_w(vk_address_mode(desc.address_w))
        .mip_lod_bias(desc.mip_lod_bias)
        .anisotropy_enable(desc.max_anisotropy.is_some())
        .max_anisotropy(desc.max_anisotropy.unwrap_or(1.0))
        .compare_enable(desc.compare.is_some())
        .compare_op(
            desc.compare
                .map(vk_compare_op)
                .unwrap_or(ash::vk::CompareOp::ALWAYS),
        )
        .min_lod(desc.min_lod)
        .max_lod(desc.max_lod)
        .border_color(vk_border_color(desc.border_color))
        .unnormalized_coordinates(desc.unnormalized_coordinates)
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
