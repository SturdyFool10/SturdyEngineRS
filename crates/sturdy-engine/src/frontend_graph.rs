use std::{cell::RefCell, collections::HashMap, path::PathBuf, rc::Rc, sync::Mutex};

use sturdy_engine_core as core;

use crate::{
    compute_program::ComputeProgram,
    mesh::Mesh,
    mesh_program::MeshProgram,
    sampler_catalog::SamplerPreset,
    Access, BindGroup, BindGroupDesc, BindGroupEntry, BindingKind, Buffer, BufferDesc, BufferUsage,
    ColorTargetDesc, CullMode, DispatchDesc, DrawDesc, Engine, Error, Format, FrontFace,
    GraphicsPipelineDesc, ImageDesc, ImageHandle, ImageRef, IndexBufferBinding, PassDesc, PassWork,
    Pipeline, PipelineLayout, PrimitiveTopology, PushConstants, QueueType, RasterState,
    ResourceBinding, Result, RgState, Shader, ShaderDesc, ShaderReflection, ShaderSource,
    ShaderStage, StageMask, SubresourceRange, SurfaceImage, VertexAttributeDesc,
    VertexBufferBinding, VertexBufferLayout, VertexFormat, VertexInputRate,
};

const FULLSCREEN_VERTEX_SHADER: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shaders/fullscreen_vertex.slang"));

#[repr(C)]
#[derive(Copy, Clone)]
struct FullscreenVertex {
    position: [f32; 2],
    uv: [f32; 2],
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct GraphImageCacheKey {
    name: String,
    desc_key: GraphImageDescKey,
    swapchain_slot: u64,
    usage: u32,
}

impl GraphImageCacheKey {
    pub fn new(name: impl Into<String>, desc: ImageDesc, swapchain_slot: u64) -> Self {
        Self {
            name: name.into(),
            desc_key: GraphImageDescKey::from(desc),
            swapchain_slot,
            usage: desc.usage.0,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn debug_name(&self) -> Option<String> {
        (!self.name.is_empty()).then(|| format!("graph-image-{}", self.name))
    }
}

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
struct GraphImageDescKey {
    dimension: core::ImageDimension,
    width: u32,
    height: u32,
    depth: u32,
    mip_levels: u16,
    layers: u16,
    samples: u8,
    format: Format,
    usage: u32,
    transient: bool,
    clear_value: Option<core::ImageClearValue>,
}

impl From<ImageDesc> for GraphImageDescKey {
    fn from(desc: ImageDesc) -> Self {
        Self {
            dimension: desc.dimension,
            width: desc.extent.width,
            height: desc.extent.height,
            depth: desc.extent.depth,
            mip_levels: desc.mip_levels,
            layers: desc.layers,
            samples: desc.samples,
            format: desc.format,
            usage: desc.usage.0,
            transient: desc.transient,
            clear_value: desc.clear_value,
        }
    }
}

pub struct ShaderProgramDesc {
    pub fragment: ShaderDesc,
    pub vertex: Option<ShaderDesc>,
}

pub struct ShaderProgram {
    engine: Engine,
    pipelines: Mutex<HashMap<Format, Pipeline>>,
    pub(crate) pipeline_layout: PipelineLayout,
    vertex: Shader,
    fragment: Shader,
    fullscreen_triangle: Buffer,
    reflection: ShaderReflection,
}

impl ShaderProgram {
    /// Create a fragment `ShaderProgram` from an inline Slang source string.
    ///
    /// Useful with `include_str!` to embed a shader that lives in a `.slang`
    /// file alongside the crate without needing a runtime file path.
    pub fn from_inline_fragment(engine: &Engine, source: &str) -> Result<Self> {
        Self::new(
            engine,
            ShaderProgramDesc {
                vertex: None,
                fragment: ShaderDesc {
                    source: ShaderSource::Inline(source.to_owned()),
                    entry_point: "main".to_owned(),
                    stage: ShaderStage::Fragment,
                },
            },
        )
    }

    /// Load a fragment shader from `path`.
    ///
    /// If the path has a `.spv` extension the file is read as pre-compiled
    /// SPIR-V (via [`ShaderSource::Spirv`]).  Any other extension is compiled
    /// at runtime through the Slang compiler (via [`ShaderSource::File`]).
    pub fn load_fragment(engine: &Engine, path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let source = if path.extension().and_then(|e| e.to_str()) == Some("spv") {
            let bytes = std::fs::read(&path).map_err(|e| {
                Error::Unknown(format!("failed to read SPIR-V file {}: {e}", path.display()))
            })?;
            ShaderSource::Spirv(crate::spirv_words_from_bytes(&bytes).map_err(|e| {
                Error::Unknown(format!(
                    "invalid SPIR-V in {}: {e}",
                    path.display()
                ))
            })?)
        } else {
            ShaderSource::File(path)
        };
        Self::new(
            engine,
            ShaderProgramDesc {
                vertex: None,
                fragment: ShaderDesc {
                    source,
                    entry_point: "main".to_owned(),
                    stage: ShaderStage::Fragment,
                },
            },
        )
    }

    pub fn new(engine: &Engine, desc: ShaderProgramDesc) -> Result<Self> {
        let vertex = engine.create_shader(desc.vertex.unwrap_or_else(default_vertex_desc))?;
        let fragment = engine.create_shader(desc.fragment)?;
        let reflection = engine.shader_reflection(&fragment)?;
        let pipeline_layout = engine.create_pipeline_layout(reflection.layout.clone())?;
        let fullscreen_triangle = create_fullscreen_triangle(engine)?;
        Ok(Self {
            engine: engine.clone(),
            pipelines: Mutex::new(HashMap::new()),
            pipeline_layout,
            vertex,
            fragment,
            fullscreen_triangle,
            reflection,
        })
    }

    pub fn reflection(&self) -> &ShaderReflection {
        &self.reflection
    }

    fn pipeline_handle(&self, format: Format) -> Result<core::PipelineHandle> {
        let mut pipelines = self
            .pipelines
            .lock()
            .expect("shader program pipeline mutex poisoned");
        if !pipelines.contains_key(&format) {
            let pipeline = self.engine.create_graphics_pipeline(GraphicsPipelineDesc {
                vertex_shader: self.vertex.handle(),
                fragment_shader: Some(self.fragment.handle()),
                layout: Some(self.pipeline_layout.handle()),
                vertex_buffers: vec![VertexBufferLayout {
                    binding: 0,
                    stride: std::mem::size_of::<FullscreenVertex>() as u32,
                    input_rate: VertexInputRate::Vertex,
                }],
                vertex_attributes: vec![
                    VertexAttributeDesc {
                        location: 0,
                        binding: 0,
                        format: VertexFormat::Float32x2,
                        offset: std::mem::offset_of!(FullscreenVertex, position) as u32,
                    },
                    VertexAttributeDesc {
                        location: 1,
                        binding: 0,
                        format: VertexFormat::Float32x2,
                        offset: std::mem::offset_of!(FullscreenVertex, uv) as u32,
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
            pipeline.set_debug_name("reflected-fullscreen-program")?;
            pipelines.insert(format, pipeline);
        }
        pipelines
            .get(&format)
            .map(Pipeline::handle)
            .ok_or_else(|| Error::Unknown("shader program pipeline cache miss".into()))
    }
}

fn create_fullscreen_triangle(engine: &Engine) -> Result<Buffer> {
    let vertices = [
        FullscreenVertex {
            position: [-1.0, -3.0],
            uv: [0.0, -1.0],
        },
        FullscreenVertex {
            position: [-1.0, 1.0],
            uv: [0.0, 1.0],
        },
        FullscreenVertex {
            position: [3.0, 1.0],
            uv: [2.0, 1.0],
        },
    ];
    let buffer = engine.create_buffer(BufferDesc {
        size: std::mem::size_of_val(&vertices) as u64,
        usage: BufferUsage::VERTEX,
    })?;
    buffer.write(0, bytes_of_slice(&vertices))?;
    buffer.set_debug_name("shader-program-fullscreen-triangle")?;
    Ok(buffer)
}

fn bytes_of_slice<T>(values: &[T]) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), std::mem::size_of_val(values))
    }
}

fn default_vertex_desc() -> ShaderDesc {
    ShaderDesc {
        source: ShaderSource::Inline(FULLSCREEN_VERTEX_SHADER.to_owned()),
        entry_point: "main".to_owned(),
        stage: ShaderStage::Vertex,
    }
}

#[derive(Clone)]
pub struct RenderFrame {
    inner: Rc<RefCell<RenderFrameInner>>,
}

struct RenderFrameInner {
    engine: Engine,
    frame: crate::Frame,
    images_by_name: HashMap<String, GraphImageRecord>,
    samplers_by_name: HashMap<String, core::SamplerHandle>,
    held_bind_groups: Vec<BindGroup>,
    declaration_index: u32,
    swapchain_slot: u64,
    flushed: bool,
}

#[derive(Copy, Clone)]
struct GraphImageRecord {
    handle: ImageHandle,
    desc: ImageDesc,
}

impl RenderFrame {
    pub(crate) fn new(engine: Engine, swapchain_slot: u64) -> Result<Self> {
        let frame = engine.begin_frame()?;
        Ok(Self {
            inner: Rc::new(RefCell::new(RenderFrameInner {
                engine,
                frame,
                images_by_name: HashMap::new(),
                samplers_by_name: HashMap::new(),
                held_bind_groups: Vec::new(),
                declaration_index: 0,
                swapchain_slot,
                flushed: false,
            })),
        })
    }

    pub fn image(&self, name: impl Into<String>, desc: ImageDesc) -> Result<GraphImage> {
        let name = name.into();
        let mut inner = self.inner.borrow_mut();
        let slot = inner.swapchain_slot;
        let key = GraphImageCacheKey::new(name.clone(), desc, slot);
        let (handle, desc) = inner.engine.cached_graph_image(key, desc)?;
        inner
            .frame
            .inner
            .graph_mut(|graph| graph.import_image(handle, desc))?;
        inner
            .images_by_name
            .insert(name.clone(), GraphImageRecord { handle, desc });
        Ok(GraphImage {
            frame: self.inner.clone(),
            name,
            handle,
            desc,
        })
    }

    pub fn swapchain_image(&self, image: &SurfaceImage) -> Result<GraphImage> {
        let name = "swapchain".to_owned();
        let mut inner = self.inner.borrow_mut();
        inner.frame.import_surface_image(image)?;
        inner.images_by_name.insert(
            name.clone(),
            GraphImageRecord {
                handle: image.handle(),
                desc: image.desc(),
            },
        );
        Ok(GraphImage {
            frame: self.inner.clone(),
            name,
            handle: image.handle(),
            desc: image.desc(),
        })
    }

    /// Register a sampler preset under a name.
    ///
    /// When the engine auto-creates bind groups from shader reflection, any
    /// `SamplerState` binding whose variable name matches `name` will use this
    /// sampler instead of the default bilinear sampler.
    ///
    /// Call this before the first `execute_shader` or `draw_mesh` that needs it.
    pub fn set_sampler(&self, name: impl Into<String>, preset: SamplerPreset) -> &Self {
        let mut inner = self.inner.borrow_mut();
        let handle = inner.engine.sampler_handle(preset);
        inner.samplers_by_name.insert(name.into(), handle);
        self
    }

    pub fn flush(&self) -> Result<core::SubmissionHandle> {
        let mut inner = self.inner.borrow_mut();
        inner.flushed = true;
        inner.frame.flush()
    }

    pub fn wait(&self) -> Result<()> {
        self.inner.borrow().frame.wait()
    }

    pub fn present_image(&self, image: &GraphImage) -> Result<()> {
        let mut inner = self.inner.borrow_mut();
        inner.frame.present_image(image)
    }
}

impl Drop for RenderFrame {
    fn drop(&mut self) {
        if Rc::strong_count(&self.inner) != 1 {
            return;
        }
        let mut inner = self.inner.borrow_mut();
        if inner.flushed {
            return;
        }
        let _ = inner.frame.flush();
    }
}

pub struct GraphImage {
    frame: Rc<RefCell<RenderFrameInner>>,
    name: String,
    handle: ImageHandle,
    desc: ImageDesc,
}

impl GraphImage {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn handle(&self) -> ImageHandle {
        self.handle
    }

    pub fn desc(&self) -> ImageDesc {
        self.desc
    }

    pub fn execute_shader(&self, shader: &ShaderProgram) -> Result<()> {
        self.execute_shader_inner(shader, None)
    }

    pub fn execute_shader_with_push_constants(
        &self,
        shader: &ShaderProgram,
        stages: StageMask,
        bytes: &[u8],
    ) -> Result<()> {
        self.execute_shader_inner(
            shader,
            Some(PushConstants {
                offset: 0,
                stages,
                bytes: bytes.to_vec(),
            }),
        )
    }

    fn execute_shader_inner(
        &self,
        shader: &ShaderProgram,
        push_constants: Option<PushConstants>,
    ) -> Result<()> {
        let mut inner = self.frame.borrow_mut();
        let declaration_index = inner.declaration_index;
        inner.declaration_index = inner.declaration_index.saturating_add(1);

        inner
            .frame
            .inner
            .graph_mut(|graph| graph.import_image(self.handle, self.desc))?;
        inner.frame.inner.graph_mut(|graph| {
            graph.import_buffer(
                shader.fullscreen_triangle.handle(),
                shader.fullscreen_triangle.desc(),
            )
        })?;

        let mut reads = Vec::new();
        for binding in reflected_image_reads(shader.reflection()) {
            if binding == self.name {
                continue;
            }
            let record = inner.images_by_name.get(&binding).copied().ok_or_else(|| {
                Error::InvalidInput(format!(
                    "shader requires image '{binding}', but no frame image with that name exists"
                ))
            })?;
            inner
                .frame
                .inner
                .graph_mut(|graph| graph.import_image(record.handle, record.desc))?;
            reads.push(crate::ImageUse {
                image: record.handle,
                access: Access::Read,
                state: RgState::ShaderRead,
                subresource: SubresourceRange {
                    base_mip: 0,
                    mip_count: 1,
                    base_layer: 0,
                    layer_count: 1,
                },
            });
        }

        let pipeline = shader.pipeline_handle(self.desc.format)?;
        let bind_group = build_reflected_bind_group(
            &inner.engine,
            &shader.pipeline_layout,
            shader.reflection(),
            &inner.images_by_name,
            &inner.samplers_by_name,
            None,
        )?;
        let bind_group_handles: Vec<core::BindGroupHandle> =
            bind_group.iter().map(|bg| bg.handle()).collect();
        inner.held_bind_groups.extend(bind_group);

        inner.frame.add_pass(PassDesc {
            name: format!("{declaration_index:04}-execute-{}", self.name),
            queue: QueueType::Graphics,
            shader: Some(shader.fragment.handle()),
            pipeline: Some(pipeline),
            bind_groups: bind_group_handles,
            push_constants,
            work: PassWork::Draw(DrawDesc {
                vertex_count: 3,
                instance_count: 1,
                first_vertex: 0,
                first_instance: 0,
                vertex_buffer: Some(VertexBufferBinding {
                    buffer: shader.fullscreen_triangle.handle(),
                    binding: 0,
                    offset: 0,
                }),
                index_buffer: None,
            }),
            reads,
            writes: vec![crate::ImageUse {
                image: self.handle,
                access: Access::Write,
                state: RgState::RenderTarget,
                subresource: SubresourceRange {
                    base_mip: 0,
                    mip_count: 1,
                    base_layer: 0,
                    layer_count: 1,
                },
            }],
            buffer_reads: vec![crate::BufferUse {
                buffer: shader.fullscreen_triangle.handle(),
                access: Access::Read,
                state: RgState::VertexRead,
                offset: 0,
                size: shader.fullscreen_triangle.desc().size,
            }],
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
        })
    }

    pub fn draw_mesh(&self, mesh: &Mesh, program: &MeshProgram) -> Result<()> {
        self.draw_mesh_inner(mesh, program, None)
    }

    pub fn draw_mesh_with_push_constants(
        &self,
        mesh: &Mesh,
        program: &MeshProgram,
        stages: StageMask,
        bytes: &[u8],
    ) -> Result<()> {
        self.draw_mesh_inner(
            mesh,
            program,
            Some(PushConstants {
                offset: 0,
                stages,
                bytes: bytes.to_vec(),
            }),
        )
    }

    fn draw_mesh_inner(
        &self,
        mesh: &Mesh,
        program: &MeshProgram,
        push_constants: Option<PushConstants>,
    ) -> Result<()> {
        let mut inner = self.frame.borrow_mut();
        let declaration_index = inner.declaration_index;
        inner.declaration_index = inner.declaration_index.saturating_add(1);

        inner
            .frame
            .inner
            .graph_mut(|graph| graph.import_image(self.handle, self.desc))?;
        inner.frame.inner.graph_mut(|graph| {
            graph.import_buffer(mesh.vertex_buffer.handle(), mesh.vertex_buffer.desc())
        })?;
        if let Some(ib) = &mesh.index_buffer {
            inner
                .frame
                .inner
                .graph_mut(|graph| graph.import_buffer(ib.handle(), ib.desc()))?;
        }

        let pipeline = program.pipeline_handle(self.desc.format)?;
        let bind_group = build_reflected_bind_group(
            &inner.engine,
            &program.pipeline_layout,
            program.reflection(),
            &inner.images_by_name,
            &inner.samplers_by_name,
            None,
        )?;
        let bind_group_handles: Vec<core::BindGroupHandle> =
            bind_group.iter().map(|bg| bg.handle()).collect();
        inner.held_bind_groups.extend(bind_group);

        let draw_count = if mesh.is_indexed() {
            mesh.index_count
        } else {
            mesh.vertex_count
        };

        let vertex_buffer = Some(VertexBufferBinding {
            buffer: mesh.vertex_buffer.handle(),
            binding: 0,
            offset: 0,
        });
        let index_buffer = mesh.index_buffer.as_ref().map(|ib| IndexBufferBinding {
            buffer: ib.handle(),
            offset: 0,
            format: mesh.index_format,
        });

        let mut buffer_reads = vec![crate::BufferUse {
            buffer: mesh.vertex_buffer.handle(),
            access: Access::Read,
            state: RgState::VertexRead,
            offset: 0,
            size: mesh.vertex_buffer.desc().size,
        }];
        if let Some(ib) = &mesh.index_buffer {
            buffer_reads.push(crate::BufferUse {
                buffer: ib.handle(),
                access: Access::Read,
                state: RgState::IndexRead,
                offset: 0,
                size: ib.desc().size,
            });
        }

        inner.frame.add_pass(PassDesc {
            name: format!("{declaration_index:04}-draw-mesh-{}", self.name),
            queue: crate::QueueType::Graphics,
            shader: Some(program.fragment.handle()),
            pipeline: Some(pipeline),
            bind_groups: bind_group_handles,
            push_constants,
            work: PassWork::Draw(DrawDesc {
                vertex_count: draw_count,
                instance_count: 1,
                first_vertex: 0,
                first_instance: 0,
                vertex_buffer,
                index_buffer,
            }),
            reads: Vec::new(),
            writes: vec![crate::ImageUse {
                image: self.handle,
                access: Access::Write,
                state: RgState::RenderTarget,
                subresource: SubresourceRange {
                    base_mip: 0,
                    mip_count: 1,
                    base_layer: 0,
                    layer_count: 1,
                },
            }],
            buffer_reads,
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
        })
    }

    /// Register this image under `name` in the frame's image registry.
    ///
    /// Use this when a shader samples an input by a name that differs from the
    /// name this image was created with — e.g. aliasing `bloom_mip_5` as
    /// `"bloom_base"` before running a composite shader that declares
    /// `Texture2D<float4> bloom_base`.  Must be called before the
    /// `execute_shader` call that needs the alias.
    pub fn register_as(&self, name: impl Into<String>) {
        let mut inner = self.frame.borrow_mut();
        inner.images_by_name.insert(
            name.into(),
            GraphImageRecord {
                handle: self.handle,
                desc: self.desc,
            },
        );
    }

    pub fn execute_compute(
        &self,
        program: &ComputeProgram,
        groups: [u32; 3],
    ) -> Result<()> {
        self.execute_compute_inner(program, None, groups)
    }

    pub fn execute_compute_with_push_constants(
        &self,
        program: &ComputeProgram,
        stages: StageMask,
        bytes: &[u8],
        groups: [u32; 3],
    ) -> Result<()> {
        self.execute_compute_inner(
            program,
            Some(PushConstants {
                offset: 0,
                stages,
                bytes: bytes.to_vec(),
            }),
            groups,
        )
    }

    fn execute_compute_inner(
        &self,
        program: &ComputeProgram,
        push_constants: Option<PushConstants>,
        groups: [u32; 3],
    ) -> Result<()> {
        let mut inner = self.frame.borrow_mut();
        let declaration_index = inner.declaration_index;
        inner.declaration_index = inner.declaration_index.saturating_add(1);

        inner
            .frame
            .inner
            .graph_mut(|graph| graph.import_image(self.handle, self.desc))?;

        let mut reads = Vec::new();
        for binding in reflected_storage_image_reads(program.reflection()) {
            if binding == self.name {
                continue;
            }
            let record = inner.images_by_name.get(&binding).copied().ok_or_else(|| {
                Error::InvalidInput(format!(
                    "compute shader requires storage image '{binding}', but no frame image with that name exists"
                ))
            })?;
            inner
                .frame
                .inner
                .graph_mut(|graph| graph.import_image(record.handle, record.desc))?;
            reads.push(crate::ImageUse {
                image: record.handle,
                access: Access::Read,
                state: RgState::ShaderRead,
                subresource: SubresourceRange {
                    base_mip: 0,
                    mip_count: 1,
                    base_layer: 0,
                    layer_count: 1,
                },
            });
        }

        let bind_group = build_reflected_bind_group(
            &inner.engine,
            &program.pipeline_layout,
            program.reflection(),
            &inner.images_by_name,
            &inner.samplers_by_name,
            Some((self.name.as_str(), self.handle)),
        )?;
        let bind_group_handles: Vec<core::BindGroupHandle> =
            bind_group.iter().map(|bg| bg.handle()).collect();
        inner.held_bind_groups.extend(bind_group);

        inner.frame.add_pass(PassDesc {
            name: format!("{declaration_index:04}-compute-{}", self.name),
            queue: QueueType::Compute,
            shader: Some(program.shader.handle()),
            pipeline: Some(program.pipeline.handle()),
            bind_groups: bind_group_handles,
            push_constants,
            work: PassWork::Dispatch(DispatchDesc {
                x: groups[0],
                y: groups[1],
                z: groups[2],
            }),
            reads,
            writes: vec![crate::ImageUse {
                image: self.handle,
                access: Access::Write,
                state: RgState::ShaderWrite,
                subresource: SubresourceRange {
                    base_mip: 0,
                    mip_count: 1,
                    base_layer: 0,
                    layer_count: 1,
                },
            }],
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
        })
    }
}

impl ImageRef for GraphImage {
    fn image_handle(&self) -> ImageHandle {
        self.handle
    }

    fn image_desc(&self) -> ImageDesc {
        self.desc
    }
}

fn reflected_image_reads(reflection: &ShaderReflection) -> Vec<String> {
    reflected_bindings_of_kind(reflection, core::BindingKind::SampledImage)
}

fn reflected_storage_image_reads(reflection: &ShaderReflection) -> Vec<String> {
    reflected_bindings_of_kind(reflection, core::BindingKind::StorageImage)
}

fn reflected_bindings_of_kind(reflection: &ShaderReflection, kind: core::BindingKind) -> Vec<String> {
    reflection
        .layout
        .groups
        .iter()
        .flat_map(|group| group.bindings.iter())
        .filter(|binding| binding.kind == kind)
        .map(|binding| binding.path.clone())
        .collect()
}

/// Build a reflected bind group from shader reflection.
///
/// `output_image`: for compute passes, the image this pass writes to so it can
/// be bound as a StorageImage under its frame name.
fn build_reflected_bind_group(
    engine: &Engine,
    layout: &PipelineLayout,
    reflection: &ShaderReflection,
    images_by_name: &HashMap<String, GraphImageRecord>,
    samplers_by_name: &HashMap<String, core::SamplerHandle>,
    output_image: Option<(&str, ImageHandle)>,
) -> Result<Vec<BindGroup>> {
    let has_bindings = reflection
        .layout
        .groups
        .iter()
        .any(|g| !g.bindings.is_empty());
    if !has_bindings {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    for group in &reflection.layout.groups {
        for binding in &group.bindings {
            match binding.kind {
                BindingKind::SampledImage => {
                    if let Some(record) = images_by_name.get(&binding.path) {
                        entries.push(BindGroupEntry {
                            path: binding.path.clone(),
                            resource: ResourceBinding::Image(record.handle),
                        });
                    }
                }
                BindingKind::StorageImage => {
                    let handle = if let Some((name, h)) = output_image {
                        if binding.path == name {
                            Some(h)
                        } else {
                            images_by_name.get(&binding.path).map(|r| r.handle)
                        }
                    } else {
                        images_by_name.get(&binding.path).map(|r| r.handle)
                    };
                    if let Some(h) = handle {
                        entries.push(BindGroupEntry {
                            path: binding.path.clone(),
                            resource: ResourceBinding::Image(h),
                        });
                    }
                }
                BindingKind::Sampler => {
                    let handle = samplers_by_name
                        .get(&binding.path)
                        .copied()
                        .unwrap_or_else(|| engine.default_sampler());
                    entries.push(BindGroupEntry {
                        path: binding.path.clone(),
                        resource: ResourceBinding::Sampler(handle),
                    });
                }
                _ => {}
            }
        }
    }

    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let bind_group = engine.create_bind_group(BindGroupDesc {
        layout: layout.handle(),
        entries,
    })?;
    Ok(vec![bind_group])
}
