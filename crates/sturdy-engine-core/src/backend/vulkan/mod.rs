mod allocator;
mod caps;
mod commands;
mod config;
mod descriptors;
mod device;
mod instance;
mod pipelines;
mod resources;
mod shaders;
mod surfaces;

use ash::{vk, Device as AshDevice, Entry, Instance};
use std::sync::{Mutex, RwLock};
use std::{fs, path::PathBuf};

use crate::backend::{Backend, BackendKind};
use crate::{
    BindGroupDesc, BindGroupHandle, BufferDesc, BufferHandle, CanonicalPipelineLayout, Caps,
    CompiledGraph, ComputePipelineDesc, Error, GraphicsPipelineDesc, ImageDesc, ImageHandle,
    NativeSurfaceDesc, PipelineHandle, PipelineLayoutHandle, Result, SamplerDesc, SamplerHandle,
    ShaderDesc, ShaderHandle, SubmissionHandle, SurfaceHandle, SurfaceInfo, SurfaceSize,
};

pub use config::VulkanBackendConfig;
use device::{create_logical_device, DeviceSelection};
use instance::{create_instance, load_entry};

pub const KIND: BackendKind = BackendKind::Vulkan;

pub struct VulkanBackend {
    _entry: Entry,
    instance: Instance,
    physical_device: vk::PhysicalDevice,
    device: AshDevice,
    graphics_queue_family: u32,
    graphics_queue: vk::Queue,
    caps: Caps,
    commands: Mutex<commands::CommandContext>,
    descriptors: RwLock<descriptors::DescriptorRegistry>,
    pipelines: Mutex<pipelines::PipelineRegistry>,
    resources: RwLock<resources::ResourceRegistry>,
    shaders: Mutex<shaders::ShaderRegistry>,
    surfaces: Mutex<surfaces::SurfaceRegistry>,
    /// Surface whose image was most recently acquired; cleared after present.
    active_surface: Mutex<Option<SurfaceHandle>>,
}

impl VulkanBackend {
    pub fn create(config: VulkanBackendConfig) -> Result<Self> {
        let entry = load_entry()?;
        let instance = create_instance(&entry, &config)?;
        let selection = DeviceSelection::pick(&instance)?;
        let logical = create_logical_device(&instance, &selection)?;
        let caps = caps::query_caps(&instance, selection.physical_device);
        let memory_properties =
            unsafe { instance.get_physical_device_memory_properties(selection.physical_device) };
        let resource_registry = resources::ResourceRegistry::new(memory_properties);
        let commands =
            commands::CommandContext::create(&logical.device, selection.graphics_queue_family)?;
        let cache_data = load_pipeline_cache_file();
        let pipeline_registry =
            pipelines::PipelineRegistry::create(&logical.device, cache_data.as_deref())?;

        Ok(Self {
            _entry: entry,
            instance,
            physical_device: selection.physical_device,
            device: logical.device,
            graphics_queue_family: selection.graphics_queue_family,
            graphics_queue: logical.graphics_queue,
            caps,
            commands: Mutex::new(commands),
            descriptors: RwLock::new(descriptors::DescriptorRegistry::default()),
            pipelines: Mutex::new(pipeline_registry),
            resources: RwLock::new(resource_registry),
            shaders: Mutex::new(shaders::ShaderRegistry::default()),
            surfaces: Mutex::new(surfaces::SurfaceRegistry::default()),
            active_surface: Mutex::new(None),
        })
    }

    pub fn physical_device_name(&self) -> String {
        device::physical_device_name(&self.instance, self.physical_device)
    }

    pub fn graphics_queue_family(&self) -> u32 {
        self.graphics_queue_family
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
        self.caps
    }

    fn create_image(&self, handle: ImageHandle, desc: ImageDesc) -> Result<()> {
        self.resources
            .write()
            .expect("vulkan resource registry rwlock poisoned")
            .create_image(&self.device, handle, desc)
    }

    fn destroy_image(&self, handle: ImageHandle) -> Result<()> {
        let mut resources = self
            .resources
            .write()
            .expect("vulkan resource registry rwlock poisoned");
        let view = resources.image_view(handle)?;
        self.pipelines
            .lock()
            .expect("vulkan pipeline registry mutex poisoned")
            .invalidate_framebuffers_for_view(&self.device, view);
        resources.destroy_image(&self.device, handle)
    }

    fn create_buffer(&self, handle: BufferHandle, desc: BufferDesc) -> Result<()> {
        self.resources
            .write()
            .expect("vulkan resource registry rwlock poisoned")
            .create_buffer(&self.device, handle, desc)
    }

    fn destroy_buffer(&self, handle: BufferHandle) -> Result<()> {
        self.resources
            .write()
            .expect("vulkan resource registry rwlock poisoned")
            .destroy_buffer(&self.device, handle)
    }

    fn create_sampler(&self, handle: SamplerHandle, desc: SamplerDesc) -> Result<()> {
        self.resources
            .write()
            .expect("vulkan resource registry rwlock poisoned")
            .create_sampler(&self.device, handle, desc)
    }

    fn destroy_sampler(&self, handle: SamplerHandle) -> Result<()> {
        self.resources
            .write()
            .expect("vulkan resource registry rwlock poisoned")
            .destroy_sampler(&self.device, handle)
    }

    fn write_buffer(&self, handle: BufferHandle, offset: u64, data: &[u8]) -> Result<()> {
        self.resources
            .write()
            .expect("vulkan resource registry rwlock poisoned")
            .write_buffer(handle, offset, data)
    }

    fn read_buffer(&self, handle: BufferHandle, offset: u64, out: &mut [u8]) -> Result<()> {
        self.resources
            .read()
            .expect("vulkan resource registry rwlock poisoned")
            .read_buffer(handle, offset, out)
    }

    fn create_shader(&self, handle: ShaderHandle, desc: &ShaderDesc) -> Result<()> {
        self.shaders
            .lock()
            .expect("vulkan shader registry mutex poisoned")
            .create_shader(&self.device, handle, desc)
    }

    fn destroy_shader(&self, handle: ShaderHandle) -> Result<()> {
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
        self.descriptors
            .write()
            .expect("vulkan descriptor registry rwlock poisoned")
            .create_pipeline_layout(&self.device, handle, layout)
    }

    fn destroy_pipeline_layout(&self, handle: PipelineLayoutHandle) -> Result<()> {
        self.descriptors
            .write()
            .expect("vulkan descriptor registry rwlock poisoned")
            .destroy_pipeline_layout(&self.device, handle)
    }

    fn create_bind_group(&self, handle: BindGroupHandle, desc: &BindGroupDesc) -> Result<()> {
        let resources = self
            .resources
            .read()
            .expect("vulkan resource registry rwlock poisoned");
        self.descriptors
            .write()
            .expect("vulkan descriptor registry rwlock poisoned")
            .create_bind_group(&self.device, handle, desc, &resources)
    }

    fn destroy_bind_group(&self, handle: BindGroupHandle) -> Result<()> {
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
        let shaders = self
            .shaders
            .lock()
            .expect("vulkan shader registry mutex poisoned");
        let descriptors = self
            .descriptors
            .read()
            .expect("vulkan descriptor registry rwlock poisoned");
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
        let shaders = self
            .shaders
            .lock()
            .expect("vulkan shader registry mutex poisoned");
        let descriptors = self
            .descriptors
            .read()
            .expect("vulkan descriptor registry rwlock poisoned");
        self.pipelines
            .lock()
            .expect("vulkan pipeline registry mutex poisoned")
            .create_graphics_pipeline(&self.device, handle, desc, &shaders, &descriptors)
    }

    fn destroy_pipeline(&self, handle: PipelineHandle) -> Result<()> {
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
        self.surfaces
            .lock()
            .expect("vulkan surface registry mutex poisoned")
            .create_surface(
                &self._entry,
                &self.instance,
                &self.device,
                self.physical_device,
                self.graphics_queue_family,
                handle,
                desc,
            )
    }

    fn resize_surface(&self, handle: SurfaceHandle, size: SurfaceSize) -> Result<SurfaceInfo> {
        unsafe {
            self.device
                .device_wait_idle()
                .map_err(|error| Error::Backend(format!("vkDeviceWaitIdle failed: {error:?}")))?;
        }
        self.pipelines
            .lock()
            .expect("vulkan pipeline registry mutex poisoned")
            .clear_all_framebuffers(&self.device);
        self.surfaces
            .lock()
            .expect("vulkan surface registry mutex poisoned")
            .resize_surface(&self.device, self.physical_device, handle, size)
    }

    fn destroy_surface(&self, handle: SurfaceHandle) -> Result<()> {
        unsafe {
            self.device
                .device_wait_idle()
                .map_err(|error| Error::Backend(format!("vkDeviceWaitIdle failed: {error:?}")))?;
        }
        *self
            .active_surface
            .lock()
            .expect("vulkan active surface mutex poisoned") = None;
        self.pipelines
            .lock()
            .expect("vulkan pipeline registry mutex poisoned")
            .clear_all_framebuffers(&self.device);
        self.surfaces
            .lock()
            .expect("vulkan surface registry mutex poisoned")
            .destroy_surface(&self.device, handle)?;
        Ok(())
    }

    fn acquire_surface_image(
        &self,
        surface: SurfaceHandle,
        image: ImageHandle,
    ) -> Result<ImageDesc> {
        let acquired = self
            .surfaces
            .lock()
            .expect("vulkan surface registry mutex poisoned")
            .acquire_image(surface)?;
        self.resources
            .write()
            .expect("vulkan resource registry rwlock poisoned")
            .import_image(image, acquired.image, acquired.image_view, acquired.desc)?;
        *self
            .active_surface
            .lock()
            .expect("vulkan active surface mutex poisoned") = Some(surface);
        Ok(acquired.desc)
    }

    fn present_surface(&self, surface: SurfaceHandle) -> Result<()> {
        let result = self
            .surfaces
            .lock()
            .expect("vulkan surface registry mutex poisoned")
            .present(self.graphics_queue, surface);
        *self
            .active_surface
            .lock()
            .expect("vulkan active surface mutex poisoned") = None;
        result
    }

    fn flush(&self, graph: &CompiledGraph) -> Result<SubmissionHandle> {
        // Resolve per-surface semaphores if a swapchain image was acquired.
        let (wait_sem, signal_sem) = {
            let active = *self
                .active_surface
                .lock()
                .expect("vulkan active surface mutex poisoned");
            if let Some(sh) = active {
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

        let resources = self
            .resources
            .read()
            .expect("vulkan resource registry rwlock poisoned");
        let descriptors = self
            .descriptors
            .read()
            .expect("vulkan descriptor registry rwlock poisoned");
        let mut pipelines = self
            .pipelines
            .lock()
            .expect("vulkan pipeline registry mutex poisoned");
        let mut commands = self
            .commands
            .lock()
            .expect("vulkan command context mutex poisoned");
        commands.submit_graph(
            &self.device,
            self.graphics_queue,
            graph,
            &resources,
            &descriptors,
            &mut pipelines,
            wait_sem,
            signal_sem,
        )
    }

    fn wait_submission(&self, token: SubmissionHandle) -> Result<()> {
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
