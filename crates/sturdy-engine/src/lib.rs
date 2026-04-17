//! Ergonomic Rust API for Sturdy Engine.
//!
//! Use this crate from Rust applications. It wraps the core handle-oriented API
//! with RAII resource types and builder-style descriptors while keeping the
//! lower-level `sturdy-engine-core` crate available for engine internals.

use std::collections::HashMap;

#[cfg(test)]
mod tests;
mod texture;

pub use glam::{Vec2, Vec3};
#[cfg(not(target_arch = "wasm32"))]
pub use sturdy_engine_core::NativeSurfaceDesc;
pub use sturdy_engine_core::{
    Access, AddressMode, BackendKind, BindGroupDesc, BindGroupEntry, BindingKind, BorderColor,
    BufferDesc, BufferUsage, BufferUse, CanonicalBinding, CanonicalGroupLayout,
    CanonicalPipelineLayout, Caps, ColorTargetDesc, CompareOp, CompiledShaderArtifact,
    ComputePipelineDesc, CopyBufferToImageDesc, CopyImageToBufferDesc, CullMode, DispatchDesc,
    DrawDesc, Error, Extent3d, FilterMode, Format, FrontFace, GraphicsPipelineDesc, ImageDesc,
    ImageUsage, ImageUse, IndexBufferBinding, IndexFormat, MipmapMode, PassDesc, PassWork,
    PrimitiveTopology, PushConstants, QueueType, RasterState, ResourceBinding, Result, RgState,
    SamplerDesc, ShaderDesc, ShaderSource, ShaderStage, ShaderTarget, SlangCompileDesc, StageMask,
    SubresourceRange, UpdateRate, VertexAttributeDesc, VertexBufferBinding, VertexBufferLayout,
    VertexFormat, VertexInputRate, compile_slang, compile_slang_to_file, compile_slang_to_spirv,
    spirv_words_from_bytes,
};
pub use sturdy_engine_core::{ImageHandle, SamplerHandle, SurfaceHandle, SurfaceSize};
pub use texture::{ImageCopyRegion, TextureUploadDesc};

use sturdy_engine_core as core;

#[derive(Clone)]
pub struct Engine {
    device: core::Device,
}

impl Engine {
    pub fn new() -> Result<Self> {
        Self::with_backend(BackendKind::Auto)
    }

    pub fn with_backend(backend: BackendKind) -> Result<Self> {
        Ok(Self {
            device: core::Device::create(core::DeviceDesc {
                backend,
                validation: cfg!(debug_assertions),
            })?,
        })
    }

    pub fn caps(&self) -> Caps {
        self.device.caps()
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

    pub fn create_buffer(&self, desc: BufferDesc) -> Result<Buffer> {
        let handle = self.device.create_buffer(desc)?;
        Ok(Buffer {
            device: self.device.clone(),
            handle,
            desc,
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
            transient_buffers: Vec::new(),
        })
    }

    pub fn render_image(
        &self,
        image: &Image,
        render: impl FnOnce(&mut RenderContext<'_>) -> Result<()>,
    ) -> Result<()> {
        let mut frame = self.begin_frame()?;
        frame.import_image(image)?;
        {
            let mut context = RenderContext {
                frame: &mut frame,
                framebuffer: Framebuffer {
                    image: image.handle(),
                    format: image.desc().format,
                },
            };
            render(&mut context)?;
        }
        frame.flush()?;
        frame.wait()
    }

    pub fn render_surface(
        &self,
        surface: &Surface,
        render: impl FnOnce(&mut RenderContext<'_>) -> Result<()>,
    ) -> Result<()> {
        let surface_image = surface.acquire_image()?;
        let mut frame = self.begin_frame()?;
        frame.import_surface_image(&surface_image)?;
        {
            let mut context = RenderContext {
                frame: &mut frame,
                framebuffer: Framebuffer {
                    image: surface_image.handle(),
                    format: surface_image.desc().format,
                },
            };
            render(&mut context)?;
            context.present_framebuffer()?;
        }
        frame.flush()?;
        frame.wait()?;
        surface.present()
    }

    pub fn wait_idle(&self) -> Result<()> {
        self.device.wait_idle()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn create_surface(&self, desc: NativeSurfaceDesc) -> Result<Surface> {
        let handle = self.device.create_surface(desc)?;
        Ok(Surface {
            device: self.device.clone(),
            handle,
            size: desc.size,
        })
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RenderVertex {
    pub position: Vec2,
    pub color: Vec3,
}

impl RenderVertex {
    pub const fn new(position: Vec2, color: Vec3) -> Self {
        Self { position, color }
    }
}

pub struct RenderMesh {
    vertex_buffer: Buffer,
    vertex_count: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct PackedRenderVertex {
    position: [f32; 2],
    color: [f32; 3],
}

impl RenderMesh {
    pub fn new(engine: &Engine, vertices: &[RenderVertex]) -> Result<Self> {
        if vertices.is_empty() {
            return Err(Error::InvalidInput(
                "render mesh requires at least one vertex".into(),
            ));
        }
        let packed = vertices
            .iter()
            .map(|vertex| PackedRenderVertex {
                position: vertex.position.to_array(),
                color: vertex.color.to_array(),
            })
            .collect::<Vec<_>>();
        let vertex_buffer = engine.create_buffer(BufferDesc {
            size: std::mem::size_of_val(packed.as_slice()) as u64,
            usage: BufferUsage::VERTEX,
        })?;
        vertex_buffer.write(0, bytes_of_slice(packed.as_slice()))?;
        Ok(Self {
            vertex_buffer,
            vertex_count: vertices.len() as u32,
        })
    }
}

pub struct RenderShader {
    engine: Engine,
    vertex_shader: Shader,
    fragment_shader: Shader,
    pipelines: HashMap<Format, Pipeline>,
}

impl RenderShader {
    pub fn new(engine: &Engine, vertex_spirv: Vec<u32>, fragment_spirv: Vec<u32>) -> Result<Self> {
        let vertex_shader = engine.create_shader(ShaderDesc {
            source: ShaderSource::Spirv(vertex_spirv),
            entry_point: "main".to_owned(),
            stage: ShaderStage::Vertex,
        })?;
        let fragment_shader = engine.create_shader(ShaderDesc {
            source: ShaderSource::Spirv(fragment_spirv),
            entry_point: "main".to_owned(),
            stage: ShaderStage::Fragment,
        })?;
        Ok(Self {
            engine: engine.clone(),
            vertex_shader,
            fragment_shader,
            pipelines: HashMap::new(),
        })
    }

    fn pipeline(&mut self, format: Format) -> Result<&Pipeline> {
        if !self.pipelines.contains_key(&format) {
            let pipeline = self.engine.create_graphics_pipeline(GraphicsPipelineDesc {
                vertex_shader: self.vertex_shader.handle(),
                fragment_shader: Some(self.fragment_shader.handle()),
                layout: None,
                vertex_buffers: vec![VertexBufferLayout {
                    binding: 0,
                    stride: std::mem::size_of::<PackedRenderVertex>() as u32,
                    input_rate: VertexInputRate::Vertex,
                }],
                vertex_attributes: vec![
                    VertexAttributeDesc {
                        location: 0,
                        binding: 0,
                        format: VertexFormat::Float32x2,
                        offset: std::mem::offset_of!(PackedRenderVertex, position) as u32,
                    },
                    VertexAttributeDesc {
                        location: 1,
                        binding: 0,
                        format: VertexFormat::Float32x3,
                        offset: std::mem::offset_of!(PackedRenderVertex, color) as u32,
                    },
                ],
                color_targets: vec![ColorTargetDesc { format }],
                depth_format: None,
                topology: PrimitiveTopology::TriangleList,
                raster: RasterState {
                    cull_mode: CullMode::None,
                    front_face: FrontFace::CounterClockwise,
                },
            })?;
            self.pipelines.insert(format, pipeline);
        }
        self.pipelines
            .get(&format)
            .ok_or_else(|| Error::Unknown("render shader pipeline cache miss".into()))
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Framebuffer {
    image: ImageHandle,
    format: Format,
}

pub struct RenderContext<'a> {
    frame: &'a mut Frame,
    framebuffer: Framebuffer,
}

impl RenderContext<'_> {
    pub fn draw_mesh(&mut self, mesh: &RenderMesh, shader: &mut RenderShader) -> Result<()> {
        let shader_handle = shader.vertex_shader.handle();
        let pipeline_handle = shader.pipeline(self.framebuffer.format)?.handle();
        self.frame.import_buffer(&mesh.vertex_buffer)?;
        self.frame.add_pass(PassDesc {
            name: "draw-mesh".to_owned(),
            queue: QueueType::Graphics,
            shader: Some(shader_handle),
            pipeline: Some(pipeline_handle),
            bind_groups: Vec::new(),
            push_constants: None,
            work: PassWork::Draw(DrawDesc {
                vertex_count: mesh.vertex_count,
                instance_count: 1,
                first_vertex: 0,
                first_instance: 0,
                vertex_buffer: Some(VertexBufferBinding {
                    buffer: mesh.vertex_buffer.handle(),
                    binding: 0,
                    offset: 0,
                }),
                index_buffer: None,
            }),
            reads: Vec::new(),
            writes: vec![ImageUse {
                image: self.framebuffer.image,
                access: Access::Write,
                state: RgState::RenderTarget,
                subresource: SubresourceRange {
                    base_mip: 0,
                    mip_count: 1,
                    base_layer: 0,
                    layer_count: 1,
                },
            }],
            buffer_reads: vec![BufferUse {
                buffer: mesh.vertex_buffer.handle(),
                access: Access::Read,
                state: RgState::VertexRead,
                offset: 0,
                size: mesh.vertex_buffer.desc().size,
            }],
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
        })
    }

    fn present_framebuffer(&mut self) -> Result<()> {
        self.frame.add_pass(PassDesc {
            name: "present-framebuffer".to_owned(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            work: PassWork::None,
            reads: vec![ImageUse {
                image: self.framebuffer.image,
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
}

fn bytes_of_slice<T>(values: &[T]) -> &[u8] {
    let len = std::mem::size_of_val(values);
    unsafe { std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), len) }
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
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        let _ = self.device.destroy_pipeline(self.handle);
    }
}

pub struct Surface {
    device: core::Device,
    handle: core::SurfaceHandle,
    size: SurfaceSize,
}

impl Surface {
    pub fn handle(&self) -> core::SurfaceHandle {
        self.handle
    }

    pub fn size(&self) -> SurfaceSize {
        self.size
    }

    pub fn resize(&mut self, size: SurfaceSize) -> Result<()> {
        self.device.resize_surface(self.handle, size)?;
        self.size = size;
        Ok(())
    }

    pub fn acquire_image(&self) -> Result<SurfaceImage> {
        let handle = self.device.acquire_surface_image(self.handle)?;
        let desc = self.device.image_desc(handle)?;
        Ok(SurfaceImage {
            device: self.device.clone(),
            handle,
            desc,
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
    pub(crate) transient_buffers: Vec<Buffer>,
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

    pub fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }

    pub fn present(&mut self) -> Result<()> {
        self.inner.present()
    }

    pub fn wait(&self) -> Result<()> {
        self.inner.wait()
    }
}
