mod adapter;
mod alias_heaps;
mod allocator;
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
    AdapterInfo, BindGroupDesc, BindGroupHandle, BufferDesc, BufferHandle, CanonicalPipelineLayout,
    Caps, CompiledGraph, ComputePipelineDesc, Error, ExternalBufferDesc, ExternalBufferHandle,
    ExternalImageDesc, ExternalImageHandle, Format, FormatCapabilities, GraphicsPipelineDesc,
    ImageDesc, ImageHandle, NativeSurfaceDesc, PipelineHandle, PipelineLayoutHandle, Result,
    SamplerDesc, SamplerHandle, ShaderDesc, ShaderHandle, SubmissionHandle, SurfaceCapabilities,
    SurfaceHandle, SurfaceInfo, SurfaceRecreateDesc, SurfaceSize,
};

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
        let caps = caps::query_caps(&instance, selection.physical_device);
        let memory_properties =
            unsafe { instance.get_physical_device_memory_properties(selection.physical_device) };
        let resource_registry = resources::ResourceRegistry::new(memory_properties);
        let commands = commands::FramedCommands::create(&logical.device, logical.queue_families)?;
        let cache_data = load_pipeline_cache_file();
        let pipeline_registry =
            pipelines::PipelineRegistry::create(&logical.device, cache_data.as_deref())?;

        let debug_utils = debug::DebugUtils::new(&instance, &logical.device);
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
        })
    }

    pub fn physical_device_name(&self) -> String {
        device::physical_device_name(&self.instance, self.physical_device)
    }

    pub fn graphics_queue_family(&self) -> u32 {
        self.queue_families.graphics
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

    fn create_sampler(&self, handle: SamplerHandle, desc: SamplerDesc) -> Result<()> {
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
            .create_pipeline_layout(&self.device, handle, layout)
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
        let resources = self
            .resources
            .read()
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
        let handle = commands.submit(
            &self.device,
            self.queues,
            self.queue_families,
            graph,
            &resources,
            &descriptors,
            &mut pipelines,
            &self.debug,
            wait_sem,
            signal_sem,
        )?;

        // Incrementally save the pipeline cache after enough new pipelines have
        // been compiled, so data is not lost if the process is killed before shutdown.
        let checkpoint = pipelines.maybe_checkpoint(&self.device);
        drop(pipelines); // release lock before disk I/O
        if let Some(data) = checkpoint {
            save_pipeline_cache_file(&data);
        }

        Ok(handle)
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
                resources.destroy_all(&self.device);
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
