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

use ash::{Device as AshDevice, Entry, Instance, vk};
use std::sync::Mutex;
use std::{fs, path::PathBuf};

use crate::backend::{Backend, BackendKind};
use crate::{
    BindGroupDesc, BindGroupHandle, BufferDesc, BufferHandle, CanonicalPipelineLayout, Caps,
    CompiledGraph, ComputePipelineDesc, Error, GraphicsPipelineDesc, ImageDesc, ImageHandle,
    NativeSurfaceDesc, PipelineHandle, PipelineLayoutHandle, Result, ShaderDesc, ShaderHandle,
    SurfaceHandle, SurfaceSize,
};

pub use config::VulkanBackendConfig;
use device::{DeviceSelection, create_logical_device};
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
    descriptors: Mutex<descriptors::DescriptorRegistry>,
    pipelines: Mutex<pipelines::PipelineRegistry>,
    resources: Mutex<resources::ResourceRegistry>,
    shaders: Mutex<shaders::ShaderRegistry>,
    surfaces: Mutex<surfaces::SurfaceRegistry>,
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
        let pipeline_registry = pipelines::PipelineRegistry::create(
            &logical.device,
            cache_data.as_deref(),
        )?;

        Ok(Self {
            _entry: entry,
            instance,
            physical_device: selection.physical_device,
            device: logical.device,
            graphics_queue_family: selection.graphics_queue_family,
            graphics_queue: logical.graphics_queue,
            caps,
            commands: Mutex::new(commands),
            descriptors: Mutex::new(descriptors::DescriptorRegistry::default()),
            pipelines: Mutex::new(pipeline_registry),
            resources: Mutex::new(resource_registry),
            shaders: Mutex::new(shaders::ShaderRegistry::default()),
            surfaces: Mutex::new(surfaces::SurfaceRegistry::default()),
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
            .lock()
            .expect("vulkan resource registry mutex poisoned")
            .create_image(&self.device, handle, desc)
    }

    fn destroy_image(&self, handle: ImageHandle) -> Result<()> {
        let view = self
            .resources
            .lock()
            .expect("vulkan resource registry mutex poisoned")
            .image_view(handle)?;
        self.pipelines
            .lock()
            .expect("vulkan pipeline registry mutex poisoned")
            .invalidate_framebuffers_for_view(&self.device, view);
        self.resources
            .lock()
            .expect("vulkan resource registry mutex poisoned")
            .destroy_image(&self.device, handle)
    }

    fn create_buffer(&self, handle: BufferHandle, desc: BufferDesc) -> Result<()> {
        self.resources
            .lock()
            .expect("vulkan resource registry mutex poisoned")
            .create_buffer(&self.device, handle, desc)
    }

    fn destroy_buffer(&self, handle: BufferHandle) -> Result<()> {
        self.resources
            .lock()
            .expect("vulkan resource registry mutex poisoned")
            .destroy_buffer(&self.device, handle)
    }

    fn write_buffer(&self, handle: BufferHandle, offset: u64, data: &[u8]) -> Result<()> {
        self.resources
            .lock()
            .expect("vulkan resource registry mutex poisoned")
            .write_buffer(handle, offset, data)
    }

    fn read_buffer(&self, handle: BufferHandle, offset: u64, out: &mut [u8]) -> Result<()> {
        self.resources
            .lock()
            .expect("vulkan resource registry mutex poisoned")
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
            .lock()
            .expect("vulkan descriptor registry mutex poisoned")
            .create_pipeline_layout(&self.device, handle, layout)
    }

    fn destroy_pipeline_layout(&self, handle: PipelineLayoutHandle) -> Result<()> {
        self.descriptors
            .lock()
            .expect("vulkan descriptor registry mutex poisoned")
            .destroy_pipeline_layout(&self.device, handle)
    }

    fn create_bind_group(&self, handle: BindGroupHandle, desc: &BindGroupDesc) -> Result<()> {
        let resources = self
            .resources
            .lock()
            .expect("vulkan resource registry mutex poisoned");
        self.descriptors
            .lock()
            .expect("vulkan descriptor registry mutex poisoned")
            .create_bind_group(&self.device, handle, desc, &resources)
    }

    fn destroy_bind_group(&self, handle: BindGroupHandle) -> Result<()> {
        self.descriptors
            .lock()
            .expect("vulkan descriptor registry mutex poisoned")
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
            .lock()
            .expect("vulkan descriptor registry mutex poisoned");
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
            .lock()
            .expect("vulkan descriptor registry mutex poisoned");
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

    fn create_surface(&self, handle: SurfaceHandle, desc: NativeSurfaceDesc) -> Result<()> {
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

    fn resize_surface(&self, handle: SurfaceHandle, size: SurfaceSize) -> Result<()> {
        self.surfaces
            .lock()
            .expect("vulkan surface registry mutex poisoned")
            .resize_surface(&self.device, self.physical_device, handle, size)?;
        self.pipelines
            .lock()
            .expect("vulkan pipeline registry mutex poisoned")
            .clear_all_framebuffers(&self.device);
        Ok(())
    }

    fn destroy_surface(&self, handle: SurfaceHandle) -> Result<()> {
        self.surfaces
            .lock()
            .expect("vulkan surface registry mutex poisoned")
            .destroy_surface(&self.device, handle)?;
        self.pipelines
            .lock()
            .expect("vulkan pipeline registry mutex poisoned")
            .clear_all_framebuffers(&self.device);
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
            .acquire_image(&self.device, surface)?;
        self.resources
            .lock()
            .expect("vulkan resource registry mutex poisoned")
            .import_image(image, acquired.image, acquired.image_view, acquired.desc)?;
        Ok(acquired.desc)
    }

    fn present_surface(&self, surface: SurfaceHandle) -> Result<()> {
        self.surfaces
            .lock()
            .expect("vulkan surface registry mutex poisoned")
            .present(self.graphics_queue, surface)
    }

    fn flush(&self, graph: &CompiledGraph) -> Result<()> {
        let resources = self
            .resources
            .lock()
            .expect("vulkan resource registry mutex poisoned");
        let descriptors = self
            .descriptors
            .lock()
            .expect("vulkan descriptor registry mutex poisoned");
        let mut pipelines = self
            .pipelines
            .lock()
            .expect("vulkan pipeline registry mutex poisoned");
        let mut commands = self
            .commands
            .lock()
            .expect("vulkan command context mutex poisoned");
        commands.record_submit_and_wait(
            &self.device,
            self.graphics_queue,
            graph,
            &resources,
            &descriptors,
            &mut pipelines,
        )
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
            if let Ok(mut descriptors) = self.descriptors.lock() {
                descriptors.destroy_all(&self.device);
            }
            if let Ok(mut shaders) = self.shaders.lock() {
                shaders.destroy_all(&self.device);
            }
            if let Ok(mut resources) = self.resources.lock() {
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
