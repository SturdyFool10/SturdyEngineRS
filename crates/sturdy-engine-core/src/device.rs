use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[cfg(not(target_arch = "wasm32"))]
use crate::NativeSurfaceDesc;
#[cfg(not(target_arch = "wasm32"))]
use crate::backend::vulkan::{VulkanBackend, VulkanBackendConfig};
use crate::backend::{Backend, BackendKind, NullBackend, auto_backend_preference_order};
use crate::handles::HandleAllocator;
use crate::{
    BindGroupDesc, BindGroupHandle, BufferDesc, BufferHandle, CanonicalBinding, CanonicalGroupLayout,
    CanonicalPipelineLayout, Caps, ComputePipelineDesc, Error, FrameHandle, GraphicsPipelineDesc,
    ImageDesc, ImageHandle, PipelineHandle, PipelineLayoutHandle, RenderGraph, Result, ShaderDesc,
    ShaderHandle, ShaderReflection, ShaderTarget, SurfaceHandle, SurfaceSize,
};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DeviceDesc {
    pub backend: BackendKind,
    pub validation: bool,
}

impl Default for DeviceDesc {
    fn default() -> Self {
        Self {
            backend: BackendKind::Auto,
            validation: cfg!(debug_assertions),
        }
    }
}

#[derive(Clone)]
pub struct Device {
    inner: Arc<Mutex<DeviceInner>>,
}

struct DeviceInner {
    backend: Box<dyn Backend>,
    images: HashMap<ImageHandle, ImageDesc>,
    buffers: HashMap<BufferHandle, BufferDesc>,
    shaders: HashMap<ShaderHandle, ShaderDesc>,
    shader_reflections: HashMap<ShaderHandle, ShaderReflection>,
    pipeline_layouts: HashMap<PipelineLayoutHandle, CanonicalPipelineLayout>,
    pipelines: HashMap<PipelineHandle, PipelineDesc>,
    bind_groups: HashMap<BindGroupHandle, BindGroupDesc>,
    surfaces: HashMap<SurfaceHandle, SurfaceSize>,
    frames: HashMap<FrameHandle, RenderGraph>,
    image_handles: HandleAllocator,
    buffer_handles: HandleAllocator,
    shader_handles: HandleAllocator,
    pipeline_layout_handles: HandleAllocator,
    pipeline_handles: HandleAllocator,
    bind_group_handles: HandleAllocator,
    surface_handles: HandleAllocator,
    frame_handles: HandleAllocator,
}

impl Device {
    pub fn create(desc: DeviceDesc) -> Result<Self> {
        let backend = create_backend(desc.backend)?;

        Ok(Self {
            inner: Arc::new(Mutex::new(DeviceInner {
                backend,
                images: HashMap::new(),
                buffers: HashMap::new(),
                shaders: HashMap::new(),
                shader_reflections: HashMap::new(),
                pipeline_layouts: HashMap::new(),
                pipelines: HashMap::new(),
                bind_groups: HashMap::new(),
                surfaces: HashMap::new(),
                frames: HashMap::new(),
                image_handles: HandleAllocator::default(),
                buffer_handles: HandleAllocator::default(),
                shader_handles: HandleAllocator::default(),
                pipeline_layout_handles: HandleAllocator::default(),
                pipeline_handles: HandleAllocator::default(),
                bind_group_handles: HandleAllocator::default(),
                surface_handles: HandleAllocator::default(),
                frame_handles: HandleAllocator::default(),
            })),
        })
    }

    pub fn backend_kind(&self) -> BackendKind {
        self.inner
            .lock()
            .expect("device mutex poisoned")
            .backend
            .kind()
    }

    pub fn adapter_name(&self) -> Option<String> {
        self.inner
            .lock()
            .expect("device mutex poisoned")
            .backend
            .adapter_name()
    }

    pub fn caps(&self) -> Caps {
        self.inner
            .lock()
            .expect("device mutex poisoned")
            .backend
            .caps()
    }

    pub fn create_image(&self, desc: ImageDesc) -> Result<ImageHandle> {
        desc.validate()?;
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let handle = ImageHandle(inner.image_handles.alloc());
        inner.backend.create_image(handle, desc)?;
        inner.images.insert(handle, desc);
        Ok(handle)
    }

    pub fn destroy_image(&self, handle: ImageHandle) -> Result<()> {
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let _desc = inner.images.remove(&handle).ok_or(Error::InvalidHandle)?;
        inner.backend.destroy_image(handle)
    }

    pub fn image_desc(&self, handle: ImageHandle) -> Result<ImageDesc> {
        let inner = self.inner.lock().expect("device mutex poisoned");
        inner
            .images
            .get(&handle)
            .copied()
            .ok_or(Error::InvalidHandle)
    }

    pub fn create_buffer(&self, desc: BufferDesc) -> Result<BufferHandle> {
        desc.validate()?;
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let handle = BufferHandle(inner.buffer_handles.alloc());
        inner.backend.create_buffer(handle, desc)?;
        inner.buffers.insert(handle, desc);
        Ok(handle)
    }

    pub fn destroy_buffer(&self, handle: BufferHandle) -> Result<()> {
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let _desc = inner.buffers.remove(&handle).ok_or(Error::InvalidHandle)?;
        inner.backend.destroy_buffer(handle)
    }

    pub fn write_buffer(&self, handle: BufferHandle, offset: u64, data: &[u8]) -> Result<()> {
        let inner = self.inner.lock().expect("device mutex poisoned");
        let desc = inner.buffers.get(&handle).ok_or(Error::InvalidHandle)?;
        let end = offset
            .checked_add(data.len() as u64)
            .ok_or_else(|| Error::InvalidInput("buffer write range overflowed".into()))?;
        if end > desc.size {
            return Err(Error::InvalidInput(format!(
                "buffer write range [{offset}, {end}) exceeds buffer size {}",
                desc.size
            )));
        }
        inner.backend.write_buffer(handle, offset, data)
    }

    pub fn read_buffer(&self, handle: BufferHandle, offset: u64, out: &mut [u8]) -> Result<()> {
        let inner = self.inner.lock().expect("device mutex poisoned");
        let desc = inner.buffers.get(&handle).ok_or(Error::InvalidHandle)?;
        let end = offset
            .checked_add(out.len() as u64)
            .ok_or_else(|| Error::InvalidInput("buffer read range overflowed".into()))?;
        if end > desc.size {
            return Err(Error::InvalidInput(format!(
                "buffer read range [{offset}, {end}) exceeds buffer size {}",
                desc.size
            )));
        }
        inner.backend.read_buffer(handle, offset, out)
    }

    pub fn buffer_desc(&self, handle: BufferHandle) -> Result<BufferDesc> {
        let inner = self.inner.lock().expect("device mutex poisoned");
        inner
            .buffers
            .get(&handle)
            .copied()
            .ok_or(Error::InvalidHandle)
    }

    pub fn create_shader(&self, desc: ShaderDesc) -> Result<ShaderHandle> {
        desc.validate()?;
        let target = {
            let inner = self.inner.lock().expect("device mutex poisoned");
            inner.backend.preferred_shader_ir()
        };
        let (compiled_desc, reflection) = crate::slang::compile_and_reflect(&desc, target)?;
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let handle = ShaderHandle(inner.shader_handles.alloc());
        inner.backend.create_shader(handle, &compiled_desc)?;
        inner.shader_reflections.insert(handle, reflection);
        inner.shaders.insert(handle, compiled_desc);
        Ok(handle)
    }

    pub fn shader_reflection(&self, handle: ShaderHandle) -> Result<ShaderReflection> {
        let inner = self.inner.lock().expect("device mutex poisoned");
        inner
            .shader_reflections
            .get(&handle)
            .cloned()
            .ok_or(Error::InvalidHandle)
    }

    pub fn destroy_shader(&self, handle: ShaderHandle) -> Result<()> {
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let _desc = inner.shaders.remove(&handle).ok_or(Error::InvalidHandle)?;
        inner.shader_reflections.remove(&handle);
        inner.backend.destroy_shader(handle)
    }

    pub fn create_pipeline_layout(
        &self,
        layout: CanonicalPipelineLayout,
    ) -> Result<PipelineLayoutHandle> {
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let handle = PipelineLayoutHandle(inner.pipeline_layout_handles.alloc());
        inner.backend.create_pipeline_layout(handle, &layout)?;
        inner.pipeline_layouts.insert(handle, layout);
        Ok(handle)
    }

    pub fn destroy_pipeline_layout(&self, handle: PipelineLayoutHandle) -> Result<()> {
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let _layout = inner
            .pipeline_layouts
            .remove(&handle)
            .ok_or(Error::InvalidHandle)?;
        inner.backend.destroy_pipeline_layout(handle)
    }

    pub fn create_bind_group(&self, desc: BindGroupDesc) -> Result<BindGroupHandle> {
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let handle = BindGroupHandle(inner.bind_group_handles.alloc());
        inner.backend.create_bind_group(handle, &desc)?;
        inner.bind_groups.insert(handle, desc);
        Ok(handle)
    }

    pub fn destroy_bind_group(&self, handle: BindGroupHandle) -> Result<()> {
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let _desc = inner
            .bind_groups
            .remove(&handle)
            .ok_or(Error::InvalidHandle)?;
        inner.backend.destroy_bind_group(handle)
    }

    pub fn create_compute_pipeline(&self, desc: ComputePipelineDesc) -> Result<PipelineHandle> {
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        if !inner.shaders.contains_key(&desc.shader) {
            return Err(Error::InvalidHandle);
        }
        let (layout_handle, owned_layout) = match desc.layout {
            Some(h) => {
                if !inner.pipeline_layouts.contains_key(&h) {
                    return Err(Error::InvalidHandle);
                }
                (h, false)
            }
            None => {
                let reflection = inner
                    .shader_reflections
                    .get(&desc.shader)
                    .cloned()
                    .unwrap_or_default();
                let layout = reflection.layout;
                let lh = PipelineLayoutHandle(inner.pipeline_layout_handles.alloc());
                inner.backend.create_pipeline_layout(lh, &layout)?;
                inner.pipeline_layouts.insert(lh, layout);
                (lh, true)
            }
        };
        let resolved = ComputePipelineDesc {
            layout: Some(layout_handle),
            ..desc
        };
        let handle = PipelineHandle(inner.pipeline_handles.alloc());
        inner.backend.create_compute_pipeline(handle, resolved)?;
        inner.pipelines.insert(
            handle,
            PipelineDesc::Compute {
                desc: resolved,
                owned_layout: owned_layout.then_some(layout_handle),
            },
        );
        Ok(handle)
    }

    pub fn create_graphics_pipeline(&self, desc: GraphicsPipelineDesc) -> Result<PipelineHandle> {
        if desc.color_targets.is_empty() && desc.depth_format.is_none() {
            return Err(Error::InvalidInput(
                "graphics pipeline requires at least one color target or a depth target".into(),
            ));
        }
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        if !inner.shaders.contains_key(&desc.vertex_shader)
            || desc
                .fragment_shader
                .is_some_and(|shader| !inner.shaders.contains_key(&shader))
        {
            return Err(Error::InvalidHandle);
        }
        let (layout_handle, owned_layout) = match desc.layout {
            Some(h) => {
                if !inner.pipeline_layouts.contains_key(&h) {
                    return Err(Error::InvalidHandle);
                }
                (h, false)
            }
            None => {
                let layout =
                    merge_shader_reflections(&inner.shader_reflections, &desc);
                let lh = PipelineLayoutHandle(inner.pipeline_layout_handles.alloc());
                inner.backend.create_pipeline_layout(lh, &layout)?;
                inner.pipeline_layouts.insert(lh, layout);
                (lh, true)
            }
        };
        let resolved = GraphicsPipelineDesc {
            layout: Some(layout_handle),
            ..desc
        };
        let handle = PipelineHandle(inner.pipeline_handles.alloc());
        inner.backend.create_graphics_pipeline(handle, &resolved)?;
        inner.pipelines.insert(
            handle,
            PipelineDesc::Graphics {
                desc: resolved,
                owned_layout: owned_layout.then_some(layout_handle),
            },
        );
        Ok(handle)
    }

    pub fn destroy_pipeline(&self, handle: PipelineHandle) -> Result<()> {
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let desc = inner
            .pipelines
            .remove(&handle)
            .ok_or(Error::InvalidHandle)?;
        inner.backend.destroy_pipeline(handle)?;
        if let Some(lh) = desc.owned_layout() {
            inner.pipeline_layouts.remove(&lh);
            inner.backend.destroy_pipeline_layout(lh)?;
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn create_surface(&self, desc: NativeSurfaceDesc) -> Result<SurfaceHandle> {
        desc.validate()?;
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let handle = SurfaceHandle(inner.surface_handles.alloc());
        inner.backend.create_surface(handle, desc)?;
        inner.surfaces.insert(handle, desc.size);
        Ok(handle)
    }

    pub fn resize_surface(&self, handle: SurfaceHandle, size: SurfaceSize) -> Result<()> {
        size.validate()?;
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        if !inner.surfaces.contains_key(&handle) {
            return Err(Error::InvalidHandle);
        }
        inner.backend.resize_surface(handle, size)?;
        inner.surfaces.insert(handle, size);
        Ok(())
    }

    pub fn acquire_surface_image(&self, surface: SurfaceHandle) -> Result<ImageHandle> {
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        if !inner.surfaces.contains_key(&surface) {
            return Err(Error::InvalidHandle);
        }
        let handle = ImageHandle(inner.image_handles.alloc());
        let desc = inner.backend.acquire_surface_image(surface, handle)?;
        inner.images.insert(handle, desc);
        Ok(handle)
    }

    pub fn present_surface(&self, surface: SurfaceHandle) -> Result<()> {
        let inner = self.inner.lock().expect("device mutex poisoned");
        if !inner.surfaces.contains_key(&surface) {
            return Err(Error::InvalidHandle);
        }
        inner.backend.present_surface(surface)
    }

    pub fn destroy_surface(&self, handle: SurfaceHandle) -> Result<()> {
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let _size = inner.surfaces.remove(&handle).ok_or(Error::InvalidHandle)?;
        inner.backend.destroy_surface(handle)
    }

    pub fn begin_frame(&self) -> Result<Frame> {
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let handle = FrameHandle(inner.frame_handles.alloc());
        inner.frames.insert(handle, RenderGraph::new());
        Ok(Frame {
            device: self.clone(),
            handle,
            submitted: false,
        })
    }

    pub fn wait_idle(&self) -> Result<()> {
        self.inner
            .lock()
            .expect("device mutex poisoned")
            .backend
            .wait_idle()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PipelineDesc {
    Compute {
        desc: ComputePipelineDesc,
        owned_layout: Option<PipelineLayoutHandle>,
    },
    Graphics {
        desc: GraphicsPipelineDesc,
        owned_layout: Option<PipelineLayoutHandle>,
    },
}

impl PipelineDesc {
    fn owned_layout(&self) -> Option<PipelineLayoutHandle> {
        match self {
            PipelineDesc::Compute { owned_layout, .. } => *owned_layout,
            PipelineDesc::Graphics { owned_layout, .. } => *owned_layout,
        }
    }
}

fn merge_shader_reflections(
    reflections: &HashMap<ShaderHandle, ShaderReflection>,
    desc: &GraphicsPipelineDesc,
) -> CanonicalPipelineLayout {
    use std::collections::BTreeMap;
    use crate::CanonicalBinding;

    let mut groups: BTreeMap<usize, (String, Vec<(String, CanonicalBinding)>)> = BTreeMap::new();

    let shaders: Vec<ShaderHandle> = [Some(desc.vertex_shader), desc.fragment_shader]
        .into_iter()
        .flatten()
        .collect();

    for shader in shaders {
        let Some(reflection) = reflections.get(&shader) else {
            continue;
        };
        for (group_idx, group) in reflection.layout.groups.iter().enumerate() {
            let entry = groups
                .entry(group_idx)
                .or_insert_with(|| (group.name.clone(), Vec::new()));
            for binding in &group.bindings {
                if let Some(existing) = entry.1.iter_mut().find(|(p, _)| p == &binding.path) {
                    existing.1.stage_mask |= binding.stage_mask;
                } else {
                    entry.1.push((binding.path.clone(), binding.clone()));
                }
            }
        }
    }

    CanonicalPipelineLayout {
        groups: groups
            .into_values()
            .map(|(name, bindings)| CanonicalGroupLayout {
                name,
                bindings: bindings.into_iter().map(|(_, b)| b).collect(),
            })
            .collect(),
        push_constants_bytes: 0,
    }
}

fn create_backend(kind: BackendKind) -> Result<Box<dyn Backend>> {
    match kind {
        BackendKind::Auto => {
            let preferred = auto_backend_preference_order();
            let mut last_error = None;
            for backend in preferred {
                match create_backend(backend) {
                    Ok(backend) => return Ok(backend),
                    Err(error) => last_error = Some(error),
                }
            }
            Err(last_error.unwrap_or(Error::Unsupported("no backend is available on this target")))
        }
        BackendKind::Null => Ok(Box::new(NullBackend::new())),
        BackendKind::Vulkan => create_vulkan_backend(),
        BackendKind::D3d12 => create_available_backend(BackendKind::D3d12, "D3D12"),
        BackendKind::Metal => create_available_backend(BackendKind::Metal, "Metal"),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn create_vulkan_backend() -> Result<Box<dyn Backend>> {
    if !BackendKind::Vulkan.is_available_on_target() {
        return Err(Error::Unsupported("Vulkan is not available on this target"));
    }
    Ok(Box::new(VulkanBackend::create(VulkanBackendConfig {
        validation: cfg!(debug_assertions),
    })?))
}

#[cfg(target_arch = "wasm32")]
fn create_vulkan_backend() -> Result<Box<dyn Backend>> {
    Err(Error::Unsupported("Vulkan is not available on this target"))
}

fn create_available_backend(kind: BackendKind, name: &'static str) -> Result<Box<dyn Backend>> {
    if !kind.is_available_on_target() {
        return Err(Error::Unsupported(match kind {
            BackendKind::Vulkan => "Vulkan is not available on this target",
            BackendKind::D3d12 => "D3D12 is not available on this target",
            BackendKind::Metal => "Metal is not available on this target",
            BackendKind::Auto | BackendKind::Null => "backend is not available on this target",
        }));
    }

    let _name = name;
    Ok(Box::new(NullBackend::for_kind(kind)))
}

#[derive(Clone)]
pub struct Frame {
    device: Device,
    handle: FrameHandle,
    submitted: bool,
}

impl Frame {
    pub fn handle(&self) -> FrameHandle {
        self.handle
    }

    pub fn graph_mut<R>(&mut self, f: impl FnOnce(&mut RenderGraph) -> Result<R>) -> Result<R> {
        let mut inner = self.device.inner.lock().expect("device mutex poisoned");
        let graph = inner
            .frames
            .get_mut(&self.handle)
            .ok_or(Error::InvalidHandle)?;
        f(graph)
    }

    pub fn flush(&mut self) -> Result<()> {
        let compiled = {
            let inner = self.device.inner.lock().expect("device mutex poisoned");
            let graph = inner.frames.get(&self.handle).ok_or(Error::InvalidHandle)?;
            graph.compile()?
        };

        self.device
            .inner
            .lock()
            .expect("device mutex poisoned")
            .backend
            .flush(&compiled)?;
        self.submitted = true;
        Ok(())
    }

    pub fn present(&mut self) -> Result<()> {
        if !self.submitted {
            self.flush()?;
        }
        self.device
            .inner
            .lock()
            .expect("device mutex poisoned")
            .backend
            .present()
    }

    pub fn wait(&self) -> Result<()> {
        self.device.wait_idle()
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.device.inner.lock() {
            inner.frames.remove(&self.handle);
        }
    }
}
