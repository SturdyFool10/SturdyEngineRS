//! Ergonomic Rust API for Sturdy Engine.
//!
//! Use this crate from Rust applications. It wraps the core handle-oriented API
//! with RAII resource types and builder-style descriptors while keeping the
//! lower-level `sturdy-engine-core` crate available for engine internals.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

mod application;
mod bind_group;
mod bloom_pass;
mod compute_program;
mod device_manager;
mod frontend_graph;
mod gpu_procedural_texture;
mod graph_frame;
mod hdr_pipeline;
mod mesh;
mod mesh_program;
mod mip_pyramid;
mod pipeline_layout;
mod procedural_texture;
mod quad_batch;
mod sampler_catalog;
#[cfg(test)]
mod tests;
mod text_draw;
mod text_engine;
mod texture;
mod upload_arena;

#[cfg(not(target_arch = "wasm32"))]
pub use application::{EngineApp, ShellFrame, WindowConfig, run};
pub use bloom_pass::{
    BloomCompositeConstants, BloomConfig, BloomPass, BrightPassConstants, DownsampleConstants,
    UpsampleConstants,
};
pub use compute_program::ComputeProgram;
pub use device_manager::{AdapterEntry, DeviceManager};
pub use gpu_procedural_texture::GpuProceduralTexture;
pub use graph_frame::{FullscreenPassBuilder, GraphFrame, ImageNode};
pub use hdr_pipeline::{HdrMode, HdrPipelineDesc, HdrPreference, ToneMappingOp};
pub use mesh::{Mesh, Vertex2d, Vertex3d};
pub use mesh_program::{MeshProgram, MeshProgramDesc, MeshVertexKind};
pub use mip_pyramid::MipPyramid;
pub use procedural_texture::{
    CpuProceduralTexture2d, ProceduralTextureRecipe, ProceduralTextureUpdatePolicy,
};
pub use quad_batch::QuadBatch;
pub use sampler_catalog::SamplerPreset;
pub use text_draw::{
    TextAtlasPage, TextDrawDesc, TextGlyphQuad, TextLayoutOutput, TextPlacement, TextRenderer,
    TextScene, TextSceneQuad, TextTypography,
};
pub use text_engine::{
    PreparedTextDraw, PreparedTextQuad, TextEngine, TextEngineFrame, TextUiRenderer,
};

pub use bind_group::BindGroupBuilder;
pub use frontend_graph::{
    DiagnosticLevel, GraphDiagnostic, GraphImage, GraphImageCacheKey, GraphImageInfo,
    GraphPassInfo, GraphReport, PassKind, RenderFrame, ShaderProgram, ShaderProgramDesc,
};
pub use glam::{Vec2, Vec3};
pub use pipeline_layout::PipelineLayoutBuilder;
#[cfg(not(target_arch = "wasm32"))]
pub use sturdy_engine_core::NativeSurfaceDesc;
pub use sturdy_engine_core::ShaderReflection;
pub use sturdy_engine_core::{
    Access, AdapterInfo, AdapterKind, AdapterSelection, AddressMode, BackendKind,
    BackendRawCapabilities, BindGroupDesc, BindGroupEntry, BindingKind, BlendMode, BorderColor,
    BufferDesc, BufferUsage, BufferUse, CanonicalBinding, CanonicalGroupLayout,
    CanonicalPipelineLayout, Caps, ColorTargetDesc, CompareOp, CompiledShaderArtifact,
    ComputePipelineDesc, CopyBufferToImageDesc, CopyImageToBufferDesc, CullMode,
    D3d12RawCapabilities, DispatchDesc, DrawDesc, Error, Extent3d, ExternalBufferDesc,
    ExternalBufferHandle, ExternalImageDesc, ExternalImageHandle, FilterMode, Format,
    FormatCapabilities, FrontFace, GpuCaptureDesc, GpuCaptureTool, GraphicsPipelineDesc,
    ImageBuilder, ImageDesc, ImageDimension, ImageRole, ImageUsage, ImageUse, IndexBufferBinding,
    IndexFormat, MetalRawCapabilities, MipmapMode, NativeHandleCapabilities,
    NativeHandleCapability, NativeHandleKind, NativeHandleOwnership, PassDesc, PassWork,
    PrimitiveTopology, PushConstants, QueueType, RasterState, ResourceBinding, Result, RgState,
    SamplerDesc, ShaderDesc, ShaderSource, ShaderStage, ShaderTarget, SlangCompileDesc, StageMask,
    SubresourceRange, SurfaceColorSpace, SurfaceEvent, SurfaceHdrCaps, SurfaceHdrPreference,
    SurfaceInfo, SurfacePresentMode, SurfaceRecreateDesc, UpdateRate, VertexAttributeDesc,
    VertexBufferBinding, VertexBufferLayout, VertexFormat, VertexInputRate, VulkanExternalBuffer,
    VulkanExternalImage, VulkanRawCapabilities, compile_slang, compile_slang_to_file,
    compile_slang_to_spirv, native_handle_capabilities_for_backend, spirv_words_from_bytes,
};
pub use sturdy_engine_core::{
    DeviceDesc, ImageHandle, SamplerHandle, SubmissionHandle, SurfaceHandle, SurfaceSize,
};
pub use sturdy_engine_macros::push_constants;
pub use texture::{ImageCopyRegion, TextureUploadDesc};

use sturdy_engine_core as core;
use upload_arena::UploadArena;

#[derive(Clone)]
pub struct Engine {
    device: core::Device,
    graph_image_cache: Arc<Mutex<HashMap<GraphImageCacheKey, Image>>>,
    sampler_catalog: Arc<sampler_catalog::SamplerCatalog>,
}

impl Engine {
    pub fn new() -> Result<Self> {
        Self::with_backend(BackendKind::Auto)
    }

    pub fn with_backend(backend: BackendKind) -> Result<Self> {
        let mut desc = core::DeviceDesc {
            backend,
            validation: cfg!(debug_assertions),
            adapter: core::AdapterSelection::Auto,
            ..core::DeviceDesc::default()
        };
        desc.optional_features
            .push("sampler_anisotropy".to_string());
        Self::with_desc(desc)
    }

    pub fn with_desc(desc: core::DeviceDesc) -> Result<Self> {
        let device = core::Device::create(desc)?;
        let mut engine = Self {
            device,
            graph_image_cache: Arc::new(Mutex::new(HashMap::new())),
            sampler_catalog: Arc::new(sampler_catalog::SamplerCatalog::empty()),
        };
        let catalog = sampler_catalog::SamplerCatalog::build(&engine)?;
        engine.sampler_catalog = Arc::new(catalog);
        Ok(engine)
    }

    /// Return the handle for a sampler preset. Used internally to resolve shader bindings.
    pub fn sampler_handle(&self, preset: SamplerPreset) -> core::SamplerHandle {
        self.sampler_catalog.handle(preset)
    }

    pub(crate) fn default_sampler(&self) -> core::SamplerHandle {
        self.sampler_catalog.handle(SamplerPreset::Linear)
    }

    pub fn caps(&self) -> Caps {
        self.device.caps()
    }

    pub fn format_capabilities(&self, format: Format) -> FormatCapabilities {
        self.device.format_capabilities(format)
    }

    pub fn native_handle_capabilities(&self) -> NativeHandleCapabilities {
        self.device.native_handle_capabilities()
    }

    pub fn raw_capabilities(&self) -> BackendRawCapabilities {
        self.device.raw_capabilities()
    }

    pub fn backend_kind(&self) -> BackendKind {
        self.device.backend_kind()
    }

    pub fn adapter_name(&self) -> Option<String> {
        self.device.adapter_name()
    }

    pub fn create_image(&self, desc: ImageDesc) -> Result<Image> {
        let handle = self.device.create_image(desc)?;
        Ok(Image {
            device: self.device.clone(),
            handle,
            desc,
        })
    }

    /// Import a borrowed native image into the engine.
    ///
    /// # Safety
    ///
    /// The caller must uphold the backend-specific lifetime and compatibility
    /// requirements documented by `Device::import_external_image`.
    pub unsafe fn import_external_image(&self, desc: ExternalImageDesc) -> Result<Image> {
        let handle = unsafe { self.device.import_external_image(desc)? };
        Ok(Image {
            device: self.device.clone(),
            handle,
            desc: desc.desc,
        })
    }

    pub fn create_buffer(&self, desc: BufferDesc) -> Result<Buffer> {
        let handle = self.device.create_buffer(desc)?;
        Ok(Buffer {
            device: self.device.clone(),
            handle,
            desc,
        })
    }

    /// Import a borrowed native buffer into the engine.
    ///
    /// # Safety
    ///
    /// The caller must uphold the backend-specific lifetime and compatibility
    /// requirements documented by `Device::import_external_buffer`.
    pub unsafe fn import_external_buffer(&self, desc: ExternalBufferDesc) -> Result<Buffer> {
        let handle = unsafe { self.device.import_external_buffer(desc)? };
        Ok(Buffer {
            device: self.device.clone(),
            handle,
            desc: desc.desc,
        })
    }

    pub fn write_buffer(&self, buffer: &Buffer, offset: u64, data: &[u8]) -> Result<()> {
        self.device.write_buffer(buffer.handle, offset, data)
    }

    pub fn read_buffer(&self, buffer: &Buffer, offset: u64, out: &mut [u8]) -> Result<()> {
        self.device.read_buffer(buffer.handle, offset, out)
    }

    pub fn create_sampler(&self, desc: SamplerDesc) -> Result<Sampler> {
        let handle = self.device.create_sampler(desc)?;
        Ok(Sampler {
            device: self.device.clone(),
            handle,
            desc,
        })
    }

    pub fn create_shader(&self, desc: ShaderDesc) -> Result<Shader> {
        let handle = self.device.create_shader(desc.clone())?;
        Ok(Shader {
            device: self.device.clone(),
            handle,
            desc,
        })
    }

    pub fn load_shader(&self, path: impl Into<std::path::PathBuf>) -> Result<ShaderProgram> {
        ShaderProgram::load_fragment(self, path)
    }

    pub fn create_shader_program(&self, desc: ShaderProgramDesc) -> Result<ShaderProgram> {
        ShaderProgram::new(self, desc)
    }

    pub fn begin_render_frame(&self) -> Result<RenderFrame> {
        RenderFrame::new(self.clone(), 0)
    }

    /// Begin a render frame whose per-frame graph image cache is keyed by the
    /// given swapchain image. Use this instead of `begin_render_frame` when
    /// rendering to a swapchain so that intermediate images (e.g. `scene_color`)
    /// get separate GPU allocations for each swapchain slot, preventing races
    /// between frames in flight.
    pub fn begin_render_frame_for(&self, surface_image: &SurfaceImage) -> Result<RenderFrame> {
        RenderFrame::new(self.clone(), surface_image.slot)
    }

    pub(crate) fn cached_graph_image(
        &self,
        key: GraphImageCacheKey,
        desc: ImageDesc,
    ) -> Result<(core::ImageHandle, ImageDesc)> {
        let mut cache = self
            .graph_image_cache
            .lock()
            .expect("graph image cache mutex poisoned");
        if let Some(image) = cache.get(&key) {
            return Ok((image.handle(), image.desc()));
        }

        // Evict any stale entry that has the same name+slot but a different
        // descriptor (e.g. after a swapchain resize changed the image dimensions).
        cache.retain(|k, _| !k.is_stale_for(&key));

        let image = self.create_image(desc)?;
        if let Some(name) = key.debug_name() {
            let _ = image.set_debug_name(&name);
        }
        let handle = image.handle();
        let desc = image.desc();
        cache.insert(key, image);
        Ok((handle, desc))
    }

    pub fn shader_reflection(&self, shader: &Shader) -> Result<ShaderReflection> {
        self.device.shader_reflection(shader.handle())
    }

    pub fn create_bind_group(&self, desc: BindGroupDesc) -> Result<BindGroup> {
        let handle = self.device.create_bind_group(desc.clone())?;
        Ok(BindGroup {
            device: self.device.clone(),
            handle,
            desc,
        })
    }

    pub fn create_pipeline_layout(
        &self,
        layout: CanonicalPipelineLayout,
    ) -> Result<PipelineLayout> {
        let handle = self.device.create_pipeline_layout(layout.clone())?;
        Ok(PipelineLayout {
            device: self.device.clone(),
            handle,
            layout,
        })
    }

    pub fn create_compute_pipeline(&self, desc: ComputePipelineDesc) -> Result<Pipeline> {
        let handle = self.device.create_compute_pipeline(desc)?;
        Ok(Pipeline {
            device: self.device.clone(),
            handle,
        })
    }

    pub fn create_graphics_pipeline(&self, desc: GraphicsPipelineDesc) -> Result<Pipeline> {
        let handle = self.device.create_graphics_pipeline(desc)?;
        Ok(Pipeline {
            device: self.device.clone(),
            handle,
        })
    }

    pub fn begin_frame(&self) -> Result<Frame> {
        Ok(Frame {
            engine: self.clone(),
            inner: self.device.begin_frame()?,
            upload_arena: UploadArena::default(),
        })
    }

    /// Begin a new image-centric graph frame.
    pub fn begin_graph_frame(&self) -> Result<GraphFrame> {
        let frame = self.begin_frame()?;
        Ok(GraphFrame::new(self.clone(), frame))
    }

    /// Generate a 2-D texture from a CPU pixel function, upload it, and return the image.
    ///
    /// `fill` receives `(x, y)` for every pixel and returns `[r, g, b, a]` as `u8`.
    /// The texture is created, uploaded, and the GPU work is submitted synchronously
    /// before this call returns.  Use this for one-time assets such as noise maps,
    /// gradient ramps, lookup tables, and debug patterns.
    ///
    /// The returned [`Image`] is sampled as `Rgba8Unorm` and ready to use as a
    /// shader input in subsequent frames.
    pub fn generate_texture_2d(
        &self,
        name: impl Into<String>,
        width: u32,
        height: u32,
        fill: impl Fn(u32, u32) -> [u8; 4],
    ) -> Result<Image> {
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                let rgba = fill(x, y);
                let i = ((y * width + x) * 4) as usize;
                pixels[i..i + 4].copy_from_slice(&rgba);
            }
        }
        let name = name.into();
        let mut frame = self.begin_frame()?;
        let image = frame.upload_texture_2d(
            &name,
            crate::TextureUploadDesc::sampled_rgba8(width, height),
            &pixels,
        )?;
        let _ = image.set_debug_name(&format!("procedural-{name}"));
        frame.flush()?;
        frame.wait()?;
        Ok(image)
    }

    pub fn wait_idle(&self) -> Result<()> {
        self.device.wait_idle()
    }

    pub fn supported_gpu_capture_tools(&self) -> Vec<GpuCaptureTool> {
        self.device.supported_gpu_capture_tools()
    }

    pub fn begin_gpu_capture(&self, desc: &GpuCaptureDesc) -> Result<()> {
        self.device.begin_gpu_capture(desc)
    }

    pub fn end_gpu_capture(&self, tool: GpuCaptureTool) -> Result<()> {
        self.device.end_gpu_capture(tool)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn create_surface(&self, desc: NativeSurfaceDesc) -> Result<Surface> {
        let handle = self.device.create_surface(desc)?;
        let info = self.device.surface_info(handle)?;
        Ok(Surface {
            device: self.device.clone(),
            handle,
            info,
        })
    }

    /// Create a surface from any window that provides raw handles.
    ///
    /// Handles extraction, `.as_raw()`, error mapping, and size clamping so
    /// callers never need to import `raw_window_handle` directly or write
    /// unsafe handle-lifetime casts.
    ///
    /// ```ignore
    /// let surface = engine.create_surface_for_window(&window, SurfaceSize { width: 1280, height: 720 })?;
    /// ```
    #[cfg(not(target_arch = "wasm32"))]
    pub fn create_surface_for_window(
        &self,
        window: &(impl raw_window_handle::HasWindowHandle + raw_window_handle::HasDisplayHandle),
        size: SurfaceSize,
    ) -> Result<Surface> {
        self.create_surface_for_window_with_hdr(window, size, SurfaceHdrPreference::Sdr)
    }

    /// Create a surface from a window and request a specific HDR preference.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn create_surface_for_window_with_hdr(
        &self,
        window: &(impl raw_window_handle::HasWindowHandle + raw_window_handle::HasDisplayHandle),
        size: SurfaceSize,
        hdr: SurfaceHdrPreference,
    ) -> Result<Surface> {
        use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
        let display = window
            .display_handle()
            .map_err(|e| Error::InvalidInput(e.to_string()))?;
        let window_handle = window
            .window_handle()
            .map_err(|e| Error::InvalidInput(e.to_string()))?;
        // SAFETY: The window outlives the surface because both live in the same
        // ShellApp struct and the surface is dropped before the window.
        let raw_display: RawDisplayHandle = unsafe { std::mem::transmute_copy(&display) };
        let raw_window: RawWindowHandle = unsafe { std::mem::transmute_copy(&window_handle) };
        let mut desc = NativeSurfaceDesc::new(
            raw_display,
            raw_window,
            SurfaceSize {
                width: size.width.max(1),
                height: size.height.max(1),
            },
        );
        desc.hdr = hdr;
        self.create_surface(desc)
    }
}

pub struct Image {
    device: core::Device,
    handle: core::ImageHandle,
    desc: ImageDesc,
}

impl Image {
    pub fn handle(&self) -> core::ImageHandle {
        self.handle
    }

    pub fn desc(&self) -> ImageDesc {
        self.desc
    }

    pub fn set_debug_name(&self, name: &str) -> Result<()> {
        self.device.set_image_debug_name(self.handle, name)
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        let _ = self.device.destroy_image(self.handle);
    }
}

pub struct Buffer {
    device: core::Device,
    handle: core::BufferHandle,
    desc: BufferDesc,
}

impl Buffer {
    pub fn handle(&self) -> core::BufferHandle {
        self.handle
    }

    pub fn desc(&self) -> BufferDesc {
        self.desc
    }

    pub fn write(&self, offset: u64, data: &[u8]) -> Result<()> {
        self.device.write_buffer(self.handle, offset, data)
    }

    pub fn read(&self, offset: u64, out: &mut [u8]) -> Result<()> {
        self.device.read_buffer(self.handle, offset, out)
    }

    pub fn set_debug_name(&self, name: &str) -> Result<()> {
        self.device.set_buffer_debug_name(self.handle, name)
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        let _ = self.device.destroy_buffer(self.handle);
    }
}

pub struct Sampler {
    device: core::Device,
    handle: core::SamplerHandle,
    desc: SamplerDesc,
}

impl Sampler {
    pub fn handle(&self) -> core::SamplerHandle {
        self.handle
    }

    pub fn desc(&self) -> SamplerDesc {
        self.desc
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        let _ = self.device.destroy_sampler(self.handle);
    }
}

pub struct Shader {
    device: core::Device,
    handle: core::ShaderHandle,
    desc: ShaderDesc,
}

impl Shader {
    pub fn handle(&self) -> core::ShaderHandle {
        self.handle
    }

    pub fn desc(&self) -> &ShaderDesc {
        &self.desc
    }
}

impl Drop for Shader {
    fn drop(&mut self) {
        let _ = self.device.destroy_shader(self.handle);
    }
}

pub struct BindGroup {
    device: core::Device,
    handle: core::BindGroupHandle,
    desc: BindGroupDesc,
}

impl BindGroup {
    pub fn handle(&self) -> core::BindGroupHandle {
        self.handle
    }

    pub fn desc(&self) -> &BindGroupDesc {
        &self.desc
    }
}

impl Drop for BindGroup {
    fn drop(&mut self) {
        let _ = self.device.destroy_bind_group(self.handle);
    }
}

pub struct PipelineLayout {
    device: core::Device,
    handle: core::PipelineLayoutHandle,
    layout: CanonicalPipelineLayout,
}

impl PipelineLayout {
    pub fn handle(&self) -> core::PipelineLayoutHandle {
        self.handle
    }

    pub fn layout(&self) -> &CanonicalPipelineLayout {
        &self.layout
    }
}

impl Drop for PipelineLayout {
    fn drop(&mut self) {
        let _ = self.device.destroy_pipeline_layout(self.handle);
    }
}

pub struct Pipeline {
    device: core::Device,
    handle: core::PipelineHandle,
}

impl Pipeline {
    pub fn handle(&self) -> core::PipelineHandle {
        self.handle
    }

    pub fn set_debug_name(&self, name: &str) -> Result<()> {
        self.device.set_pipeline_debug_name(self.handle, name)
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        let _ = self.device.destroy_pipeline(self.handle);
    }
}

pub struct Surface {
    device: core::Device,
    handle: core::SurfaceHandle,
    info: SurfaceInfo,
}

impl Surface {
    pub fn handle(&self) -> core::SurfaceHandle {
        self.handle
    }

    pub fn size(&self) -> SurfaceSize {
        self.info.size
    }

    pub fn info(&self) -> SurfaceInfo {
        self.info
    }

    pub fn resize(&mut self, size: SurfaceSize) -> Result<()> {
        self.device.resize_surface(self.handle, size)?;
        self.info = self.device.surface_info(self.handle)?;
        Ok(())
    }

    pub fn recreate(&mut self, desc: SurfaceRecreateDesc) -> Result<()> {
        self.device.recreate_surface(self.handle, desc)?;
        self.info = self.device.surface_info(self.handle)?;
        Ok(())
    }

    pub fn drain_events(&mut self) -> Result<Vec<SurfaceEvent>> {
        let events = self.device.drain_surface_events(self.handle)?;
        self.info = self.device.surface_info(self.handle)?;
        Ok(events)
    }

    pub fn hdr_caps(&self) -> Result<SurfaceHdrCaps> {
        self.device.surface_hdr_caps(self.handle)
    }

    pub fn acquire_image(&self) -> Result<SurfaceImage> {
        let (handle, slot) = self.device.acquire_surface_image(self.handle)?;
        let desc = self.device.image_desc(handle)?;
        Ok(SurfaceImage {
            device: self.device.clone(),
            handle,
            desc,
            slot,
        })
    }

    pub fn present(&self) -> Result<()> {
        self.device.present_surface(self.handle)
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        let _ = self.device.destroy_surface(self.handle);
    }
}

pub struct SurfaceImage {
    device: core::Device,
    handle: core::ImageHandle,
    desc: ImageDesc,
    /// Stable swapchain image index (0..swapchain_image_count).
    slot: u64,
}

impl SurfaceImage {
    pub fn handle(&self) -> core::ImageHandle {
        self.handle
    }

    pub fn desc(&self) -> ImageDesc {
        self.desc
    }
}

impl Drop for SurfaceImage {
    fn drop(&mut self) {
        let _ = self.device.destroy_image(self.handle);
    }
}

pub trait ImageRef {
    fn image_handle(&self) -> core::ImageHandle;
    fn image_desc(&self) -> ImageDesc;
}

impl ImageRef for Image {
    fn image_handle(&self) -> core::ImageHandle {
        self.handle
    }
    fn image_desc(&self) -> ImageDesc {
        self.desc
    }
}

impl ImageRef for SurfaceImage {
    fn image_handle(&self) -> core::ImageHandle {
        self.handle
    }
    fn image_desc(&self) -> ImageDesc {
        self.desc
    }
}

pub struct DrawPassBuilder<'f> {
    frame: &'f mut Frame,
    name: String,
    pipeline: Option<core::PipelineHandle>,
    bind_groups: Vec<core::BindGroupHandle>,
    color_writes: Vec<(core::ImageHandle, ImageDesc)>,
    depth_write: Option<(core::ImageHandle, ImageDesc)>,
    image_reads: Vec<(core::ImageHandle, ImageDesc)>,
    extra_buffer_reads: Vec<(core::BufferHandle, BufferDesc)>,
    vertex_buf: Option<(core::BufferHandle, BufferDesc, u32, u64)>,
    index_buf: Option<(core::BufferHandle, BufferDesc, IndexFormat, u64)>,
    vertex_count: u32,
    instance_count: u32,
    first_vertex: u32,
    first_instance: u32,
    push_constants: Option<PushConstants>,
    /// Clear color per image handle (stored as f32 bit-patterns).
    clear_colors: Vec<(core::ImageHandle, [u32; 4])>,
    clear_depth: Option<(core::ImageHandle, u32, u8)>,
}

impl<'f> DrawPassBuilder<'f> {
    pub fn color(mut self, image: &impl ImageRef) -> Self {
        self.color_writes
            .push((image.image_handle(), image.image_desc()));
        self
    }

    /// Clear the last added color attachment to `rgba` before this pass executes.
    /// Must be called after the `.color()` call for the image to clear.
    pub fn clear_color(mut self, rgba: [f32; 4]) -> Self {
        if let Some((handle, _)) = self.color_writes.last() {
            let bits = rgba.map(f32::to_bits);
            self.clear_colors.push((*handle, bits));
        }
        self
    }

    /// Clear the depth attachment to `depth` (and `stencil`) before this pass executes.
    pub fn clear_depth(mut self, depth: f32, stencil: u8) -> Self {
        if let Some((handle, _)) = self.depth_write {
            self.clear_depth = Some((handle, depth.to_bits(), stencil));
        }
        self
    }

    pub fn depth(mut self, image: &impl ImageRef) -> Self {
        self.depth_write = Some((image.image_handle(), image.image_desc()));
        self
    }

    pub fn sample(mut self, image: &impl ImageRef) -> Self {
        self.image_reads
            .push((image.image_handle(), image.image_desc()));
        self
    }

    pub fn pipeline(mut self, pipeline: &Pipeline) -> Self {
        self.pipeline = Some(pipeline.handle());
        self
    }

    pub fn bind(mut self, bind_group: &BindGroup) -> Self {
        self.bind_groups.push(bind_group.handle());
        self
    }

    pub fn push_constants(mut self, stages: StageMask, bytes: &[u8]) -> Self {
        self.push_constants = Some(PushConstants {
            offset: 0,
            stages,
            bytes: bytes.to_vec(),
        });
        self
    }

    pub fn push_constants_at(mut self, offset: u32, stages: StageMask, bytes: &[u8]) -> Self {
        self.push_constants = Some(PushConstants {
            offset,
            stages,
            bytes: bytes.to_vec(),
        });
        self
    }

    pub fn vertex_buffer(mut self, buffer: &Buffer, binding: u32, offset: u64) -> Self {
        self.vertex_buf = Some((buffer.handle(), buffer.desc(), binding, offset));
        self
    }

    pub fn index_buffer(mut self, buffer: &Buffer, format: IndexFormat, offset: u64) -> Self {
        self.index_buf = Some((buffer.handle(), buffer.desc(), format, offset));
        self
    }

    pub fn draw(mut self, vertex_count: u32) -> Self {
        self.vertex_count = vertex_count;
        self
    }

    pub fn draw_instanced(mut self, vertex_count: u32, instance_count: u32) -> Self {
        self.vertex_count = vertex_count;
        self.instance_count = instance_count;
        self
    }

    pub fn submit(self) -> Result<()> {
        let Self {
            frame,
            name,
            pipeline,
            bind_groups,
            color_writes,
            depth_write,
            image_reads,
            extra_buffer_reads,
            vertex_buf,
            index_buf,
            vertex_count,
            instance_count,
            first_vertex,
            first_instance,
            push_constants,
            clear_colors,
            clear_depth,
        } = self;

        for (handle, desc) in &color_writes {
            frame.inner.graph_mut(|g| g.import_image(*handle, *desc))?;
        }
        if let Some((handle, desc)) = &depth_write {
            frame.inner.graph_mut(|g| g.import_image(*handle, *desc))?;
        }
        for (handle, desc) in &image_reads {
            frame.inner.graph_mut(|g| g.import_image(*handle, *desc))?;
        }
        if let Some((handle, desc, _, _)) = &vertex_buf {
            frame.inner.graph_mut(|g| g.import_buffer(*handle, *desc))?;
        }
        if let Some((handle, desc, _, _)) = &index_buf {
            frame.inner.graph_mut(|g| g.import_buffer(*handle, *desc))?;
        }
        for (handle, desc) in &extra_buffer_reads {
            frame.inner.graph_mut(|g| g.import_buffer(*handle, *desc))?;
        }

        let subresource = SubresourceRange {
            base_mip: 0,
            mip_count: 1,
            base_layer: 0,
            layer_count: 1,
        };

        let writes: Vec<ImageUse> = color_writes
            .iter()
            .map(|(h, _)| ImageUse {
                image: *h,
                access: Access::Write,
                state: RgState::RenderTarget,
                subresource,
            })
            .chain(depth_write.iter().map(|(h, _)| ImageUse {
                image: *h,
                access: Access::Write,
                state: RgState::DepthWrite,
                subresource,
            }))
            .collect();

        let reads: Vec<ImageUse> = image_reads
            .iter()
            .map(|(h, _)| ImageUse {
                image: *h,
                access: Access::Read,
                state: RgState::ShaderRead,
                subresource,
            })
            .collect();

        let mut buffer_reads: Vec<BufferUse> = Vec::new();
        if let Some((handle, desc, _, _)) = &vertex_buf {
            buffer_reads.push(BufferUse {
                buffer: *handle,
                access: Access::Read,
                state: RgState::VertexRead,
                offset: 0,
                size: desc.size,
            });
        }
        if let Some((handle, desc, _, _)) = &index_buf {
            buffer_reads.push(BufferUse {
                buffer: *handle,
                access: Access::Read,
                state: RgState::IndexRead,
                offset: 0,
                size: desc.size,
            });
        }
        for (handle, desc) in &extra_buffer_reads {
            buffer_reads.push(BufferUse {
                buffer: *handle,
                access: Access::Read,
                state: RgState::ShaderRead,
                offset: 0,
                size: desc.size,
            });
        }

        let vertex_buffer = vertex_buf.map(|(handle, _, binding, offset)| VertexBufferBinding {
            buffer: handle,
            binding,
            offset,
        });
        let index_buffer = index_buf.map(|(handle, _, format, offset)| IndexBufferBinding {
            buffer: handle,
            offset,
            format,
        });

        frame.add_pass(PassDesc {
            name,
            queue: QueueType::Graphics,
            shader: None,
            pipeline,
            bind_groups,
            push_constants,
            work: PassWork::Draw(DrawDesc {
                vertex_count,
                instance_count,
                first_vertex,
                first_instance,
                vertex_buffer,
                index_buffer,
            }),
            reads,
            writes,
            buffer_reads,
            buffer_writes: Vec::new(),
            clear_colors,
            clear_depth,
        })
    }
}

pub struct ComputePassBuilder<'f> {
    frame: &'f mut Frame,
    name: String,
    pipeline: Option<core::PipelineHandle>,
    bind_groups: Vec<core::BindGroupHandle>,
    push_constants: Option<PushConstants>,
    image_reads: Vec<(core::ImageHandle, ImageDesc)>,
    image_writes: Vec<(core::ImageHandle, ImageDesc)>,
    buffer_reads: Vec<(core::BufferHandle, BufferDesc)>,
    buffer_writes: Vec<(core::BufferHandle, BufferDesc)>,
    dispatch: Option<DispatchDesc>,
}

impl<'f> ComputePassBuilder<'f> {
    pub fn read_image(mut self, image: &impl ImageRef) -> Self {
        self.image_reads
            .push((image.image_handle(), image.image_desc()));
        self
    }

    pub fn write_image(mut self, image: &impl ImageRef) -> Self {
        self.image_writes
            .push((image.image_handle(), image.image_desc()));
        self
    }

    pub fn read_buffer(mut self, buffer: &Buffer) -> Self {
        self.buffer_reads.push((buffer.handle(), buffer.desc()));
        self
    }

    pub fn write_buffer(mut self, buffer: &Buffer) -> Self {
        self.buffer_writes.push((buffer.handle(), buffer.desc()));
        self
    }

    pub fn pipeline(mut self, pipeline: &Pipeline) -> Self {
        self.pipeline = Some(pipeline.handle());
        self
    }

    pub fn bind(mut self, bind_group: &BindGroup) -> Self {
        self.bind_groups.push(bind_group.handle());
        self
    }

    pub fn push_constants(mut self, stages: StageMask, bytes: &[u8]) -> Self {
        self.push_constants = Some(PushConstants {
            offset: 0,
            stages,
            bytes: bytes.to_vec(),
        });
        self
    }

    pub fn push_constants_at(mut self, offset: u32, stages: StageMask, bytes: &[u8]) -> Self {
        self.push_constants = Some(PushConstants {
            offset,
            stages,
            bytes: bytes.to_vec(),
        });
        self
    }

    pub fn dispatch(mut self, x: u32, y: u32, z: u32) -> Self {
        self.dispatch = Some(DispatchDesc { x, y, z });
        self
    }

    pub fn submit(self) -> Result<()> {
        let Self {
            frame,
            name,
            pipeline,
            bind_groups,
            push_constants,
            image_reads,
            image_writes,
            buffer_reads,
            buffer_writes,
            dispatch,
        } = self;

        let dispatch = dispatch.ok_or_else(|| {
            Error::InvalidInput("compute pass requires a dispatch call before submit".into())
        })?;

        for (handle, desc) in image_reads.iter().chain(image_writes.iter()) {
            frame.inner.graph_mut(|g| g.import_image(*handle, *desc))?;
        }
        for (handle, desc) in buffer_reads.iter().chain(buffer_writes.iter()) {
            frame.inner.graph_mut(|g| g.import_buffer(*handle, *desc))?;
        }

        let subresource = SubresourceRange {
            base_mip: 0,
            mip_count: 1,
            base_layer: 0,
            layer_count: 1,
        };

        let reads: Vec<ImageUse> = image_reads
            .iter()
            .map(|(h, _)| ImageUse {
                image: *h,
                access: Access::Read,
                state: RgState::ShaderRead,
                subresource,
            })
            .collect();

        let writes: Vec<ImageUse> = image_writes
            .iter()
            .map(|(h, _)| ImageUse {
                image: *h,
                access: Access::Write,
                state: RgState::ShaderWrite,
                subresource,
            })
            .collect();

        let buf_reads: Vec<BufferUse> = buffer_reads
            .iter()
            .map(|(h, desc)| BufferUse {
                buffer: *h,
                access: Access::Read,
                state: RgState::ShaderRead,
                offset: 0,
                size: desc.size,
            })
            .collect();

        let buf_writes: Vec<BufferUse> = buffer_writes
            .iter()
            .map(|(h, desc)| BufferUse {
                buffer: *h,
                access: Access::Write,
                state: RgState::ShaderWrite,
                offset: 0,
                size: desc.size,
            })
            .collect();

        frame.add_pass(PassDesc {
            name,
            queue: QueueType::Compute,
            shader: None,
            pipeline,
            bind_groups,
            push_constants,
            work: PassWork::Dispatch(dispatch),
            reads,
            writes,
            buffer_reads: buf_reads,
            buffer_writes: buf_writes,
            clear_colors: Vec::new(),
            clear_depth: None,
        })
    }
}

pub struct Frame {
    pub(crate) engine: Engine,
    pub(crate) inner: core::Frame,
    pub(crate) upload_arena: UploadArena,
}

impl Frame {
    pub fn import_image(&mut self, image: &Image) -> Result<()> {
        self.inner
            .graph_mut(|graph| graph.import_image(image.handle(), image.desc()))
    }

    pub fn import_surface_image(&mut self, image: &SurfaceImage) -> Result<()> {
        self.inner
            .graph_mut(|graph| graph.import_image(image.handle(), image.desc()))
    }

    pub fn import_buffer(&mut self, buffer: &Buffer) -> Result<()> {
        self.inner
            .graph_mut(|graph| graph.import_buffer(buffer.handle(), buffer.desc()))
    }

    pub fn add_pass(&mut self, pass: PassDesc) -> Result<()> {
        self.inner.graph_mut(|graph| graph.add_pass(pass))
    }

    pub fn debug_marker(&mut self, name: impl Into<String>) -> Result<()> {
        self.add_pass(PassDesc {
            name: name.into(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            work: PassWork::None,
            reads: Vec::new(),
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
        })
    }

    pub fn draw_pass(&mut self, name: impl Into<String>) -> DrawPassBuilder<'_> {
        DrawPassBuilder {
            frame: self,
            name: name.into(),
            pipeline: None,
            bind_groups: Vec::new(),
            color_writes: Vec::new(),
            depth_write: None,
            image_reads: Vec::new(),
            extra_buffer_reads: Vec::new(),
            vertex_buf: None,
            index_buf: None,
            vertex_count: 0,
            instance_count: 1,
            first_vertex: 0,
            first_instance: 0,
            push_constants: None,
            clear_colors: Vec::new(),
            clear_depth: None,
        }
    }

    pub fn compute_pass(&mut self, name: impl Into<String>) -> ComputePassBuilder<'_> {
        ComputePassBuilder {
            frame: self,
            name: name.into(),
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            image_reads: Vec::new(),
            image_writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            dispatch: None,
        }
    }

    pub fn present_image(&mut self, image: &impl ImageRef) -> Result<()> {
        self.inner
            .graph_mut(|g| g.import_image(image.image_handle(), image.image_desc()))?;
        self.add_pass(PassDesc {
            name: "present".to_owned(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            work: PassWork::None,
            reads: vec![ImageUse {
                image: image.image_handle(),
                access: Access::Read,
                state: RgState::Present,
                subresource: SubresourceRange {
                    base_mip: 0,
                    mip_count: 1,
                    base_layer: 0,
                    layer_count: 1,
                },
            }],
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
        })
    }

    pub fn flush(&mut self) -> Result<SubmissionHandle> {
        self.inner.flush()
    }

    pub fn present(&mut self) -> Result<()> {
        self.inner.present()
    }

    pub fn wait(&self) -> Result<()> {
        self.inner.wait()
    }

    /// Finish rendering this frame and present to the given surface in a single call.
    ///
    /// This is a convenience method that calls `flush()`, `wait()`, and
    /// `surface.present()` in sequence, returning the first error if any step fails.
    ///
    /// It is the replacement for the common three-call pattern:
    /// ```ignore
    /// frame.flush()?;
    /// frame.wait()?;
    /// self.surface.present()?;
    /// ```
    ///
    /// **Note**: The caller must have already called [`present_image`](Self::present_image)
    /// with the surface image that will be presented.
    pub fn finish_and_present(&mut self, surface: &Surface) -> Result<()> {
        self.flush()?;
        self.wait()?;
        surface.present()
    }
}
