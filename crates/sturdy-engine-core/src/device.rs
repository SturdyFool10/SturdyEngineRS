use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use crate::NativeSurfaceDesc;
use crate::backend::factory::create_backend;
use crate::backend::{Backend, BackendKind, factory};
use crate::handles::HandleAllocator;
use crate::shader_object::{ShaderObjectDesc, ShaderObjectHandle};
use crate::{
    AccelerationStructureBuildMode, AccelerationStructureBuildSizes, AccelerationStructureDesc,
    AccelerationStructureHandle, AdapterInfo, AntiLagMode, BackendRawCapabilities, BindGroupDesc,
    BindGroupHandle, BindingKind, BlasBuildDesc, BufferDesc, BufferHandle, BufferStateKey,
    BufferUsage, CanonicalGroupLayout, CanonicalPipelineLayout, Caps, ComputePipelineDesc, Error,
    ExternalBufferDesc, ExternalImageDesc, Format, FormatCapabilities, FrameHandle, GpuCaptureDesc,
    GpuCaptureTool, GraphicsPipelineDesc, HdrMetadata, ImageDesc, ImageHandle, ImageStateKey,
    ImageUsage, LatencyMode, NativeHandleCapabilities, PipelineHandle, PipelineLayoutHandle,
    RayTracingPipelineDesc, ReflexMode, RenderGraph, ResourceBinding, Result, RgState, SamplerDesc,
    SamplerHandle, ShaderBindingTable, ShaderBindingTableDesc, ShaderBindingTableRegion,
    ShaderDesc, ShaderHandle, ShaderReflection, ShaderSource, StageMask, SubmissionHandle,
    SurfaceCapabilities, SurfaceEvent, SurfaceHandle, SurfaceHdrCaps, SurfaceInfo,
    SurfaceRecreateDesc, SurfaceSize, TlasBuildDesc, VideoSessionDesc, VideoSessionHandle,
};

mod device_desc;
pub use device_desc::{DeviceDesc, DeviceFeature};

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
    AccelerationStructure(AccelerationStructureHandle),
    Sampler(SamplerHandle),
    Shader(ShaderHandle),
    Pipeline(PipelineHandle),
    PipelineLayout(PipelineLayoutHandle),
    BindGroup(BindGroupHandle),
    ShaderObject(ShaderObjectHandle),
    VideoSession(VideoSessionHandle),
    IndirectCommandLayout(crate::IndirectCommandLayoutHandle),
    OpticalFlowSession(crate::OpticalFlowSessionHandle),
}

struct DeviceInner {
    backend: Box<dyn Backend>,
    /// The descriptor used to create (or most recently rebuild) the backend.
    /// Preserved through restarts so `rebuild_backend` can apply incremental changes.
    creation_desc: DeviceDesc,
    images: HashMap<ImageHandle, ImageDesc>,
    buffers: HashMap<BufferHandle, BufferDesc>,
    acceleration_structures: HashMap<AccelerationStructureHandle, AccelerationStructureDesc>,
    image_states: HashMap<ImageStateKey, RgState>,
    buffer_states: HashMap<BufferStateKey, RgState>,
    samplers: HashMap<SamplerHandle, SamplerDesc>,
    bindless_sampled_images: HashMap<ImageHandle, u32>,
    bindless_storage_images: HashMap<ImageHandle, u32>,
    bindless_samplers: HashMap<SamplerHandle, u32>,
    bindless_storage_buffers: HashMap<BufferHandle, u32>,
    shaders: HashMap<ShaderHandle, ShaderDesc>,
    shader_reflections: HashMap<ShaderHandle, ShaderReflection>,
    pipeline_layouts: HashMap<PipelineLayoutHandle, CanonicalPipelineLayout>,
    pipelines: HashMap<PipelineHandle, PipelineDesc>,
    bind_groups: HashMap<BindGroupHandle, BindGroupDesc>,
    shader_objects: HashMap<ShaderObjectHandle, ShaderObjectDesc>,
    surfaces: HashMap<SurfaceHandle, SurfaceState>,
    frames: HashMap<FrameHandle, RenderGraph>,
    /// In-process SPIR-V compilation cache keyed by a hash of the shader source
    /// content, entry point, stage, and backend IR target.
    ///
    /// Only caches sources whose content is fixed at program startup (Inline,
    /// MemoryUtf8, MemoryBytes).  File sources are intentionally excluded so hot
    /// reload always sees the updated content on disk.
    shader_compile_cache: HashMap<u64, (ShaderDesc, ShaderReflection)>,
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
    acceleration_structure_handles: HandleAllocator,
    sampler_handles: HandleAllocator,
    shader_handles: HandleAllocator,
    pipeline_layout_handles: HandleAllocator,
    pipeline_handles: HandleAllocator,
    bind_group_handles: HandleAllocator,
    shader_object_handles: HandleAllocator,
    surface_handles: HandleAllocator,
    frame_handles: HandleAllocator,
    video_sessions: HashMap<VideoSessionHandle, VideoSessionDesc>,
    video_session_handles: HandleAllocator,
    indirect_command_layouts:
        HashMap<crate::IndirectCommandLayoutHandle, crate::IndirectCommandLayoutDesc>,
    indirect_command_layout_handles: HandleAllocator,
    optical_flow_sessions: HashMap<crate::OpticalFlowSessionHandle, crate::OpticalFlowSessionDesc>,
    optical_flow_session_handles: HandleAllocator,
    /// GFX-5b: exportable fence handle allocator.
    fence_handles: HandleAllocator,
}

struct SurfaceState {
    info: SurfaceInfo,
    events: Vec<SurfaceEvent>,
}

impl Device {
    pub fn create(desc: DeviceDesc) -> Result<Self> {
        let backend = create_backend(&desc)?;

        let slot_count = backend.transient_pool_slot_count();

        let device = Self {
            inner: Arc::new(Mutex::new(DeviceInner {
                backend,
                creation_desc: desc,
                images: HashMap::new(),
                buffers: HashMap::new(),
                acceleration_structures: HashMap::new(),
                image_states: HashMap::new(),
                buffer_states: HashMap::new(),
                samplers: HashMap::new(),
                bindless_sampled_images: HashMap::new(),
                bindless_storage_images: HashMap::new(),
                bindless_samplers: HashMap::new(),
                bindless_storage_buffers: HashMap::new(),
                shaders: HashMap::new(),
                shader_reflections: HashMap::new(),
                pipeline_layouts: HashMap::new(),
                pipelines: HashMap::new(),
                bind_groups: HashMap::new(),
                shader_objects: HashMap::new(),
                surfaces: HashMap::new(),
                frames: HashMap::new(),
                shader_compile_cache: HashMap::new(),
                deferred_destroys: Vec::new(),
                pending_transient_destroys: Vec::new(),
                image_handles: HandleAllocator::default(),
                buffer_handles: HandleAllocator::default(),
                acceleration_structure_handles: HandleAllocator::default(),
                sampler_handles: HandleAllocator::default(),
                shader_handles: HandleAllocator::default(),
                pipeline_layout_handles: HandleAllocator::default(),
                pipeline_handles: HandleAllocator::default(),
                bind_group_handles: HandleAllocator::default(),
                shader_object_handles: HandleAllocator::default(),
                surface_handles: HandleAllocator::default(),
                frame_handles: HandleAllocator::default(),
                video_sessions: HashMap::new(),
                video_session_handles: HandleAllocator::default(),
                indirect_command_layouts: HashMap::new(),
                indirect_command_layout_handles: HandleAllocator::default(),
                optical_flow_sessions: HashMap::new(),
                optical_flow_session_handles: HandleAllocator::default(),
                fence_handles: HandleAllocator::default(),
            })),
        };

        // Track 11a: allocate one BufferHandle per frame slot and register the transient pool
        // buffers in the backend resource registry so they can be bound via push descriptors.
        if slot_count > 0 {
            let mut inner = device.inner.lock();
            let handles: Vec<crate::BufferHandle> = (0..slot_count)
                .map(|_| crate::BufferHandle(inner.buffer_handles.alloc()))
                .collect();
            inner.backend.register_transient_buffer_handles(&handles);
        }

        Ok(device)
    }

    /// The `DeviceDesc` used to create (or most recently rebuild) this device.
    pub fn creation_desc(&self) -> DeviceDesc {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().creation_desc.clone()
    }

    /// Replace the graphics backend with a new one built from `new_desc`.
    ///
    /// All existing GPU resource handles (images, buffers, pipelines, shaders,
    /// surfaces, samplers) become invalid after this call — the caller is
    /// responsible for recreating them.  The caller should also recreate any
    /// presentation surface before issuing new frames.
    ///
    /// Steps performed:
    /// 1. Wait for the GPU to be completely idle (`vkDeviceWaitIdle`).
    /// 2. Create the new backend (fails fast if the new config is invalid).
    /// 3. Swap and drop the old backend (triggers full Vulkan resource cleanup).
    /// 4. Clear all resource-tracking state in `DeviceInner`.
    /// 5. Re-register transient buffer handles for the new backend.
    pub fn rebuild_backend(&self, new_desc: &DeviceDesc) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();

        // 1. Wait for idle so the GPU is not using any resources we're about to destroy.
        inner.backend.wait_idle()?;

        // 2. Create the new backend. Fail before touching anything if config is bad.
        let new_backend = create_backend(new_desc)
            .map_err(|e| Error::Backend(format!("backend rebuild failed: {e}")))?;

        // 3. Swap — dropping the old backend triggers full Vulkan cleanup (vkDestroyDevice etc.).
        let _old = std::mem::replace(&mut inner.backend, new_backend);
        drop(_old);

        // 4. Clear all resource-tracking maps. Every handle the caller holds is now invalid.
        inner.images.clear();
        inner.buffers.clear();
        inner.acceleration_structures.clear();
        inner.image_states.clear();
        inner.buffer_states.clear();
        inner.samplers.clear();
        inner.bindless_sampled_images.clear();
        inner.bindless_storage_images.clear();
        inner.bindless_samplers.clear();
        inner.bindless_storage_buffers.clear();
        inner.shaders.clear();
        inner.shader_reflections.clear();
        inner.shader_compile_cache.clear();
        inner.pipeline_layouts.clear();
        inner.pipelines.clear();
        inner.bind_groups.clear();
        inner.shader_objects.clear();
        inner.surfaces.clear();
        inner.frames.clear();
        inner.video_sessions.clear();
        inner.indirect_command_layouts.clear();
        inner.optical_flow_sessions.clear();
        inner.deferred_destroys.clear();
        inner.pending_transient_destroys.clear();
        // Reset handle allocators so new handles start from a clean state.
        inner.image_handles = HandleAllocator::default();
        inner.buffer_handles = HandleAllocator::default();
        inner.acceleration_structure_handles = HandleAllocator::default();
        inner.sampler_handles = HandleAllocator::default();
        inner.shader_handles = HandleAllocator::default();
        inner.pipeline_layout_handles = HandleAllocator::default();
        inner.pipeline_handles = HandleAllocator::default();
        inner.bind_group_handles = HandleAllocator::default();
        inner.shader_object_handles = HandleAllocator::default();
        inner.surface_handles = HandleAllocator::default();
        inner.frame_handles = HandleAllocator::default();
        inner.video_session_handles = HandleAllocator::default();
        inner.indirect_command_layout_handles = HandleAllocator::default();
        inner.optical_flow_session_handles = HandleAllocator::default();
        inner.fence_handles = HandleAllocator::default();

        // 5. Re-register transient buffer handles for the new backend.
        let slot_count = inner.backend.transient_pool_slot_count();
        if slot_count > 0 {
            let handles: Vec<crate::BufferHandle> = (0..slot_count)
                .map(|_| crate::BufferHandle(inner.buffer_handles.alloc()))
                .collect();
            inner.backend.register_transient_buffer_handles(&handles);
        }

        // Update the stored desc so future callers can inspect what's active.
        inner.creation_desc = new_desc.clone();

        Ok(())
    }

    pub fn backend_kind(&self) -> BackendKind {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.kind()
    }

    pub fn adapter_name(&self) -> Option<String> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.adapter_name()
    }

    pub fn caps(&self) -> Caps {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.caps()
    }

    /// Current GPU memory usage and sub-allocator capacity.
    ///
    /// Returns `None` on backends that don't expose allocator statistics.
    /// Call once per frame; the overhead is a single mutex read.
    pub fn memory_budget(&self) -> Option<crate::GpuMemoryBudget> {
        //panic allowed, reason = "poisoned device mutex is unrecoverable"
        self.inner.lock().backend.memory_budget()
    }

    /// Per-heap memory budget from `VK_EXT_memory_budget`.
    ///
    /// Returns `None` when the extension is unavailable. More precise than `memory_budget()`
    /// because the driver reports actual OS-level budget including other processes.
    pub fn memory_budget_ext(&self) -> Option<crate::MemoryBudgetReport> {
        //panic allowed, reason = "poisoned device mutex is unrecoverable"
        self.inner.lock().backend.memory_budget_ext()
    }

    // ── Bindless descriptor heap (Track 8a) ───────────────────────────────────

    /// Returns `true` when the GPU + driver support the bindless descriptor heap.
    ///
    /// Requires `VK_EXT_descriptor_indexing` with `runtime_descriptor_array`
    /// and `descriptor_binding_partially_bound`. All discrete GPUs since 2016
    /// and all current consoles support this.
    pub fn bindless_supported(&self) -> bool {
        //panic allowed, reason = "poisoned device mutex is unrecoverable"
        self.inner.lock().backend.bindless_supported()
    }

    /// Register a sampled image in the global bindless heap and return its stable index.
    ///
    /// The returned `u32` is valid for the lifetime of the engine. Embed it in
    /// push constants or a per-draw data buffer; sample in shaders via:
    /// `g_bindless_textures[NonUniformResourceIndex(index)].Sample(...)`.
    ///
    /// Returns `None` if bindless is not supported or the heap is full.
    pub fn register_bindless_sampled_image(&self, handle: ImageHandle) -> Option<u32> {
        //panic allowed, reason = "poisoned device mutex is unrecoverable"
        let mut inner = self.inner.lock();
        register_bindless_sampled_image(&mut inner, handle)
    }

    /// Register a sampler in the bindless heap and return its stable index.
    pub fn register_bindless_sampler(&self, handle: SamplerHandle) -> Option<u32> {
        //panic allowed, reason = "poisoned device mutex is unrecoverable"
        let mut inner = self.inner.lock();
        register_bindless_sampler(&mut inner, handle)
    }

    /// Register a storage (read-write) image in the bindless heap.
    pub fn register_bindless_storage_image(&self, handle: ImageHandle) -> Option<u32> {
        //panic allowed, reason = "poisoned device mutex is unrecoverable"
        let mut inner = self.inner.lock();
        register_bindless_storage_image(&mut inner, handle)
    }

    /// Register a storage buffer in the bindless heap.
    pub fn register_bindless_storage_buffer(&self, handle: BufferHandle) -> Option<u32> {
        //panic allowed, reason = "poisoned device mutex is unrecoverable"
        let mut inner = self.inner.lock();
        register_bindless_storage_buffer(&mut inner, handle)
    }

    /// Return the stable bindless index assigned to this buffer, if any.
    ///
    /// Non-`None` for every storage buffer registered in the bindless heap
    /// (all non-transient `STORAGE` buffers when bindless is supported).
    /// Use the returned index in shaders via `g_bindless_buffers[idx]`.
    pub fn buffer_bindless_index(&self, handle: BufferHandle) -> Option<u32> {
        let inner = self.inner.lock();
        inner.bindless_storage_buffers.get(&handle).copied()
    }

    // ── Parallel secondary command buffer recording ───────────────────────────

    /// Returns `true` when the backend supports parallel secondary command buffer
    /// recording.  Always `true` on Vulkan; `false` on stub/null backends.
    pub fn parallel_secondary_recording_supported(&self) -> bool {
        self.inner
            .lock()
            .backend
            .parallel_secondary_recording_supported()
    }

    /// Pre-allocate `count` secondary command buffer recording slots on all
    /// per-frame contexts.  Call once at init time or when a higher bin/cascade
    /// count is needed.
    ///
    /// `queue_family_index` is the queue family index for the secondary slots
    /// (typically the graphics family from `caps().queue_families.graphics`).
    pub fn prepare_parallel_secondary_capacity(
        &self,
        count: usize,
        queue_family_index: u32,
    ) -> Result<()> {
        self.inner
            .lock()
            .backend
            .prepare_parallel_secondary_capacity(count, queue_family_index)
    }

    // ── Descriptor buffer (GFX-7a) ────────────────────────────────────────────

    /// Returns the required alignment for descriptor buffer offsets when
    /// `VK_EXT_descriptor_buffer` is available, or `None` otherwise.
    pub fn descriptor_buffer_offset_alignment(&self) -> Option<u64> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
        if !inner.backend.caps().features.descriptor_buffer {
            return None;
        }
        inner.backend.descriptor_buffer_offset_alignment()
    }

    /// GFX-7b: Query the driver-reported byte size of a descriptor of the given type.
    ///
    /// Required to size the resource/sampler heap buffers for `VK_EXT_descriptor_heap`.
    /// Returns `None` when `BackendFeatures::descriptor_heap` is not available.
    pub fn descriptor_heap_type_size(&self, descriptor_type: u32) -> Option<u64> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
        if !inner.backend.caps().features.descriptor_heap {
            return None;
        }
        inner.backend.descriptor_heap_type_size(descriptor_type)
    }

    // ── Shader objects (GFX-8) ────────────────────────────────────────────────

    /// Create a standalone shader object that can be bound without a pipeline.
    ///
    /// Requires `BackendFeatures::shader_object`.
    pub fn create_shader_object(&self, desc: ShaderObjectDesc) -> Result<ShaderObjectHandle> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        if !inner.backend.caps().features.shader_object {
            return Err(Error::Unsupported(
                "shader objects require BackendFeatures::shader_object".into(),
            ));
        }
        let handle = ShaderObjectHandle(inner.shader_object_handles.alloc());
        inner.backend.create_shader_object(handle, &desc)?;
        inner.shader_objects.insert(handle, desc);
        Ok(handle)
    }

    /// Destroy a shader object and queue its GPU resources for deferred cleanup.
    pub fn destroy_shader_object(&self, handle: ShaderObjectHandle) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        if inner.shader_objects.remove(&handle).is_some() {
            inner
                .deferred_destroys
                .push(DeferredDestroy::ShaderObject(handle));
        }
    }

    /// Per-pass GPU timings from the most recently completed frame.
    ///
    /// Returns per-pass GPU timings in submission order.
    /// Empty before the second frame or on backends without timestamp support.
    /// Track 11a: Bump-allocate `size` bytes from the per-frame transient buffer pool.
    ///
    /// Returns a `TransientAllocation` with `(offset, cpu_ptr, size)` or `None` when the
    /// pool is full. Write data to `mapped_ptr`, then bind the pool's buffer at `offset`
    /// for GPU commands. Valid until the next frame boundary.
    ///
    /// Requires no synchronisation — the pool resets after the previous frame's fence fires.
    pub fn alloc_transient(&self, size: u64, alignment: u64) -> Option<crate::TransientAllocation> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.alloc_transient(size, alignment)
    }

    /// Track 11a: Return the `BufferHandle` for the current frame's transient pool buffer.
    ///
    /// Use together with `alloc_transient` to bind the allocation as a uniform or storage
    /// descriptor: allocate via `alloc_transient`, write data to `mapped_ptr`, then build
    /// a `PushDescriptorBinding::UniformBuffer { buffer: handle, offset: alloc.offset, .. }`.
    ///
    /// Returns `None` when the transient pool is not available for this frame slot.
    pub fn transient_buffer_handle(&self) -> Option<crate::BufferHandle> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.current_transient_buffer_handle()
    }

    /// Track 11b: Return per-queue GPU utilisation for the previous frame.
    ///
    /// Aggregates per-pass timestamp data into per-queue totals using the queue
    /// type recorded at submission time, not name heuristics.
    pub fn gpu_timeline(&self) -> crate::GpuTimeline {
        let timings = self.pass_timings();
        let mut timeline = crate::GpuTimeline::default();
        for report in &timings {
            match report.queue_type {
                crate::QueueType::AsyncCompute => timeline.async_compute_ms += report.gpu_ms,
                crate::QueueType::Transfer | crate::QueueType::Dma => {
                    timeline.transfer_ms += report.gpu_ms
                }
                crate::QueueType::Compute => timeline.compute_ms += report.gpu_ms,
                crate::QueueType::Graphics => timeline.graphics_ms += report.gpu_ms,
            }
            timeline.total_frame_ms += report.gpu_ms;
            timeline.passes.push((report.name.clone(), report.gpu_ms));
        }
        timeline
    }

    pub fn pass_timings(&self) -> Vec<crate::PassTimingReport> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.pass_timings()
    }

    pub fn last_submit_gpu_wait_ms(&self) -> f32 {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.last_submit_gpu_wait_ms()
    }

    /// Draw and dispatch call counts from the most recently submitted frame.
    pub fn frame_draw_dispatch_counts(&self) -> (u32, u32) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.frame_draw_dispatch_counts()
    }

    /// Bytes currently held in transient alias-heap memory (render-graph intermediates).
    pub fn transient_aliased_bytes(&self) -> u64 {
        self.inner.lock().backend.transient_aliased_bytes()
    }

    /// All [`BackendFeature`] variants that are enabled on the current backend.
    ///
    /// Reflects what the device actually supports and has enabled, not what was
    /// requested.  Use this to inspect capability, build feature menus, or compare
    /// before/after a [`Device::rebuild_backend`] call.
    pub fn enabled_features(&self) -> Vec<crate::BackendFeature> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.caps().features.enabled_features()
    }

    /// Returns `true` when a specific [`BackendFeature`] is active on this device.
    pub fn has_feature(&self, feature: crate::BackendFeature) -> bool {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.caps().features.has(feature)
    }

    // ── GFX-4a: Video decode/encode session creation ───────────────────────
    pub fn format_capabilities(&self, format: Format) -> FormatCapabilities {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.format_capabilities(format)
    }

    pub fn native_handle_capabilities(&self) -> NativeHandleCapabilities {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.native_handle_capabilities()
    }

    pub fn raw_capabilities(&self) -> BackendRawCapabilities {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.raw_capabilities()
    }

    pub fn bindless_sampled_image_index(&self, handle: ImageHandle) -> Option<u32> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner
            .lock()
            .bindless_sampled_images
            .get(&handle)
            .copied()
    }

    pub fn bindless_storage_image_index(&self, handle: ImageHandle) -> Option<u32> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner
            .lock()
            .bindless_storage_images
            .get(&handle)
            .copied()
    }

    pub fn bindless_sampler_index(&self, handle: SamplerHandle) -> Option<u32> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().bindless_samplers.get(&handle).copied()
    }

    pub fn bindless_storage_buffer_index(&self, handle: BufferHandle) -> Option<u32> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner
            .lock()
            .bindless_storage_buffers
            .get(&handle)
            .copied()
    }

    /// Track 8a: Validate a bindless sampled-image index in debug builds.
    ///
    /// Panics when `index` ≥ the number of registered sampled images, providing a clear
    /// error instead of a GPU hang. No-op in release builds.
    #[inline]
    pub fn validate_bindless_sampled_image_index(&self, index: u32) {
        #[cfg(debug_assertions)]
        {
            let inner = self.inner.lock();
            let (count, _) = inner.backend.bindless_registered_counts();
            assert!(
                index < count,
                "bindless sampled-image index {index} is out of range (registered: {count})"
            );
        }
        let _ = index;
    }

    /// Track 8a: Validate a bindless sampler index in debug builds.
    #[inline]
    pub fn validate_bindless_sampler_index(&self, index: u32) {
        #[cfg(debug_assertions)]
        {
            let inner = self.inner.lock();
            let (_, count) = inner.backend.bindless_registered_counts();
            assert!(
                index < count,
                "bindless sampler index {index} is out of range (registered: {count})"
            );
        }
        let _ = index;
    }

    pub fn create_image(&self, desc: ImageDesc) -> Result<ImageHandle> {
        desc.validate()?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        validate_sample_count(
            desc.samples,
            inner.backend.caps().max_color_sample_count,
            "image",
        )?;
        let handle = ImageHandle(inner.image_handles.alloc());
        inner.backend.create_image(handle, desc)?;
        if let Some(name) = desc.debug_name {
            inner.backend.set_image_debug_name(handle, name);
        }
        inner.images.insert(handle, desc);
        register_created_image_bindless_indices(&mut inner, handle, desc);
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
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        let handle = ImageHandle(inner.image_handles.alloc());
        unsafe {
            inner.backend.import_external_image(handle, desc)?;
        }
        inner.images.insert(handle, desc.desc);
        register_created_image_bindless_indices(&mut inner, handle, desc.desc);
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
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        validate_sample_count(
            desc.samples,
            inner.backend.caps().max_color_sample_count,
            "transient image",
        )?;
        let handle = ImageHandle(inner.image_handles.alloc());
        inner.backend.create_transient_image(handle, desc)?;
        if let Some(name) = desc.debug_name {
            inner.backend.set_image_debug_name(handle, name);
        }
        inner.images.insert(handle, desc);
        Ok(handle)
    }

    pub fn destroy_image(&self, handle: ImageHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        let _desc = inner.images.remove(&handle).ok_or(Error::InvalidHandle)?;
        inner.bindless_sampled_images.remove(&handle);
        inner.bindless_storage_images.remove(&handle);
        inner.image_states.retain(|key, _| key.image != handle);
        inner.deferred_destroys.push(DeferredDestroy::Image(handle));
        Ok(())
    }

    pub fn image_desc(&self, handle: ImageHandle) -> Result<ImageDesc> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
        inner
            .images
            .get(&handle)
            .copied()
            .ok_or(Error::InvalidHandle)
    }

    pub fn create_buffer(&self, desc: BufferDesc) -> Result<BufferHandle> {
        desc.validate()?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        if desc.usage.contains(BufferUsage::SHADER_DEVICE_ADDRESS)
            && !inner.backend.caps().features.buffer_device_address
        {
            return Err(Error::Unsupported(
                "buffer device address is not supported by this backend".into(),
            ));
        }
        let handle = BufferHandle(inner.buffer_handles.alloc());
        inner.backend.create_buffer(handle, desc)?;
        inner.buffers.insert(handle, desc);
        if desc.usage.contains(BufferUsage::STORAGE) {
            register_bindless_storage_buffer(&mut inner, handle);
        }
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
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        let handle = BufferHandle(inner.buffer_handles.alloc());
        unsafe {
            inner.backend.import_external_buffer(handle, desc)?;
        }
        inner.buffers.insert(handle, desc.desc);
        if desc.desc.usage.contains(BufferUsage::STORAGE) {
            register_bindless_storage_buffer(&mut inner, handle);
        }
        Ok(handle)
    }

    pub fn destroy_buffer(&self, handle: BufferHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        let _desc = inner.buffers.remove(&handle).ok_or(Error::InvalidHandle)?;
        inner.bindless_storage_buffers.remove(&handle);
        inner.buffer_states.retain(|key, _| key.buffer != handle);
        inner
            .deferred_destroys
            .push(DeferredDestroy::Buffer(handle));
        Ok(())
    }

    pub fn write_buffer(&self, handle: BufferHandle, offset: u64, data: &[u8]) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
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
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
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
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
        inner
            .buffers
            .get(&handle)
            .copied()
            .ok_or(Error::InvalidHandle)
    }

    /// Return this buffer's GPU virtual address when buffer-device-address is
    /// enabled and the buffer was created with `SHADER_DEVICE_ADDRESS` usage.
    ///
    /// `Ok(None)` means the app should use a non-address fallback path.
    pub fn buffer_device_address(&self, handle: BufferHandle) -> Result<Option<u64>> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
        let desc = inner.buffers.get(&handle).ok_or(Error::InvalidHandle)?;
        if !desc.usage.contains(BufferUsage::SHADER_DEVICE_ADDRESS)
            || !inner.backend.caps().features.buffer_device_address
        {
            return Ok(None);
        }
        inner.backend.buffer_device_address(handle)
    }

    pub fn create_acceleration_structure(
        &self,
        desc: AccelerationStructureDesc,
    ) -> Result<AccelerationStructureHandle> {
        desc.validate()?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        if !inner.backend.caps().features.ray_tracing && !inner.backend.caps().features.ray_query {
            return Err(Error::Unsupported(
                "acceleration structures require ray_tracing or ray_query backend support".into(),
            ));
        }
        let handle = AccelerationStructureHandle(inner.acceleration_structure_handles.alloc());
        inner.backend.create_acceleration_structure(handle, desc)?;
        inner.acceleration_structures.insert(handle, desc);
        Ok(handle)
    }

    pub fn destroy_acceleration_structure(
        &self,
        handle: AccelerationStructureHandle,
    ) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        let _desc = inner
            .acceleration_structures
            .remove(&handle)
            .ok_or(Error::InvalidHandle)?;
        inner
            .deferred_destroys
            .push(DeferredDestroy::AccelerationStructure(handle));
        Ok(())
    }

    pub fn acceleration_structure_desc(
        &self,
        handle: AccelerationStructureHandle,
    ) -> Result<AccelerationStructureDesc> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
        inner
            .acceleration_structures
            .get(&handle)
            .copied()
            .ok_or(Error::InvalidHandle)
    }

    pub fn acceleration_structure_device_address(
        &self,
        handle: AccelerationStructureHandle,
    ) -> Result<u64> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
        validate_as_handle(&inner, handle)?;
        inner.backend.acceleration_structure_device_address(handle)
    }

    pub fn blas_build_sizes(
        &self,
        desc: &BlasBuildDesc,
    ) -> Result<AccelerationStructureBuildSizes> {
        validate_blas_build_desc(desc)?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
        if desc.mode == AccelerationStructureBuildMode::Compact {
            let src = desc.src.ok_or_else(|| {
                Error::InvalidInput("BLAS compaction size query requires a source".into())
            })?;
            return compact_acceleration_structure_build_sizes(
                acceleration_structure_desc(&inner, src)?,
                crate::AccelerationStructureKind::BottomLevel,
            );
        }
        if desc.dst != Default::default() {
            validate_as_handle(&inner, desc.dst)?;
        }
        if let Some(src) = desc.src {
            validate_as_handle(&inner, src)?;
        }
        for geometry in &desc.geometries {
            validate_buffer_handle(&inner, geometry.vertex_buffer)?;
            if let Some(index_buffer) = geometry.index_buffer {
                validate_buffer_handle(&inner, index_buffer)?;
            }
            if let Some(transform_buffer) = geometry.transform_buffer {
                validate_buffer_handle(&inner, transform_buffer)?;
            }
        }
        if let Some(scratch) = desc.scratch_buffer {
            validate_buffer_handle(&inner, scratch)?;
        }
        inner.backend.blas_build_sizes(desc)
    }

    pub fn tlas_build_sizes(
        &self,
        desc: &TlasBuildDesc,
    ) -> Result<AccelerationStructureBuildSizes> {
        validate_tlas_build_desc(desc)?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
        if desc.mode == AccelerationStructureBuildMode::Compact {
            let src = desc.src.ok_or_else(|| {
                Error::InvalidInput("TLAS compaction size query requires a source".into())
            })?;
            return compact_acceleration_structure_build_sizes(
                acceleration_structure_desc(&inner, src)?,
                crate::AccelerationStructureKind::TopLevel,
            );
        }
        if desc.dst != Default::default() {
            validate_as_handle(&inner, desc.dst)?;
        }
        if let Some(src) = desc.src {
            validate_as_handle(&inner, src)?;
        }
        validate_buffer_handle(&inner, desc.instance_buffer)?;
        if let Some(scratch) = desc.scratch_buffer {
            validate_buffer_handle(&inner, scratch)?;
        }
        inner.backend.tlas_build_sizes(desc)
    }

    pub fn create_sampler(&self, desc: SamplerDesc) -> Result<SamplerHandle> {
        desc.validate()?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        let handle = SamplerHandle(inner.sampler_handles.alloc());
        inner.backend.create_sampler(handle, desc)?;
        inner.samplers.insert(handle, desc);
        register_bindless_sampler(&mut inner, handle);
        Ok(handle)
    }

    pub fn destroy_sampler(&self, handle: SamplerHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        let _desc = inner.samplers.remove(&handle).ok_or(Error::InvalidHandle)?;
        inner.bindless_samplers.remove(&handle);
        inner
            .deferred_destroys
            .push(DeferredDestroy::Sampler(handle));
        Ok(())
    }

    pub fn sampler_desc(&self, handle: SamplerHandle) -> Result<SamplerDesc> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
        inner
            .samplers
            .get(&handle)
            .copied()
            .ok_or(Error::InvalidHandle)
    }

    pub fn create_shader(&self, desc: ShaderDesc) -> Result<ShaderHandle> {
        desc.validate()?;
        // Phase 1: acquire target and check in-process compilation cache.
        let (target, cache_key, cached) = {
            //panic allowed, reason = "poisoned mutex is unrecoverable"
            let inner = self.inner.lock();
            let target = inner.backend.preferred_shader_ir();
            let key = shader_compile_cache_key(&desc, target);
            let cached = key.and_then(|k| inner.shader_compile_cache.get(&k).cloned());
            (target, key, cached)
        };

        // Phase 2: compile without holding the lock (Slang compilation is expensive).
        let (compiled_desc, reflection) = if let Some(hit) = cached {
            hit
        } else {
            let result = crate::slang::compile_and_reflect(&desc, target)?;
            // Store in cache if this source type is cache-eligible.
            if let Some(key) = cache_key {
                //panic allowed, reason = "poisoned mutex is unrecoverable"
                let mut inner = self.inner.lock();
                // Use or_insert_with to avoid overwriting a concurrent insertion.
                inner
                    .shader_compile_cache
                    .entry(key)
                    .or_insert_with(|| result.clone());
            }
            result
        };

        // Phase 3: register the compiled shader with the backend.
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        let handle = ShaderHandle(inner.shader_handles.alloc());
        inner.backend.create_shader(handle, &compiled_desc)?;
        inner.shader_reflections.insert(handle, reflection);
        inner.shaders.insert(handle, compiled_desc);
        Ok(handle)
    }

    pub fn shader_reflection(&self, handle: ShaderHandle) -> Result<ShaderReflection> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
        inner
            .shader_reflections
            .get(&handle)
            .cloned()
            .ok_or(Error::InvalidHandle)
    }

    pub fn destroy_shader(&self, handle: ShaderHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
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
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        let handle = PipelineLayoutHandle(inner.pipeline_layout_handles.alloc());
        inner.backend.create_pipeline_layout(handle, &layout)?;
        inner.pipeline_layouts.insert(handle, layout);
        Ok(handle)
    }

    pub fn reflected_compute_pipeline_layout(
        &self,
        shader: ShaderHandle,
    ) -> Result<CanonicalPipelineLayout> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
        if !inner.shaders.contains_key(&shader) {
            return Err(Error::InvalidHandle);
        }
        Ok(inner
            .shader_reflections
            .get(&shader)
            .map(|reflection| reflection.layout.clone())
            .unwrap_or_default())
    }

    pub fn reflected_graphics_pipeline_layout(
        &self,
        vertex_shader: ShaderHandle,
        fragment_shader: Option<ShaderHandle>,
    ) -> Result<CanonicalPipelineLayout> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
        if !inner.shaders.contains_key(&vertex_shader)
            || fragment_shader.is_some_and(|shader| !inner.shaders.contains_key(&shader))
        {
            return Err(Error::InvalidHandle);
        }
        Ok(merge_shader_layouts(
            &inner.shader_reflections,
            [Some(vertex_shader), fragment_shader],
        ))
    }

    pub fn reflected_graphics_pipeline_reflection(
        &self,
        vertex_shader: ShaderHandle,
        fragment_shader: Option<ShaderHandle>,
    ) -> Result<ShaderReflection> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
        if !inner.shaders.contains_key(&vertex_shader)
            || fragment_shader.is_some_and(|shader| !inner.shaders.contains_key(&shader))
        {
            return Err(Error::InvalidHandle);
        }
        Ok(merge_shader_reflection(
            &inner.shader_reflections,
            [Some(vertex_shader), fragment_shader],
        ))
    }

    pub fn destroy_pipeline_layout(&self, handle: PipelineLayoutHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
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
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        inner.validate_bind_group_desc(&desc)?;
        let handle = BindGroupHandle(inner.bind_group_handles.alloc());
        inner.backend.create_bind_group(handle, &desc)?;
        inner.bind_groups.insert(handle, desc);
        Ok(handle)
    }

    pub fn destroy_bind_group(&self, handle: BindGroupHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
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
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
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
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        validate_sample_count(
            desc.samples,
            inner.backend.caps().max_color_sample_count,
            "graphics pipeline",
        )?;
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
                let layout = merge_shader_layouts(
                    &inner.shader_reflections,
                    [Some(desc.vertex_shader), desc.fragment_shader],
                );
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

    pub fn create_ray_tracing_pipeline(
        &self,
        desc: RayTracingPipelineDesc,
    ) -> Result<PipelineHandle> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        if !inner.backend.caps().features.ray_tracing {
            return Err(Error::Unsupported(
                "ray tracing pipelines require BackendFeatures::ray_tracing".into(),
            ));
        }
        let layout_handle = match desc.layout {
            Some(h) => {
                if !inner.pipeline_layouts.contains_key(&h) {
                    return Err(Error::InvalidHandle);
                }
                h
            }
            None => {
                return Err(Error::InvalidInput(
                    "ray tracing pipeline requires an explicit pipeline layout".into(),
                ));
            }
        };
        let resolved = RayTracingPipelineDesc {
            layout: Some(layout_handle),
            ..desc
        };
        let group_count = resolved.groups.len() as u32;
        let handle = PipelineHandle(inner.pipeline_handles.alloc());
        inner
            .backend
            .create_ray_tracing_pipeline(handle, &resolved)?;
        // RT pipelines do not auto-create a layout, so owned_layout is always None here
        // (the layout was externally supplied).
        inner.pipelines.insert(
            handle,
            PipelineDesc::RayTracing {
                group_count,
                owned_layout: None,
            },
        );
        Ok(handle)
    }

    pub fn create_shader_binding_table(
        &self,
        desc: ShaderBindingTableDesc,
    ) -> Result<ShaderBindingTable> {
        let (props, handles) = {
            //panic allowed, reason = "poisoned mutex is unrecoverable"
            let inner = self.inner.lock();
            if !inner.backend.caps().features.ray_tracing {
                return Err(Error::Unsupported(
                    "shader binding tables require BackendFeatures::ray_tracing".into(),
                ));
            }
            let group_count = match inner.pipelines.get(&desc.pipeline) {
                Some(PipelineDesc::RayTracing { group_count, .. }) => *group_count,
                Some(_) => {
                    return Err(Error::InvalidInput(
                        "shader binding table requires a ray-tracing pipeline".into(),
                    ));
                }
                None => return Err(Error::InvalidHandle),
            };
            validate_sbt_groups(&desc, group_count)?;
            let props = inner.backend.shader_binding_table_properties()?;
            validate_sbt_properties(props)?;
            let handles = collect_sbt_group_handles(&*inner.backend, &desc, props)?;
            (props, handles)
        };

        let layout = SbtLayout::new(&desc, props)?;
        let mut data = vec![0u8; layout.total_size as usize];
        write_sbt_region(
            &mut data,
            layout.raygen_offset,
            layout.stride,
            props.shader_group_handle_size,
            &handles.raygen,
        );
        write_sbt_region(
            &mut data,
            layout.miss_offset,
            layout.stride,
            props.shader_group_handle_size,
            &handles.miss,
        );
        write_sbt_region(
            &mut data,
            layout.hit_offset,
            layout.stride,
            props.shader_group_handle_size,
            &handles.hit,
        );
        write_sbt_region(
            &mut data,
            layout.callable_offset,
            layout.stride,
            props.shader_group_handle_size,
            &handles.callable,
        );

        let buffer = self.create_buffer(BufferDesc {
            size: layout.total_size,
            usage: BufferUsage::COPY_SRC
                | BufferUsage::STORAGE
                | BufferUsage::SHADER_BINDING_TABLE
                | BufferUsage::SHADER_DEVICE_ADDRESS,
        })?;
        if let Err(error) = self.write_buffer(buffer, 0, &data) {
            let _ = self.destroy_buffer(buffer);
            return Err(error);
        }

        Ok(ShaderBindingTable {
            raygen: ShaderBindingTableRegion {
                buffer,
                offset: layout.raygen_offset,
                stride: layout.stride,
                size: layout.raygen_size,
            },
            miss: ShaderBindingTableRegion {
                buffer,
                offset: layout.miss_offset,
                stride: layout.stride,
                size: layout.miss_size,
            },
            hit: ShaderBindingTableRegion {
                buffer,
                offset: layout.hit_offset,
                stride: layout.stride,
                size: layout.hit_size,
            },
            callable: (!desc.callable_groups.is_empty()).then_some(ShaderBindingTableRegion {
                buffer,
                offset: layout.callable_offset,
                stride: layout.stride,
                size: layout.callable_size,
            }),
        })
    }

    pub fn set_image_debug_name(&self, handle: ImageHandle, name: &str) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
        if !inner.images.contains_key(&handle) {
            return Err(Error::InvalidHandle);
        }
        inner.backend.set_image_debug_name(handle, name);
        Ok(())
    }

    pub fn set_buffer_debug_name(&self, handle: BufferHandle, name: &str) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
        if !inner.buffers.contains_key(&handle) {
            return Err(Error::InvalidHandle);
        }
        inner.backend.set_buffer_debug_name(handle, name);
        Ok(())
    }

    pub fn set_pipeline_debug_name(&self, handle: PipelineHandle, name: &str) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
        if !inner.pipelines.contains_key(&handle) {
            return Err(Error::InvalidHandle);
        }
        inner.backend.set_pipeline_debug_name(handle, name);
        Ok(())
    }

    pub fn supported_gpu_capture_tools(&self) -> Vec<GpuCaptureTool> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.supported_gpu_capture_tools()
    }

    pub fn begin_gpu_capture(&self, desc: &GpuCaptureDesc) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.begin_gpu_capture(desc)
    }

    pub fn end_gpu_capture(&self, tool: GpuCaptureTool) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.end_gpu_capture(tool)
    }

    pub fn destroy_pipeline(&self, handle: PipelineHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
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
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
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
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
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
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
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
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
        inner
            .surfaces
            .get(&handle)
            .map(|surface| surface.info)
            .ok_or(Error::InvalidHandle)
    }

    pub fn drain_surface_events(&self, handle: SurfaceHandle) -> Result<Vec<SurfaceEvent>> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
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
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
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
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
        if !inner.surfaces.contains_key(&surface) {
            return Err(Error::InvalidHandle);
        }
        inner.backend.present_surface(surface)
    }

    pub fn destroy_surface(&self, handle: SurfaceHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        let _surface = inner.surfaces.remove(&handle).ok_or(Error::InvalidHandle)?;
        inner.backend.destroy_surface(handle)
    }

    pub fn query_surface_capabilities(&self, handle: SurfaceHandle) -> Result<SurfaceCapabilities> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
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
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
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
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.wait_submission(token)
    }

    pub fn wait_idle(&self) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.wait_idle()
    }

    // ── GFX-4: Video encode/decode ────────────────────────────────────────────

    pub fn create_video_session(&self, desc: VideoSessionDesc) -> Result<VideoSessionHandle> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        if !inner.backend.caps().features.video_queue {
            return Err(Error::Unsupported(
                "video sessions require BackendFeatures::video_queue".into(),
            ));
        }
        let handle = VideoSessionHandle(inner.video_session_handles.alloc());
        inner.backend.create_video_session(handle, desc)?;
        inner.video_sessions.insert(handle, desc);
        Ok(handle)
    }

    pub fn destroy_video_session(&self, handle: VideoSessionHandle) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        if inner.video_sessions.remove(&handle).is_some() {
            inner
                .deferred_destroys
                .push(DeferredDestroy::VideoSession(handle));
        }
    }

    /// GFX-4a: Create a managed video decode session with DPB pre-allocation.
    ///
    /// Allocates `max_dpb_slots` YCbCr reference images and one output image.
    /// The returned `VideoDecodeSession` owns these handles; use `output_image()` to
    /// get the decoded frame handle for import into the render graph.
    /// Requires `BackendFeatures::video_decode_h264` (or h265 etc. for the chosen codec).
    pub fn create_video_decode_session(
        &self,
        codec: crate::VideoCodec,
        width: u32,
        height: u32,
        max_dpb_slots: u32,
    ) -> Result<crate::VideoDecodeSession> {
        use crate::{Extent3d, Format, ImageDesc, ImageUsage, VideoSessionDesc, VideoSessionKind};
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        // Create the underlying VkVideoSessionKHR.
        let session_handle = VideoSessionHandle(inner.video_session_handles.alloc());
        let session_desc = VideoSessionDesc {
            kind: VideoSessionKind::Decode,
            codec,
            width,
            height,
            max_dpb_slots,
        };
        inner.video_sessions.insert(session_handle, session_desc);
        inner
            .backend
            .create_video_session(session_handle, session_desc)?;

        // Allocate the output image (decoded frame destination).
        let output_handle = inner.image_handles.alloc();
        let output_image = crate::ImageHandle(output_handle);
        let output_desc = ImageDesc {
            dimension: crate::ImageDimension::D2,
            extent: Extent3d {
                width,
                height,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::G8_B8R8_2PLANE_420_UNORM,
            usage: ImageUsage::VIDEO_DECODE_DST | ImageUsage::SAMPLED,
            ..ImageDesc::new()
        };
        inner.backend.create_image(output_image, output_desc)?;

        // Allocate DPB reference images.
        let mut dpb_images = Vec::with_capacity(max_dpb_slots as usize);
        for _ in 0..max_dpb_slots {
            let dpb_handle = inner.image_handles.alloc();
            let dpb_image = crate::ImageHandle(dpb_handle);
            let dpb_desc = ImageDesc {
                dimension: crate::ImageDimension::D2,
                extent: Extent3d {
                    width,
                    height,
                    depth: 1,
                },
                mip_levels: 1,
                layers: 1,
                samples: 1,
                format: Format::G8_B8R8_2PLANE_420_UNORM,
                usage: ImageUsage::VIDEO_DECODE_DPB,
                ..ImageDesc::new()
            };
            inner.backend.create_image(dpb_image, dpb_desc)?;
            dpb_images.push(dpb_image);
        }

        Ok(crate::VideoDecodeSession {
            session_handle,
            dpb_images,
            output_image,
            width,
            height,
            codec,
        })
    }

    /// GFX-4b: Create a managed video encode session with an internal output buffer.
    ///
    /// Allocates an output buffer large enough to hold the encoded bitstream.
    /// After encoding via `PassWork::EncodeVideoFrame`, call `read_encode_bitstream` to
    /// retrieve the compressed output.
    pub fn create_video_encode_session(
        &self,
        config: crate::VideoEncodeConfig,
    ) -> Result<crate::VideoEncodeSession> {
        use crate::{BufferDesc, BufferUsage, VideoSessionDesc, VideoSessionKind};
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        // Heuristic: allocate 2 MiB per megapixel per second at the target bitrate.
        let w = config.width as u64;
        let h = config.height as u64;
        let max_bitstream_bytes = ((w * h * 4).max(1024 * 1024)) * 2;

        let session_handle = VideoSessionHandle(inner.video_session_handles.alloc());
        let session_desc = VideoSessionDesc {
            kind: VideoSessionKind::Encode,
            codec: config.codec,
            width: config.width,
            height: config.height,
            max_dpb_slots: 2,
        };
        inner.video_sessions.insert(session_handle, session_desc);
        inner
            .backend
            .create_video_session(session_handle, session_desc)?;

        // Allocate the output bitstream buffer via the exportable buffer path which
        // gives us a dedicated VkDeviceMemory we can map for CPU readback.
        let buf_handle = inner.buffer_handles.alloc();
        let output_buffer = crate::BufferHandle(buf_handle);
        let buf_desc = BufferDesc {
            size: max_bitstream_bytes,
            usage: BufferUsage::VIDEO_ENCODE_DST | BufferUsage::COPY_SRC,
        };
        // Use create_buffer for registry tracking, then create_exportable_buffer
        // to get a host-visible dedicated allocation we can map.
        inner
            .backend
            .create_video_encode_output_buffer(output_buffer, buf_desc)?;

        Ok(crate::VideoEncodeSession {
            session_handle,
            output_buffer,
            max_bitstream_bytes,
            config,
        })
    }

    /// GFX-4b: Copy the encoded bitstream from the session's output buffer to a `Vec<u8>`.
    ///
    /// Call this after a frame has been encoded via `PassWork::EncodeVideoFrame` and the
    /// command buffer has finished executing. Returns the raw compressed bytes.
    /// Track 8e: Pre-compile pipeline library objects for common vertex and attachment formats.
    ///
    /// Triggers creation of the material-independent VertexInput and FragmentOutput pipeline
    /// library objects used by GFX-2c. Call during loading screens to front-load compilation
    /// time. Returns a report of what was compiled.
    /// No-op when `BackendFeatures::graphics_pipeline_library` is false.
    pub fn pso_pre_warm(&self) -> crate::PsoWarmupReport {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.pso_pre_warm()
    }

    pub fn read_encode_bitstream(&self, session: &crate::VideoEncodeSession) -> Result<Vec<u8>> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let inner = self.inner.lock();
        inner
            .backend
            .read_encode_bitstream(session.output_buffer, session.max_bitstream_bytes)
    }

    // ── GFX-6a: Device-generated commands ────────────────────────────────────

    pub fn create_indirect_command_layout(
        &self,
        desc: &crate::IndirectCommandLayoutDesc,
    ) -> Result<crate::IndirectCommandLayoutHandle> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        if !inner.backend.caps().features.device_generated_commands_nv
            && !inner.backend.caps().features.device_generated_commands
        {
            return Err(Error::Unsupported(
                "indirect command layouts require VK_EXT_device_generated_commands or VK_NV_device_generated_commands".into(),
            ));
        }
        let handle =
            crate::IndirectCommandLayoutHandle(inner.indirect_command_layout_handles.alloc());
        inner.backend.create_indirect_command_layout(handle, desc)?;
        inner.indirect_command_layouts.insert(handle, desc.clone());
        Ok(handle)
    }

    pub fn destroy_indirect_command_layout(&self, handle: crate::IndirectCommandLayoutHandle) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        if inner.indirect_command_layouts.remove(&handle).is_some() {
            inner
                .deferred_destroys
                .push(DeferredDestroy::IndirectCommandLayout(handle));
        }
    }

    // ── GFX-6e: Optical flow ─────────────────────────────────────────────────

    pub fn create_optical_flow_session(
        &self,
        desc: &crate::OpticalFlowSessionDesc,
    ) -> Result<crate::OpticalFlowSessionHandle> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        if !inner.backend.caps().features.optical_flow_nv {
            return Err(Error::Unsupported(
                "optical flow sessions require BackendFeatures::optical_flow_nv".into(),
            ));
        }
        let handle = crate::OpticalFlowSessionHandle(inner.optical_flow_session_handles.alloc());
        inner.backend.create_optical_flow_session(handle, desc)?;
        inner.optical_flow_sessions.insert(handle, *desc);
        Ok(handle)
    }

    pub fn destroy_optical_flow_session(&self, handle: crate::OpticalFlowSessionHandle) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        if inner.optical_flow_sessions.remove(&handle).is_some() {
            inner
                .deferred_destroys
                .push(DeferredDestroy::OpticalFlowSession(handle));
        }
    }

    // ── GFX-1h: Host image copy ───────────────────────────────────────────────

    /// Copy CPU memory directly into a GPU image without a staging buffer or command buffer.
    ///
    /// Requires `BackendFeatures::host_image_copy`. Useful on integrated/unified memory hardware
    /// to eliminate the staging buffer round-trip.
    pub fn copy_memory_to_image(
        &self,
        handle: crate::ImageHandle,
        mip: u32,
        layer: u32,
        data: &[u8],
    ) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner
            .lock()
            .backend
            .copy_memory_to_image(handle, mip, layer, data)
    }

    /// Transition a GPU image layout from the CPU side without recording a command buffer.
    ///
    /// Requires `BackendFeatures::host_image_copy`. Use after `copy_memory_to_image` to
    /// set the image to the layout required by the first shader read.
    pub fn transition_image_layout_cpu(
        &self,
        handle: crate::ImageHandle,
        new_layout: crate::RgState,
    ) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner
            .lock()
            .backend
            .transition_image_layout_cpu(handle, new_layout)
    }

    // ── GFX-6b: Latency reduction ─────────────────────────────────────────────

    // ── GFX-5a: External memory exports ──────────────────────────────────────

    /// Create a buffer with exportable GPU memory (`VK_EXTERNAL_MEMORY_HANDLE_TYPE_OPAQUE_FD_BIT`).
    ///
    /// Call `export_buffer_fd` on the returned handle to get an opaque fd for cross-process
    /// or cross-API sharing. Requires `BackendFeatures::external_memory_fd`.
    pub fn create_exportable_buffer(&self, desc: crate::BufferDesc) -> Result<crate::BufferHandle> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        if !inner.backend.caps().features.external_memory_fd {
            return Err(Error::Unsupported(
                "exportable buffers require BackendFeatures::external_memory_fd".into(),
            ));
        }
        let handle = crate::BufferHandle(inner.buffer_handles.alloc());
        inner.backend.create_exportable_buffer(handle, desc)?;
        inner.buffers.insert(handle, desc);
        Ok(handle)
    }

    /// Create an image with exportable GPU memory (`VK_EXTERNAL_MEMORY_HANDLE_TYPE_OPAQUE_FD_BIT`).
    ///
    /// Requires `BackendFeatures::external_memory_fd`.
    pub fn create_exportable_image(&self, desc: crate::ImageDesc) -> Result<crate::ImageHandle> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        if !inner.backend.caps().features.external_memory_fd {
            return Err(Error::Unsupported(
                "exportable images require BackendFeatures::external_memory_fd".into(),
            ));
        }
        let handle = crate::ImageHandle(inner.image_handles.alloc());
        inner.backend.create_exportable_image(handle, desc)?;
        inner.images.insert(handle, desc);
        Ok(handle)
    }

    /// Export a Linux fd for a buffer created with `create_exportable_buffer`.
    ///
    /// The caller owns the returned fd and must close it with `libc::close`.
    pub fn export_buffer_fd(&self, handle: crate::BufferHandle) -> Result<i32> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.export_buffer_fd(handle)
    }

    /// Export a Linux fd for an image created with `create_exportable_image`.
    pub fn export_image_fd(&self, handle: crate::ImageHandle) -> Result<i32> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.export_image_fd(handle)
    }

    /// Import a CPU host pointer as a zero-copy GPU buffer.
    ///
    /// The `ptr` must remain valid for the buffer's lifetime. Requires
    /// `BackendFeatures::external_memory_host`. The pointer alignment must be at
    /// least `VkPhysicalDeviceExternalMemoryHostPropertiesEXT::minImportedHostPointerAlignment`.
    pub fn import_host_memory(
        &self,
        ptr: *const u8,
        size: usize,
        usage: crate::BufferUsage,
    ) -> Result<crate::BufferHandle> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        if !inner.backend.caps().features.external_memory_host {
            return Err(Error::Unsupported(
                "host memory import requires BackendFeatures::external_memory_host".into(),
            ));
        }
        let desc = crate::BufferDesc {
            size: size as u64,
            usage,
        };
        let handle = crate::BufferHandle(inner.buffer_handles.alloc());
        inner.backend.import_host_memory(handle, ptr, size)?;
        inner.buffers.insert(handle, desc);
        Ok(handle)
    }

    // ── GFX-5b: External semaphore exports ────────────────────────────────────

    /// Create a binary semaphore with an exportable fd (`VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_OPAQUE_FD`).
    ///
    /// Call `export_semaphore_fd` to get the fd. Requires `BackendFeatures::external_semaphore_fd`.
    pub fn create_exportable_semaphore(&self) -> Result<crate::SemaphoreHandle> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        if !inner.backend.caps().features.external_semaphore_fd {
            return Err(Error::Unsupported(
                "exportable semaphores require BackendFeatures::external_semaphore_fd".into(),
            ));
        }
        let handle = crate::SemaphoreHandle(inner.indirect_command_layout_handles.alloc());
        inner.backend.create_exportable_semaphore(handle)?;
        Ok(handle)
    }

    /// Export a POSIX fd from an exportable semaphore. The caller must `close(fd)` when done.
    pub fn export_semaphore_fd(&self, handle: crate::SemaphoreHandle) -> Result<i32> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.export_semaphore_fd(handle)
    }

    /// Import a POSIX fd into an exportable semaphore handle for cross-process signaling.
    pub fn import_semaphore_fd(&self, handle: crate::SemaphoreHandle, fd: i32) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.import_semaphore_fd(handle, fd)
    }

    /// GFX-5b: Create an exportable fence that can be shared cross-process via a file descriptor.
    ///
    /// Requires `BackendFeatures::external_fence_fd`.
    pub fn create_exportable_fence(&self) -> Result<crate::FenceHandle> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.inner.lock();
        let handle = crate::FenceHandle(inner.fence_handles.alloc());
        inner.backend.create_exportable_fence(handle)?;
        Ok(handle)
    }

    /// GFX-5b: Export a fence as an opaque POSIX file descriptor.
    ///
    /// The receiver can import the fd via `import_fence_fd`. Requires `BackendFeatures::external_fence_fd`.
    pub fn export_fence_fd(&self, handle: crate::FenceHandle) -> Result<i32> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.export_fence_fd(handle)
    }

    /// GFX-5b: Import a fence from a POSIX file descriptor obtained via `export_fence_fd`.
    ///
    /// Requires `BackendFeatures::external_fence_fd`.
    pub fn import_fence_fd(&self, handle: crate::FenceHandle, fd: i32) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.import_fence_fd(handle, fd)
    }

    pub fn set_reflex_mode(&self, mode: ReflexMode) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.set_reflex_mode(mode)
    }

    pub fn set_anti_lag_mode(&self, mode: AntiLagMode) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.set_anti_lag_mode(mode)
    }

    pub fn latency_mode(&self) -> Option<LatencyMode> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.latency_mode()
    }

    /// Number of shader cores, compute units, or SMs reported by the GPU driver.
    ///
    /// AMD: total compute units (`shader_engine_count × arrays_per_engine × CUs_per_array`)
    /// via `VK_AMD_shader_core_properties`.
    ///
    /// NVIDIA: SM count via `VK_NV_shader_sm_builtins`.
    ///
    /// Returns `None` when neither vendor extension is available. Use this to
    /// tune compute workgroup counts and tile sizes to the actual hardware width.
    pub fn shader_core_count(&self) -> Option<u32> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.caps().shader_core_count
    }

    /// List all cooperative matrix multiply-accumulate configurations supported by the GPU.
    ///
    /// Returns empty when `BackendFeatures::cooperative_matrix` is false.
    pub fn enumerate_cooperative_matrix_properties(&self) -> Vec<crate::CoopMatrixProperty> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner
            .lock()
            .backend
            .enumerate_cooperative_matrix_properties()
    }

    /// List hardware performance counters available on this device.
    ///
    /// Requires `BackendFeatures::performance_query`. Returns empty when unavailable.
    pub fn enumerate_performance_counters(&self) -> Vec<crate::PerfCounter> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.enumerate_performance_counters()
    }

    /// Return per-stage compiled shader statistics for a pipeline.
    ///
    /// Requires `BackendFeatures::pipeline_executable_properties`. Returns empty otherwise.
    pub fn pipeline_executable_stats(
        &self,
        pipeline: crate::PipelineHandle,
    ) -> Vec<crate::ExecutableStat> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner
            .lock()
            .backend
            .pipeline_executable_stats(pipeline)
    }

    /// Return per-stage compiled shader statistics for a pipeline via `VK_AMD_shader_info`.
    ///
    /// Returns register counts (VGPR/SGPR), LDS usage, and scratch memory per stage.
    /// Requires `BackendFeatures::shader_info_amd`. Returns empty on other backends.
    pub fn pipeline_shader_stats_amd(
        &self,
        pipeline: crate::PipelineHandle,
    ) -> Vec<crate::AmdShaderStageStats> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner
            .lock()
            .backend
            .pipeline_shader_stats_amd(pipeline)
    }

    /// Block the calling thread until the driver signals the optimal frame-start time.
    ///
    // ── GFX-1c: Timeline semaphore cross-queue coordination ────────────────────

    /// Create a timeline semaphore with the given initial value.
    ///
    /// Requires `BackendFeatures::timeline_semaphores`.
    pub fn create_timeline_semaphore(&self, initial_value: u64) -> Result<crate::SemaphoreHandle> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner
            .lock()
            .backend
            .create_timeline_semaphore(initial_value)
    }

    /// Block until the timeline semaphore reaches `value` or the timeout (nanoseconds) expires.
    pub fn wait_for_timeline(
        &self,
        semaphore: crate::SemaphoreHandle,
        value: u64,
        timeout_ns: u64,
    ) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner
            .lock()
            .backend
            .wait_for_timeline(semaphore, value, timeout_ns)
    }

    /// Signal a timeline semaphore from the CPU to `value`.
    pub fn signal_timeline(&self, semaphore: crate::SemaphoreHandle, value: u64) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.signal_timeline(semaphore, value)
    }

    /// Destroy a timeline semaphore.
    pub fn destroy_timeline_semaphore(&self, semaphore: crate::SemaphoreHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner
            .lock()
            .backend
            .destroy_timeline_semaphore(semaphore)
    }

    /// Call once per frame on the render thread, before input sampling, when NVIDIA Reflex
    /// is active. Has no effect (returns `Ok(())`) when Reflex is unavailable.
    ///
    /// Requires `BackendFeatures::reflex` and `BackendFeatures::timeline_semaphores`.
    pub fn latency_sleep(&self, surface: SurfaceHandle) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.latency_sleep(surface)
    }

    /// Notify AMD Anti-Lag that a new frame has started; call before input sampling.
    ///
    /// Has no effect when `BackendFeatures::anti_lag` is false. No feature check needed.
    pub fn anti_lag_frame_start(&self) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner.lock().backend.anti_lag_frame_start()
    }

    /// Set SMPTE ST 2086 / CTA 861.3 HDR mastering display metadata on a
    /// surface.  Has no effect when `BackendFeatures::hdr_output` is false or
    /// the swapchain is not in an HDR color space.
    pub fn set_surface_hdr_metadata(
        &self,
        surface: SurfaceHandle,
        metadata: HdrMetadata,
    ) -> Result<()> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.inner
            .lock()
            .backend
            .set_surface_hdr_metadata(surface, metadata)
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
            ResourceBinding::ImageView { image, .. } if self.images.contains_key(&image) => Ok(()),
            ResourceBinding::Buffer(handle) if self.buffers.contains_key(&handle) => Ok(()),
            ResourceBinding::Sampler(handle) if self.samplers.contains_key(&handle) => Ok(()),
            ResourceBinding::AccelerationStructure(handle)
                if self.acceleration_structures.contains_key(&handle) =>
            {
                Ok(())
            }
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
            ResourceBinding::Image(_) | ResourceBinding::ImageView { .. }
        ) | (
            BindingKind::UniformBuffer | BindingKind::StorageBuffer,
            ResourceBinding::Buffer(_)
        ) | (BindingKind::Sampler, ResourceBinding::Sampler(_))
            | (
                BindingKind::AccelerationStructure,
                ResourceBinding::AccelerationStructure(_),
            )
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
        ResourceBinding::ImageView { .. } => "image view",
        ResourceBinding::Buffer(_) => "buffer",
        ResourceBinding::Sampler(_) => "sampler",
        ResourceBinding::AccelerationStructure(_) => "acceleration structure",
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
    RayTracing {
        group_count: u32,
        owned_layout: Option<PipelineLayoutHandle>,
    },
}

impl PipelineDesc {
    fn owned_layout(&self) -> Option<PipelineLayoutHandle> {
        match self {
            PipelineDesc::Compute { owned_layout, .. } => *owned_layout,
            PipelineDesc::Graphics { owned_layout, .. } => *owned_layout,
            PipelineDesc::RayTracing { owned_layout, .. } => *owned_layout,
        }
    }
}

/// Compute a stable cache key for a shader compilation request.
///
/// Returns `None` for sources that must not be cached (file sources, pre-compiled
/// IR) so that hot reload always reads fresh disk content.
fn shader_compile_cache_key(desc: &ShaderDesc, target: crate::ShaderTarget) -> Option<u64> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    match &desc.source {
        ShaderSource::Inline(s) => s.hash(&mut h),
        ShaderSource::MemoryUtf8(s) => s.hash(&mut h),
        ShaderSource::MemoryBytes(b) => b.hash(&mut h),
        // File/path sources may change during hot reload — exclude from cache.
        // Pre-compiled IR has no compilation cost to begin with.
        ShaderSource::File(_)
        | ShaderSource::FilePath(_)
        | ShaderSource::VirtualAssetPath(_)
        | ShaderSource::Spirv(_)
        | ShaderSource::Dxil(_)
        | ShaderSource::Msl(_) => return None,
    }
    desc.entry_point.hash(&mut h);
    (desc.stage as u8).hash(&mut h);
    (target as u8).hash(&mut h);
    Some(h.finish())
}

fn merge_shader_reflection<const N: usize>(
    reflections: &HashMap<ShaderHandle, ShaderReflection>,
    shaders: [Option<ShaderHandle>; N],
) -> ShaderReflection {
    let layout = merge_shader_layouts(reflections, shaders);
    let mut entry_points = Vec::new();
    let mut parameters: Vec<crate::ShaderParameterReflection> = Vec::new();
    for shader in shaders.into_iter().flatten() {
        if let Some(reflection) = reflections.get(&shader) {
            for entry_point in &reflection.entry_points {
                if !entry_points.contains(entry_point) {
                    entry_points.push(entry_point.clone());
                }
            }
            for parameter in &reflection.parameters {
                if let Some(existing) = parameters.iter_mut().find(|existing| {
                    existing.name == parameter.name
                        && existing.set == parameter.set
                        && existing.binding == parameter.binding
                        && existing.kind == parameter.kind
                }) {
                    existing.stage_mask |= parameter.stage_mask;
                } else {
                    parameters.push(parameter.clone());
                }
            }
        }
    }
    // Collect vertex inputs from the graphics vertex shader only. Looking at
    // every compiled shader reflection is order-dependent and can accidentally
    // validate a 3D pipeline against an unrelated 2D/fullscreen shader layout.
    let vertex_inputs = shaders
        .first()
        .and_then(|shader| *shader)
        .and_then(|shader| reflections.get(&shader))
        .map(|reflection| reflection.vertex_inputs.clone())
        .unwrap_or_default();
    ShaderReflection {
        layout,
        entry_points,
        parameters,
        vertex_inputs,
        workgroup_size: None,
        wave_ops_used: false,
    }
}

fn merge_shader_layouts<const N: usize>(
    reflections: &HashMap<ShaderHandle, ShaderReflection>,
    shaders: [Option<ShaderHandle>; N],
) -> CanonicalPipelineLayout {
    use crate::CanonicalBinding;
    use std::collections::BTreeMap;

    let mut groups: BTreeMap<usize, (String, Vec<(String, CanonicalBinding)>)> = BTreeMap::new();
    let mut push_constants_bytes = 0;
    let mut push_constants_stage_mask = StageMask::default();

    for shader in shaders.into_iter().flatten() {
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

fn register_created_image_bindless_indices(
    inner: &mut DeviceInner,
    handle: ImageHandle,
    desc: ImageDesc,
) {
    if desc.transient {
        return;
    }
    if desc.usage.contains(ImageUsage::SAMPLED) {
        register_bindless_sampled_image(inner, handle);
    }
    if desc.usage.contains(ImageUsage::STORAGE) {
        register_bindless_storage_image(inner, handle);
    }
}

fn register_bindless_sampled_image(inner: &mut DeviceInner, handle: ImageHandle) -> Option<u32> {
    if let Some(index) = inner.bindless_sampled_images.get(&handle).copied() {
        return Some(index);
    }
    let desc = inner.images.get(&handle)?;
    if desc.transient || !desc.usage.contains(ImageUsage::SAMPLED) {
        return None;
    }
    let index = inner.backend.register_bindless_sampled_image(handle)?;
    inner.bindless_sampled_images.insert(handle, index);
    Some(index)
}

fn register_bindless_storage_image(inner: &mut DeviceInner, handle: ImageHandle) -> Option<u32> {
    if let Some(index) = inner.bindless_storage_images.get(&handle).copied() {
        return Some(index);
    }
    let desc = inner.images.get(&handle)?;
    if desc.transient || !desc.usage.contains(ImageUsage::STORAGE) {
        return None;
    }
    let index = inner.backend.register_bindless_storage_image(handle)?;
    inner.bindless_storage_images.insert(handle, index);
    Some(index)
}

fn register_bindless_sampler(inner: &mut DeviceInner, handle: SamplerHandle) -> Option<u32> {
    if let Some(index) = inner.bindless_samplers.get(&handle).copied() {
        return Some(index);
    }
    inner.samplers.get(&handle)?;
    let index = inner.backend.register_bindless_sampler(handle)?;
    inner.bindless_samplers.insert(handle, index);
    Some(index)
}

fn register_bindless_storage_buffer(inner: &mut DeviceInner, handle: BufferHandle) -> Option<u32> {
    if let Some(index) = inner.bindless_storage_buffers.get(&handle).copied() {
        return Some(index);
    }
    let desc = inner.buffers.get(&handle)?;
    if !desc.usage.contains(BufferUsage::STORAGE) {
        return None;
    }
    let index = inner.backend.register_bindless_storage_buffer(handle)?;
    inner.bindless_storage_buffers.insert(handle, index);
    Some(index)
}

fn validate_buffer_handle(inner: &DeviceInner, handle: BufferHandle) -> Result<()> {
    inner
        .buffers
        .contains_key(&handle)
        .then_some(())
        .ok_or(Error::InvalidHandle)
}

fn validate_as_handle(inner: &DeviceInner, handle: AccelerationStructureHandle) -> Result<()> {
    inner
        .acceleration_structures
        .contains_key(&handle)
        .then_some(())
        .ok_or(Error::InvalidHandle)
}

struct SbtHandles {
    raygen: Vec<Vec<u8>>,
    miss: Vec<Vec<u8>>,
    hit: Vec<Vec<u8>>,
    callable: Vec<Vec<u8>>,
}

struct SbtLayout {
    stride: u64,
    total_size: u64,
    raygen_offset: u64,
    raygen_size: u64,
    miss_offset: u64,
    miss_size: u64,
    hit_offset: u64,
    hit_size: u64,
    callable_offset: u64,
    callable_size: u64,
}

impl SbtLayout {
    fn new(
        desc: &ShaderBindingTableDesc,
        props: crate::ShaderBindingTableProperties,
    ) -> Result<Self> {
        let stride = align_up_u64(
            props.shader_group_handle_size as u64,
            props.shader_group_handle_alignment as u64,
        )?;
        if stride > props.max_shader_group_stride as u64 {
            return Err(Error::InvalidInput(format!(
                "shader binding table stride {stride} exceeds max shader group stride {}",
                props.max_shader_group_stride
            )));
        }
        let base_alignment = props.shader_group_base_alignment as u64;
        let raygen_offset = 0;
        let raygen_size = stride;
        let miss_offset = align_up_u64(raygen_offset + raygen_size, base_alignment)?;
        let miss_size = checked_region_size(desc.miss_groups.len(), stride, "miss")?;
        let hit_offset = align_up_u64(miss_offset + miss_size, base_alignment)?;
        let hit_size = checked_region_size(desc.hit_groups.len(), stride, "hit")?;
        let callable_offset = align_up_u64(hit_offset + hit_size, base_alignment)?;
        let callable_size = checked_region_size(desc.callable_groups.len(), stride, "callable")?;
        let total_size = (callable_offset + callable_size).max(raygen_size);

        Ok(Self {
            stride,
            total_size,
            raygen_offset,
            raygen_size,
            miss_offset,
            miss_size,
            hit_offset,
            hit_size,
            callable_offset,
            callable_size,
        })
    }
}

fn validate_sbt_groups(desc: &ShaderBindingTableDesc, group_count: u32) -> Result<()> {
    let validate = |group: u32, label: &str| {
        if group >= group_count {
            return Err(Error::InvalidInput(format!(
                "{label} shader group index {group} exceeds ray-tracing pipeline group count {group_count}"
            )));
        }
        Ok(())
    };
    validate(desc.raygen_group, "raygen")?;
    for &group in &desc.miss_groups {
        validate(group, "miss")?;
    }
    for &group in &desc.hit_groups {
        validate(group, "hit")?;
    }
    for &group in &desc.callable_groups {
        validate(group, "callable")?;
    }
    Ok(())
}

fn validate_sbt_properties(props: crate::ShaderBindingTableProperties) -> Result<()> {
    if props.shader_group_handle_size == 0
        || props.shader_group_handle_alignment == 0
        || props.shader_group_base_alignment == 0
        || props.max_shader_group_stride == 0
    {
        return Err(Error::Backend(
            "Vulkan returned invalid ray-tracing SBT properties".into(),
        ));
    }
    Ok(())
}

fn collect_sbt_group_handles(
    backend: &dyn Backend,
    desc: &ShaderBindingTableDesc,
    props: crate::ShaderBindingTableProperties,
) -> Result<SbtHandles> {
    let handle_size = props.shader_group_handle_size as usize;
    let fetch_one = |group| {
        let handles = backend.ray_tracing_shader_group_handles(desc.pipeline, group, 1)?;
        if handles.len() != handle_size {
            return Err(Error::Backend(format!(
                "expected ray-tracing shader group handle size {handle_size}, got {}",
                handles.len()
            )));
        }
        Ok(handles)
    };
    let collect_many =
        |groups: &[u32]| groups.iter().copied().map(fetch_one).collect::<Result<_>>();

    Ok(SbtHandles {
        raygen: vec![fetch_one(desc.raygen_group)?],
        miss: collect_many(&desc.miss_groups)?,
        hit: collect_many(&desc.hit_groups)?,
        callable: collect_many(&desc.callable_groups)?,
    })
}

fn write_sbt_region(
    data: &mut [u8],
    offset: u64,
    stride: u64,
    handle_size: u32,
    handles: &[Vec<u8>],
) {
    let handle_size = handle_size as usize;
    for (index, handle) in handles.iter().enumerate() {
        let start = offset as usize + index * stride as usize;
        data[start..start + handle_size].copy_from_slice(handle);
    }
}

fn checked_region_size(group_count: usize, stride: u64, label: &str) -> Result<u64> {
    (group_count as u64).checked_mul(stride).ok_or_else(|| {
        Error::InvalidInput(format!(
            "shader binding table {label} region size overflowed"
        ))
    })
}

fn align_up_u64(value: u64, alignment: u64) -> Result<u64> {
    if alignment == 0 {
        return Err(Error::InvalidInput("alignment must be non-zero".into()));
    }
    let add = alignment - 1;
    let value = value
        .checked_add(add)
        .ok_or_else(|| Error::InvalidInput("alignment overflowed".into()))?;
    Ok(value / alignment * alignment)
}

fn validate_blas_build_desc(desc: &BlasBuildDesc) -> Result<()> {
    if desc.mode == AccelerationStructureBuildMode::Compact {
        if desc.src.is_none() {
            return Err(Error::InvalidInput(
                "BLAS compaction requires a source acceleration structure".into(),
            ));
        }
        return Ok(());
    }
    if desc.geometries.is_empty() {
        return Err(Error::InvalidInput(
            "BLAS build requires at least one geometry".into(),
        ));
    }
    for geometry in &desc.geometries {
        if geometry.vertex_count == 0 {
            return Err(Error::InvalidInput(
                "BLAS geometry vertex_count must be non-zero".into(),
            ));
        }
        if geometry.vertex_stride == 0 {
            return Err(Error::InvalidInput(
                "BLAS geometry vertex_stride must be non-zero".into(),
            ));
        }
        if geometry.index_buffer.is_some() {
            if geometry.index_count == 0 || geometry.index_count % 3 != 0 {
                return Err(Error::InvalidInput(
                    "indexed BLAS geometry index_count must be a non-zero multiple of 3".into(),
                ));
            }
            if geometry.index_format.is_none() {
                return Err(Error::InvalidInput(
                    "indexed BLAS geometry requires an index format".into(),
                ));
            }
        } else if geometry.vertex_count % 3 != 0 {
            return Err(Error::InvalidInput(
                "non-indexed BLAS geometry vertex_count must be a multiple of 3".into(),
            ));
        }
    }
    Ok(())
}

fn validate_tlas_build_desc(desc: &TlasBuildDesc) -> Result<()> {
    if desc.mode == AccelerationStructureBuildMode::Compact {
        if desc.src.is_none() {
            return Err(Error::InvalidInput(
                "TLAS compaction requires a source acceleration structure".into(),
            ));
        }
        return Ok(());
    }
    if desc.instance_count == 0 {
        return Err(Error::InvalidInput(
            "TLAS build instance_count must be non-zero".into(),
        ));
    }
    Ok(())
}

fn acceleration_structure_desc(
    inner: &DeviceInner,
    handle: AccelerationStructureHandle,
) -> Result<AccelerationStructureDesc> {
    inner
        .acceleration_structures
        .get(&handle)
        .copied()
        .ok_or(Error::InvalidHandle)
}

fn compact_acceleration_structure_build_sizes(
    src_desc: AccelerationStructureDesc,
    expected_kind: crate::AccelerationStructureKind,
) -> Result<AccelerationStructureBuildSizes> {
    if src_desc.kind != expected_kind {
        return Err(Error::InvalidInput(format!(
            "compaction source kind {:?} does not match expected {:?}",
            src_desc.kind, expected_kind
        )));
    }
    Ok(AccelerationStructureBuildSizes {
        acceleration_structure_size: src_desc.size,
        build_scratch_size: 0,
        update_scratch_size: 0,
    })
}

fn validate_sample_count(samples: u8, max_supported: u8, label: &str) -> Result<()> {
    if !matches!(samples, 1 | 2 | 4 | 8 | 16) {
        return Err(Error::InvalidInput(format!(
            "{label} sample count must be one of 1, 2, 4, 8, or 16"
        )));
    }
    let max_supported = max_supported.max(1).min(16);
    if samples > max_supported {
        return Err(Error::InvalidInput(format!(
            "{label} sample count {samples} exceeds device max color sample count {max_supported}"
        )));
    }
    Ok(())
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
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut inner = self.device.inner.lock();
        let graph = inner
            .frames
            .get_mut(&self.handle)
            .ok_or(Error::InvalidHandle)?;
        f(graph)
    }

    pub fn flush(&mut self) -> Result<SubmissionHandle> {
        let compiled = {
            //panic allowed, reason = "poisoned mutex is unrecoverable"
            let inner = self.device.inner.lock();
            let graph = inner.frames.get(&self.handle).ok_or_else(|| {
                Error::ResourceStateCorruption(format!(
                    "frame flush could not find render graph for frame handle {:?}",
                    self.handle
                ))
            })?;
            graph.compile().map_err(|error| {
                Error::ResourceStateCorruption(format!(
                    "render graph compile failed during frame flush: {error:?}"
                ))
            })?
        };

        let token = {
            //panic allowed, reason = "poisoned mutex is unrecoverable"
            let mut inner = self.device.inner.lock();
            // `backend.flush` → `submit_graph` waits the previous frame's fence
            // before submitting.  Everything after this point is safe to destroy.
            let token = inner.backend.flush(&compiled).map_err(|error| {
                Error::ResourceStateCorruption(format!(
                    "backend flush failed for compiled graph with {} passes: {error:?}",
                    compiled.passes.len()
                ))
            })?;

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
                    DeferredDestroy::AccelerationStructure(h) => {
                        inner.backend.destroy_acceleration_structure(h)
                    }
                    DeferredDestroy::Sampler(h) => inner.backend.destroy_sampler(h),
                    DeferredDestroy::Shader(h) => inner.backend.destroy_shader(h),
                    DeferredDestroy::Pipeline(h) => inner.backend.destroy_pipeline(h),
                    DeferredDestroy::PipelineLayout(h) => inner.backend.destroy_pipeline_layout(h),
                    DeferredDestroy::BindGroup(h) => inner.backend.destroy_bind_group(h),
                    DeferredDestroy::VideoSession(h) => inner.backend.destroy_video_session(h),
                    DeferredDestroy::IndirectCommandLayout(h) => {
                        inner.backend.destroy_indirect_command_layout(h)
                    }
                    DeferredDestroy::OpticalFlowSession(h) => {
                        inner.backend.destroy_optical_flow_session(h)
                    }
                    DeferredDestroy::ShaderObject(h) => inner.backend.destroy_shader_object(h),
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
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.device.inner.lock().backend.present()
    }

    /// Block until the GPU finishes the work submitted by `flush`.
    /// If `flush` has not been called yet this is a no-op.
    pub fn wait(&self) -> Result<()> {
        match self.last_submission {
            Some(token) => self.device.wait_for_submission(token),
            None => Ok(()),
        }
    }

    pub fn last_submission(&self) -> Option<SubmissionHandle> {
        self.last_submission
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        let mut inner = self.device.inner.lock();
        if true {
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
#[path = "device_tests.rs"]
mod tests;
