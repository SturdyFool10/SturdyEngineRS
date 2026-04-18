use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

#[cfg(not(target_arch = "wasm32"))]
use crate::NativeSurfaceDesc;
use crate::backend::factory::create_backend;
use crate::backend::{Backend, BackendKind, factory};
use crate::handles::HandleAllocator;
use crate::{
    AdapterInfo, AdapterSelection, BackendRawCapabilities, BindGroupDesc, BindGroupHandle,
    BindingKind, BufferDesc, BufferHandle, BufferStateKey, CanonicalGroupLayout,
    CanonicalPipelineLayout, Caps, ComputePipelineDesc, Error, ExternalBufferDesc,
    ExternalImageDesc, Format, FormatCapabilities, FrameHandle, GpuCaptureDesc, GpuCaptureTool,
    GraphicsPipelineDesc, ImageDesc, ImageHandle, ImageStateKey, NativeHandleCapabilities,
    PipelineHandle, PipelineLayoutHandle, RenderGraph, ResourceBinding, Result, RgState,
    SamplerDesc, SamplerHandle, ShaderDesc, ShaderHandle, ShaderReflection, StageMask,
    SubmissionHandle, SurfaceCapabilities, SurfaceEvent, SurfaceHandle, SurfaceHdrCaps,
    SurfaceInfo, SurfaceRecreateDesc, SurfaceSize,
};

#[derive(Clone, Debug)]
pub struct DeviceDesc {
    pub backend: BackendKind,
    pub validation: bool,
    pub adapter: AdapterSelection,
    pub required_features: Vec<String>,
    pub optional_features: Vec<String>,
    pub disabled_features: Vec<String>,
    pub required_extensions: Vec<String>,
    pub optional_extensions: Vec<String>,
    pub disabled_extensions: Vec<String>,
}

impl Default for DeviceDesc {
    fn default() -> Self {
        Self {
            backend: BackendKind::Auto,
            validation: cfg!(debug_assertions),
            adapter: AdapterSelection::Auto,
            required_features: Vec::new(),
            optional_features: Vec::new(),
            disabled_features: Vec::new(),
            required_extensions: Vec::new(),
            optional_extensions: Vec::new(),
            disabled_extensions: Vec::new(),
        }
    }
}

impl DeviceDesc {
    pub fn require_backend_feature(mut self, name: impl Into<String>) -> Self {
        self.required_features.push(name.into());
        self
    }

    pub fn prefer_backend_feature(mut self, name: impl Into<String>) -> Self {
        self.optional_features.push(name.into());
        self
    }

    pub fn disable_backend_feature(mut self, name: impl Into<String>) -> Self {
        self.disabled_features.push(name.into());
        self
    }

    pub fn require_backend_extension(mut self, name: impl Into<String>) -> Self {
        self.required_extensions.push(name.into());
        self
    }

    pub fn prefer_backend_extension(mut self, name: impl Into<String>) -> Self {
        self.optional_extensions.push(name.into());
        self
    }

    pub fn disable_backend_extension(mut self, name: impl Into<String>) -> Self {
        self.disabled_extensions.push(name.into());
        self
    }
}

pub fn enumerate_adapters(backend: BackendKind) -> Result<Vec<AdapterInfo>> {
    factory::enumerate_adapters(backend)
}

#[derive(Clone)]
pub struct Device {
    inner: Arc<Mutex<DeviceInner>>,
}

/// GPU resource to destroy after the in-flight frame's fence is signaled.
enum DeferredDestroy {
    Image(ImageHandle),
    Buffer(BufferHandle),
    Sampler(SamplerHandle),
    Shader(ShaderHandle),
    Pipeline(PipelineHandle),
    PipelineLayout(PipelineLayoutHandle),
    BindGroup(BindGroupHandle),
}

struct DeviceInner {
    backend: Box<dyn Backend>,
    images: HashMap<ImageHandle, ImageDesc>,
    buffers: HashMap<BufferHandle, BufferDesc>,
    image_states: HashMap<ImageStateKey, RgState>,
    buffer_states: HashMap<BufferStateKey, RgState>,
    samplers: HashMap<SamplerHandle, SamplerDesc>,
    shaders: HashMap<ShaderHandle, ShaderDesc>,
    shader_reflections: HashMap<ShaderHandle, ShaderReflection>,
    pipeline_layouts: HashMap<PipelineLayoutHandle, CanonicalPipelineLayout>,
    pipelines: HashMap<PipelineHandle, PipelineDesc>,
    bind_groups: HashMap<BindGroupHandle, BindGroupDesc>,
    surfaces: HashMap<SurfaceHandle, SurfaceState>,
    frames: HashMap<FrameHandle, RenderGraph>,
    /// Resources queued for deferred destruction.  Drained at the start of
    /// every `Frame::flush` after the previous frame's fence is waited —
    /// guaranteeing the GPU is no longer accessing them.
    deferred_destroys: Vec<DeferredDestroy>,
    /// Transient images queued for destruction after the in-flight frame's
    /// fence is signaled.  Populated by `Frame::flush` and drained at the
    /// next `Frame::flush` after the fence is waited.
    pending_transient_destroys: Vec<ImageHandle>,
    image_handles: HandleAllocator,
    buffer_handles: HandleAllocator,
    sampler_handles: HandleAllocator,
    shader_handles: HandleAllocator,
    pipeline_layout_handles: HandleAllocator,
    pipeline_handles: HandleAllocator,
    bind_group_handles: HandleAllocator,
    surface_handles: HandleAllocator,
    frame_handles: HandleAllocator,
}

struct SurfaceState {
    info: SurfaceInfo,
    events: Vec<SurfaceEvent>,
}

impl Device {
    pub fn create(desc: DeviceDesc) -> Result<Self> {
        let backend = create_backend(&desc)?;

        Ok(Self {
            inner: Arc::new(Mutex::new(DeviceInner {
                backend,
                images: HashMap::new(),
                buffers: HashMap::new(),
                image_states: HashMap::new(),
                buffer_states: HashMap::new(),
                samplers: HashMap::new(),
                shaders: HashMap::new(),
                shader_reflections: HashMap::new(),
                pipeline_layouts: HashMap::new(),
                pipelines: HashMap::new(),
                bind_groups: HashMap::new(),
                surfaces: HashMap::new(),
                frames: HashMap::new(),
                deferred_destroys: Vec::new(),
                pending_transient_destroys: Vec::new(),
                image_handles: HandleAllocator::default(),
                buffer_handles: HandleAllocator::default(),
                sampler_handles: HandleAllocator::default(),
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

    pub fn format_capabilities(&self, format: Format) -> FormatCapabilities {
        self.inner
            .lock()
            .expect("device mutex poisoned")
            .backend
            .format_capabilities(format)
    }

    pub fn native_handle_capabilities(&self) -> NativeHandleCapabilities {
        self.inner
            .lock()
            .expect("device mutex poisoned")
            .backend
            .native_handle_capabilities()
    }

    pub fn raw_capabilities(&self) -> BackendRawCapabilities {
        self.inner
            .lock()
            .expect("device mutex poisoned")
            .backend
            .raw_capabilities()
    }

    pub fn create_image(&self, desc: ImageDesc) -> Result<ImageHandle> {
        desc.validate()?;
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let handle = ImageHandle(inner.image_handles.alloc());
        inner.backend.create_image(handle, desc)?;
        if let Some(name) = desc.debug_name {
            inner.backend.set_image_debug_name(handle, name);
        }
        inner.images.insert(handle, desc);
        Ok(handle)
    }

    /// Import an externally created image into the engine's handle registry.
    ///
    /// # Safety
    ///
    /// The caller must ensure the external image and image view were created
    /// from a compatible backend device, outlive the returned engine handle,
    /// and match `desc.desc` closely enough for backend commands using the
    /// image. The engine borrows the native objects and will not destroy them.
    pub unsafe fn import_external_image(&self, desc: ExternalImageDesc) -> Result<ImageHandle> {
        desc.validate()?;
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let handle = ImageHandle(inner.image_handles.alloc());
        unsafe {
            inner.backend.import_external_image(handle, desc)?;
        }
        inner.images.insert(handle, desc.desc);
        Ok(handle)
    }

    /// Create an image whose lifetime is tied to one frame.
    ///
    /// On backends that support aliasing (Vulkan) the image is created without
    /// immediately allocating memory; memory is bound during `Frame::flush` based
    /// on the render graph's alias plan.  The caller must add the returned handle
    /// to the frame's transient list via `Frame::add_transient_image` so the
    /// device destroys it automatically after the GPU finishes the frame.
    pub fn create_transient_image(&self, desc: ImageDesc) -> Result<ImageHandle> {
        let desc = ImageDesc {
            transient: true,
            ..desc
        };
        desc.validate()?;
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let handle = ImageHandle(inner.image_handles.alloc());
        inner.backend.create_transient_image(handle, desc)?;
        if let Some(name) = desc.debug_name {
            inner.backend.set_image_debug_name(handle, name);
        }
        inner.images.insert(handle, desc);
        Ok(handle)
    }

    pub fn destroy_image(&self, handle: ImageHandle) -> Result<()> {
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let _desc = inner.images.remove(&handle).ok_or(Error::InvalidHandle)?;
        inner.image_states.retain(|key, _| key.image != handle);
        inner.deferred_destroys.push(DeferredDestroy::Image(handle));
        Ok(())
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

    /// Import an externally created buffer into the engine's handle registry.
    ///
    /// # Safety
    ///
    /// The caller must ensure the external buffer was created from a compatible
    /// backend device, outlives the returned engine handle, and matches
    /// `desc.desc` closely enough for backend commands using the buffer. The
    /// engine borrows the native object and will not destroy it.
    pub unsafe fn import_external_buffer(&self, desc: ExternalBufferDesc) -> Result<BufferHandle> {
        desc.validate()?;
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let handle = BufferHandle(inner.buffer_handles.alloc());
        unsafe {
            inner.backend.import_external_buffer(handle, desc)?;
        }
        inner.buffers.insert(handle, desc.desc);
        Ok(handle)
    }

    pub fn destroy_buffer(&self, handle: BufferHandle) -> Result<()> {
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let _desc = inner.buffers.remove(&handle).ok_or(Error::InvalidHandle)?;
        inner.buffer_states.retain(|key, _| key.buffer != handle);
        inner
            .deferred_destroys
            .push(DeferredDestroy::Buffer(handle));
        Ok(())
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

    pub fn create_sampler(&self, desc: SamplerDesc) -> Result<SamplerHandle> {
        desc.validate()?;
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let handle = SamplerHandle(inner.sampler_handles.alloc());
        inner.backend.create_sampler(handle, desc)?;
        inner.samplers.insert(handle, desc);
        Ok(handle)
    }

    pub fn destroy_sampler(&self, handle: SamplerHandle) -> Result<()> {
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let _desc = inner.samplers.remove(&handle).ok_or(Error::InvalidHandle)?;
        inner
            .deferred_destroys
            .push(DeferredDestroy::Sampler(handle));
        Ok(())
    }

    pub fn sampler_desc(&self, handle: SamplerHandle) -> Result<SamplerDesc> {
        let inner = self.inner.lock().expect("device mutex poisoned");
        inner
            .samplers
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
        inner
            .deferred_destroys
            .push(DeferredDestroy::Shader(handle));
        Ok(())
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
        inner
            .deferred_destroys
            .push(DeferredDestroy::PipelineLayout(handle));
        Ok(())
    }

    pub fn create_bind_group(&self, desc: BindGroupDesc) -> Result<BindGroupHandle> {
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        inner.validate_bind_group_desc(&desc)?;
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
        inner
            .deferred_destroys
            .push(DeferredDestroy::BindGroup(handle));
        Ok(())
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
                let layout = merge_shader_reflections(&inner.shader_reflections, &desc);
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

    pub fn set_image_debug_name(&self, handle: ImageHandle, name: &str) -> Result<()> {
        let inner = self.inner.lock().expect("device mutex poisoned");
        if !inner.images.contains_key(&handle) {
            return Err(Error::InvalidHandle);
        }
        inner.backend.set_image_debug_name(handle, name);
        Ok(())
    }

    pub fn set_buffer_debug_name(&self, handle: BufferHandle, name: &str) -> Result<()> {
        let inner = self.inner.lock().expect("device mutex poisoned");
        if !inner.buffers.contains_key(&handle) {
            return Err(Error::InvalidHandle);
        }
        inner.backend.set_buffer_debug_name(handle, name);
        Ok(())
    }

    pub fn set_pipeline_debug_name(&self, handle: PipelineHandle, name: &str) -> Result<()> {
        let inner = self.inner.lock().expect("device mutex poisoned");
        if !inner.pipelines.contains_key(&handle) {
            return Err(Error::InvalidHandle);
        }
        inner.backend.set_pipeline_debug_name(handle, name);
        Ok(())
    }

    pub fn supported_gpu_capture_tools(&self) -> Vec<GpuCaptureTool> {
        self.inner
            .lock()
            .expect("device mutex poisoned")
            .backend
            .supported_gpu_capture_tools()
    }

    pub fn begin_gpu_capture(&self, desc: &GpuCaptureDesc) -> Result<()> {
        self.inner
            .lock()
            .expect("device mutex poisoned")
            .backend
            .begin_gpu_capture(desc)
    }

    pub fn end_gpu_capture(&self, tool: GpuCaptureTool) -> Result<()> {
        self.inner
            .lock()
            .expect("device mutex poisoned")
            .backend
            .end_gpu_capture(tool)
    }

    pub fn destroy_pipeline(&self, handle: PipelineHandle) -> Result<()> {
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let desc = inner
            .pipelines
            .remove(&handle)
            .ok_or(Error::InvalidHandle)?;
        // Pipeline must be destroyed before its layout per Vulkan spec — push in order.
        inner
            .deferred_destroys
            .push(DeferredDestroy::Pipeline(handle));
        if let Some(lh) = desc.owned_layout() {
            inner.pipeline_layouts.remove(&lh);
            inner
                .deferred_destroys
                .push(DeferredDestroy::PipelineLayout(lh));
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn create_surface(&self, desc: NativeSurfaceDesc) -> Result<SurfaceHandle> {
        desc.validate()?;
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let handle = SurfaceHandle(inner.surface_handles.alloc());
        let info = inner.backend.create_surface(handle, desc)?;
        inner.surfaces.insert(
            handle,
            SurfaceState {
                info,
                events: Vec::new(),
            },
        );
        Ok(handle)
    }

    pub fn resize_surface(&self, handle: SurfaceHandle, size: SurfaceSize) -> Result<()> {
        size.validate()?;
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let old = inner
            .surfaces
            .get(&handle)
            .map(|surface| surface.info)
            .ok_or(Error::InvalidHandle)?;
        let new = inner.backend.resize_surface(handle, size)?;
        let surface = inner
            .surfaces
            .get_mut(&handle)
            .ok_or(Error::InvalidHandle)?;
        queue_surface_events(&mut surface.events, old, new);
        surface.info = new;
        Ok(())
    }

    pub fn recreate_surface(&self, handle: SurfaceHandle, desc: SurfaceRecreateDesc) -> Result<()> {
        desc.validate()?;
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let old = inner
            .surfaces
            .get(&handle)
            .map(|surface| surface.info)
            .ok_or(Error::InvalidHandle)?;
        let new = inner.backend.recreate_surface(handle, desc, old)?;
        let surface = inner
            .surfaces
            .get_mut(&handle)
            .ok_or(Error::InvalidHandle)?;
        queue_surface_events(&mut surface.events, old, new);
        surface.info = new;
        Ok(())
    }

    pub fn surface_info(&self, handle: SurfaceHandle) -> Result<SurfaceInfo> {
        let inner = self.inner.lock().expect("device mutex poisoned");
        inner
            .surfaces
            .get(&handle)
            .map(|surface| surface.info)
            .ok_or(Error::InvalidHandle)
    }

    pub fn drain_surface_events(&self, handle: SurfaceHandle) -> Result<Vec<SurfaceEvent>> {
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let surface = inner
            .surfaces
            .get_mut(&handle)
            .ok_or(Error::InvalidHandle)?;
        Ok(std::mem::take(&mut surface.events))
    }

    /// Acquire the next swapchain image.
    ///
    /// Returns `(handle, slot)` where `slot` is the stable swapchain image
    /// index (0..swapchain_image_count) — suitable as a per-frame cache key.
    pub fn acquire_surface_image(&self, surface: SurfaceHandle) -> Result<(ImageHandle, u64)> {
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        if !inner.surfaces.contains_key(&surface) {
            return Err(Error::InvalidHandle);
        }
        let handle = ImageHandle(inner.image_handles.alloc());
        let (desc, slot) = inner.backend.acquire_surface_image(surface, handle)?;
        inner.images.insert(handle, desc);
        inner.image_states.retain(|key, _| key.image != handle);
        Ok((handle, slot))
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
        let _surface = inner.surfaces.remove(&handle).ok_or(Error::InvalidHandle)?;
        inner.backend.destroy_surface(handle)
    }

    pub fn query_surface_capabilities(&self, handle: SurfaceHandle) -> Result<SurfaceCapabilities> {
        let inner = self.inner.lock().expect("device mutex poisoned");
        if !inner.surfaces.contains_key(&handle) {
            return Err(Error::InvalidHandle);
        }
        inner.backend.query_surface_capabilities(handle)
    }

    pub fn surface_hdr_caps(&self, handle: SurfaceHandle) -> Result<SurfaceHdrCaps> {
        self.query_surface_capabilities(handle)
            .map(|capabilities| SurfaceHdrCaps::from_surface_capabilities(&capabilities))
    }

    pub fn begin_frame(&self) -> Result<Frame> {
        let mut inner = self.inner.lock().expect("device mutex poisoned");
        let handle = FrameHandle(inner.frame_handles.alloc());
        let mut graph = RenderGraph::new();
        for (key, state) in &inner.image_states {
            graph.set_initial_image_subresource_state(key.image, key.subresource, *state);
        }
        for (key, state) in &inner.buffer_states {
            graph.set_initial_buffer_range_state(key.buffer, key.offset, key.size, *state);
        }
        inner.frames.insert(handle, graph);
        Ok(Frame {
            device: self.clone(),
            handle,
            submitted: false,
            last_submission: None,
            transient_images: Vec::new(),
        })
    }

    pub fn wait_for_submission(&self, token: SubmissionHandle) -> Result<()> {
        self.inner
            .lock()
            .expect("device mutex poisoned")
            .backend
            .wait_submission(token)
    }

    pub fn wait_idle(&self) -> Result<()> {
        self.inner
            .lock()
            .expect("device mutex poisoned")
            .backend
            .wait_idle()
    }
}

impl DeviceInner {
    fn validate_bind_group_desc(&self, desc: &BindGroupDesc) -> Result<()> {
        let layout = self
            .pipeline_layouts
            .get(&desc.layout)
            .ok_or(Error::InvalidHandle)?;
        let mut seen_paths = HashSet::new();

        for entry in &desc.entries {
            if !seen_paths.insert(entry.path.as_str()) {
                return Err(Error::InvalidInput(format!(
                    "bind group entry path '{}' was specified more than once",
                    entry.path
                )));
            }

            let binding = layout
                .groups
                .iter()
                .flat_map(|group| group.bindings.iter())
                .find(|binding| binding.path == entry.path)
                .ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "bind group entry path '{}' was not found in pipeline layout",
                        entry.path
                    ))
                })?;

            validate_binding_resource_kind(&entry.path, binding.kind, entry.resource)?;
            self.validate_binding_resource_handle(entry.resource)?;
        }

        Ok(())
    }

    fn validate_binding_resource_handle(&self, resource: ResourceBinding) -> Result<()> {
        match resource {
            ResourceBinding::Image(handle) if self.images.contains_key(&handle) => Ok(()),
            ResourceBinding::Buffer(handle) if self.buffers.contains_key(&handle) => Ok(()),
            ResourceBinding::Sampler(handle) if self.samplers.contains_key(&handle) => Ok(()),
            _ => Err(Error::InvalidHandle),
        }
    }
}

fn validate_binding_resource_kind(
    path: &str,
    expected: BindingKind,
    resource: ResourceBinding,
) -> Result<()> {
    let valid = matches!(
        (expected, resource),
        (
            BindingKind::SampledImage | BindingKind::StorageImage,
            ResourceBinding::Image(_)
        ) | (
            BindingKind::UniformBuffer | BindingKind::StorageBuffer,
            ResourceBinding::Buffer(_)
        ) | (BindingKind::Sampler, ResourceBinding::Sampler(_))
    );

    if valid {
        return Ok(());
    }

    Err(Error::InvalidInput(format!(
        "bind group entry path '{path}' expected {expected:?}, got {}",
        resource_binding_label(resource)
    )))
}

fn resource_binding_label(resource: ResourceBinding) -> &'static str {
    match resource {
        ResourceBinding::Image(_) => "image",
        ResourceBinding::Buffer(_) => "buffer",
        ResourceBinding::Sampler(_) => "sampler",
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
    use crate::CanonicalBinding;
    use std::collections::BTreeMap;

    let mut groups: BTreeMap<usize, (String, Vec<(String, CanonicalBinding)>)> = BTreeMap::new();
    let mut push_constants_bytes = 0;
    let mut push_constants_stage_mask = StageMask::default();

    let shaders: Vec<ShaderHandle> = [Some(desc.vertex_shader), desc.fragment_shader]
        .into_iter()
        .flatten()
        .collect();

    for shader in shaders {
        let Some(reflection) = reflections.get(&shader) else {
            continue;
        };
        push_constants_bytes = push_constants_bytes.max(reflection.layout.push_constants_bytes);
        if reflection.layout.push_constants_bytes != 0 {
            push_constants_stage_mask |= reflection.layout.push_constants_stage_mask;
        }
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
        push_constants_bytes,
        push_constants_stage_mask,
    }
}

fn queue_surface_events(events: &mut Vec<SurfaceEvent>, old: SurfaceInfo, new: SurfaceInfo) {
    if old.size != new.size {
        events.push(SurfaceEvent::Resized {
            old: old.size,
            new: new.size,
        });
    }
    if old.format != new.format {
        events.push(SurfaceEvent::FormatChanged {
            old: old.format,
            new: new.format,
        });
    }
    if old.color_space != new.color_space {
        events.push(SurfaceEvent::ColorSpaceChanged {
            old: old.color_space,
            new: new.color_space,
        });
    }
    events.push(SurfaceEvent::Recreated { old, new });
}

#[derive(Clone)]
pub struct Frame {
    device: Device,
    handle: FrameHandle,
    submitted: bool,
    last_submission: Option<SubmissionHandle>,
    /// Transient images owned by this frame; scheduled for destruction after
    /// the GPU signals the frame fence.
    transient_images: Vec<ImageHandle>,
}

impl Frame {
    pub fn handle(&self) -> FrameHandle {
        self.handle
    }

    /// Register a transient image with this frame.  The device will destroy it
    /// automatically after the GPU finishes this frame's work (at the start of
    /// the next flush, when the previous frame's fence is signaled).
    pub fn add_transient_image(&mut self, handle: ImageHandle) {
        self.transient_images.push(handle);
    }

    pub fn graph_mut<R>(&mut self, f: impl FnOnce(&mut RenderGraph) -> Result<R>) -> Result<R> {
        let mut inner = self.device.inner.lock().expect("device mutex poisoned");
        let graph = inner
            .frames
            .get_mut(&self.handle)
            .ok_or(Error::InvalidHandle)?;
        f(graph)
    }

    pub fn flush(&mut self) -> Result<SubmissionHandle> {
        let compiled = {
            let inner = self.device.inner.lock().expect("device mutex poisoned");
            let graph = inner.frames.get(&self.handle).ok_or(Error::InvalidHandle)?;
            graph.compile()?
        };

        let token = {
            let mut inner = self.device.inner.lock().expect("device mutex poisoned");
            // `backend.flush` → `submit_graph` waits the previous frame's fence
            // before submitting.  Everything after this point is safe to destroy.
            let token = inner.backend.flush(&compiled)?;

            for (key, state) in &compiled.final_image_states {
                if inner.images.contains_key(&key.image) {
                    inner.image_states.insert(*key, *state);
                }
            }
            for (key, state) in &compiled.final_buffer_states {
                if inner.buffers.contains_key(&key.buffer) {
                    inner.buffer_states.insert(*key, *state);
                }
            }

            // Drain deferred destroys (user-destroyed resources) and transient
            // images from the previous frame — both are safe now that the
            // previous frame's fence has been waited inside `backend.flush`.
            let deferred = std::mem::take(&mut inner.deferred_destroys);
            for item in deferred {
                let _ = match item {
                    DeferredDestroy::Image(h) => inner.backend.destroy_image(h),
                    DeferredDestroy::Buffer(h) => inner.backend.destroy_buffer(h),
                    DeferredDestroy::Sampler(h) => inner.backend.destroy_sampler(h),
                    DeferredDestroy::Shader(h) => inner.backend.destroy_shader(h),
                    DeferredDestroy::Pipeline(h) => inner.backend.destroy_pipeline(h),
                    DeferredDestroy::PipelineLayout(h) => inner.backend.destroy_pipeline_layout(h),
                    DeferredDestroy::BindGroup(h) => inner.backend.destroy_bind_group(h),
                };
            }

            let pending = std::mem::take(&mut inner.pending_transient_destroys);
            for handle in pending {
                inner.images.remove(&handle);
                inner.image_states.retain(|key, _| key.image != handle);
                let _ = inner.backend.destroy_image(handle);
            }

            // Schedule this frame's transient images for destruction next flush.
            inner
                .pending_transient_destroys
                .extend(self.transient_images.drain(..));

            token
        };
        self.submitted = true;
        self.last_submission = Some(token);
        Ok(token)
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

    /// Block until the GPU finishes the work submitted by `flush`.
    /// If `flush` has not been called yet this is a no-op.
    pub fn wait(&self) -> Result<()> {
        match self.last_submission {
            Some(token) => self.device.wait_for_submission(token),
            None => Ok(()),
        }
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.device.inner.lock() {
            inner.frames.remove(&self.handle);
            // Transient images that were never flushed are safe to destroy
            // immediately (they were never submitted to the GPU).
            for handle in self.transient_images.drain(..) {
                inner.images.remove(&handle);
                inner.image_states.retain(|key, _| key.image != handle);
                let _ = inner.backend.destroy_image(handle);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SurfaceColorSpace;

    #[test]
    fn surface_events_capture_resize_format_color_space_and_recreation() {
        let old = SurfaceInfo {
            size: SurfaceSize {
                width: 640,
                height: 360,
            },
            format: crate::Format::Bgra8Unorm,
            color_space: SurfaceColorSpace::SrgbNonlinear,
        };
        let new = SurfaceInfo {
            size: SurfaceSize {
                width: 1280,
                height: 720,
            },
            format: crate::Format::Rgba16Float,
            color_space: SurfaceColorSpace::Hdr10St2084,
        };
        let mut events = Vec::new();

        queue_surface_events(&mut events, old, new);

        assert_eq!(
            events,
            vec![
                SurfaceEvent::Resized {
                    old: old.size,
                    new: new.size,
                },
                SurfaceEvent::FormatChanged {
                    old: old.format,
                    new: new.format,
                },
                SurfaceEvent::ColorSpaceChanged {
                    old: old.color_space,
                    new: new.color_space,
                },
                SurfaceEvent::Recreated { old, new },
            ]
        );
    }

    #[test]
    fn surface_events_always_capture_recreation() {
        let info = SurfaceInfo {
            size: SurfaceSize {
                width: 640,
                height: 360,
            },
            format: crate::Format::Bgra8Unorm,
            color_space: SurfaceColorSpace::SrgbNonlinear,
        };
        let mut events = Vec::new();

        queue_surface_events(&mut events, info, info);

        assert_eq!(
            events,
            vec![SurfaceEvent::Recreated {
                old: info,
                new: info
            }]
        );
    }
}
