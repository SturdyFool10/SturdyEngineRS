use std::{cell::RefCell, collections::HashMap, path::PathBuf, rc::Rc, sync::Mutex};

use sturdy_engine_core as core;

use crate::{
    Access, BindGroup, BindGroupDesc, BindGroupEntry, BindingKind, Buffer, BufferDesc, BufferUsage,
    ColorTargetDesc, CullMode, DispatchDesc, DrawDesc, Engine, Error, Format, FrontFace,
    GraphicsPipelineDesc, ImageDesc, ImageHandle, ImageRef, IndexBufferBinding, PassDesc, PassWork,
    Pipeline, PipelineLayout, PrimitiveTopology, PushConstants, QueueType, RasterState,
    ResourceBinding, Result, RgState, Shader, ShaderDesc, ShaderReflection, ShaderSource,
    ShaderStage, StageMask, SubresourceRange, SurfaceImage, VertexAttributeDesc,
    VertexBufferBinding, VertexBufferLayout, VertexFormat, VertexInputRate,
    compute_program::ComputeProgram, mesh::Mesh, mesh_program::MeshProgram,
    sampler_catalog::SamplerPreset,
};

const FULLSCREEN_VERTEX_SHADER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/fullscreen_vertex.slang"
));

const PASSTHROUGH_FRAGMENT_SHADER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/passthrough_fragment.slang"
));

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

    /// Returns true when `other` targets the same logical image (same name and
    /// swapchain slot) but with a different descriptor — i.e. the cached entry
    /// is stale and should be evicted before inserting `other`.
    pub(crate) fn is_stale_for(&self, other: &Self) -> bool {
        self.name == other.name
            && self.swapchain_slot == other.swapchain_slot
            && self.desc_key != other.desc_key
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

/// The kind of render pass recorded in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassKind {
    /// Fullscreen triangle pass driven by a fragment shader.
    Fullscreen,
    /// Compute dispatch pass.
    Compute,
    /// Mesh draw pass.
    Mesh,
}

struct PassRecord {
    name: String,
    kind: PassKind,
    reads: Vec<core::ImageHandle>,
    writes: Vec<core::ImageHandle>,
}

/// Per-pass information returned by [`RenderFrame::describe`].
pub struct GraphPassInfo {
    pub name: String,
    pub kind: PassKind,
    /// Names of frame images read by this pass.
    pub reads: Vec<String>,
    /// Names of frame images written by this pass.
    pub writes: Vec<String>,
}

/// Per-image information returned by [`RenderFrame::describe`].
pub struct GraphImageInfo {
    pub name: String,
    pub format: Format,
    pub extent: core::Extent3d,
    pub write_count: usize,
    pub read_count: usize,
}

/// A snapshot of the render graph recorded so far in a [`RenderFrame`].
pub struct GraphReport {
    pub passes: Vec<GraphPassInfo>,
    pub images: Vec<GraphImageInfo>,
}

/// Severity level of a graph diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Warning,
    Error,
}

/// A diagnostic produced by [`RenderFrame::validate`].
pub struct GraphDiagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
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
    stage: ShaderStage,
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

    /// Create a compute `ShaderProgram` from an inline Slang source string.
    pub fn from_inline_compute(engine: &Engine, source: &str) -> Result<Self> {
        Self::new(
            engine,
            ShaderProgramDesc {
                vertex: None,
                fragment: ShaderDesc {
                    source: ShaderSource::Inline(source.to_owned()),
                    entry_point: "main".to_owned(),
                    stage: ShaderStage::Compute,
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
                Error::Unknown(format!(
                    "failed to read SPIR-V file {}: {e}",
                    path.display()
                ))
            })?;
            ShaderSource::Spirv(crate::spirv_words_from_bytes(&bytes).map_err(|e| {
                Error::Unknown(format!("invalid SPIR-V in {}: {e}", path.display()))
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

    /// Create a passthrough shader that samples `source` and writes it unchanged.
    ///
    /// Use this with [`GraphImage::blit_from`] to implement copy/resolve passes
    /// without writing a custom shader. The shader expects a frame image named
    /// `"source"` — call `src.register_as("source")` before `blit_from` if the
    /// source image was registered under a different name.
    pub fn passthrough(engine: &Engine) -> Result<Self> {
        Self::from_inline_fragment(engine, PASSTHROUGH_FRAGMENT_SHADER)
    }

    /// Load a compute shader from `path`.
    pub fn load_compute(engine: &Engine, path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let source = if path.extension().and_then(|e| e.to_str()) == Some("spv") {
            let bytes = std::fs::read(&path).map_err(|e| {
                Error::Unknown(format!(
                    "failed to read SPIR-V file {}: {e}",
                    path.display()
                ))
            })?;
            ShaderSource::Spirv(crate::spirv_words_from_bytes(&bytes).map_err(|e| {
                Error::Unknown(format!("invalid SPIR-V in {}: {e}", path.display()))
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
                    stage: ShaderStage::Compute,
                },
            },
        )
    }

    pub fn new(engine: &Engine, desc: ShaderProgramDesc) -> Result<Self> {
        let vertex = engine.create_shader(desc.vertex.unwrap_or_else(default_vertex_desc))?;
        let fragment_stage = desc.fragment.stage;
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
            stage: fragment_stage,
        })
    }

    pub fn reflection(&self) -> &ShaderReflection {
        &self.reflection
    }

    /// Returns the shader stage for this program (Vertex, Fragment, or Compute).
    pub fn stage(&self) -> ShaderStage {
        self.stage
    }

    /// Returns the `StageMask` corresponding to the reflected shader stage.
    ///
    /// This is useful for [`GraphImage::execute_shader_auto`] which infers the
    /// stage from reflection instead of requiring the caller to pass it.
    pub fn stage_mask(&self) -> StageMask {
        match self.stage {
            ShaderStage::Vertex => StageMask::VERTEX,
            ShaderStage::Fragment => StageMask::FRAGMENT,
            ShaderStage::Compute => StageMask::COMPUTE,
            ShaderStage::Mesh => StageMask::MESH,
            ShaderStage::Task => StageMask::TASK,
            ShaderStage::RayGeneration => StageMask::RAY_TRACING,
            ShaderStage::Miss => StageMask::RAY_TRACING,
            ShaderStage::ClosestHit => StageMask::RAY_TRACING,
        }
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
    pass_records: Vec<PassRecord>,
    /// Passes queued for submission. Flushed through the scheduler on `flush()`.
    pending_passes: Vec<PassDesc>,
    declaration_index: u32,
    swapchain_slot: u64,
    flushed: bool,
    swapchain_extent: core::Extent3d,
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
                pass_records: Vec::new(),
                pending_passes: Vec::new(),
                declaration_index: 0,
                swapchain_slot,
                flushed: false,
                swapchain_extent: core::Extent3d::default(),
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
        inner.swapchain_extent = image.desc().extent;
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

    /// Return a snapshot of every pass and image recorded in this frame so far.
    pub fn describe(&self) -> GraphReport {
        let inner = self.inner.borrow();

        let handle_to_name = |h: core::ImageHandle| -> Option<String> {
            inner
                .images_by_name
                .iter()
                .find(|(_, rec)| rec.handle == h)
                .map(|(name, _)| name.clone())
        };

        let passes = inner
            .pass_records
            .iter()
            .map(|rec| GraphPassInfo {
                name: rec.name.clone(),
                kind: rec.kind,
                reads: rec.reads.iter().filter_map(|h| handle_to_name(*h)).collect(),
                writes: rec.writes.iter().filter_map(|h| handle_to_name(*h)).collect(),
            })
            .collect();

        let mut write_counts: Vec<(core::ImageHandle, usize)> = Vec::new();
        let mut read_counts: Vec<(core::ImageHandle, usize)> = Vec::new();
        let tally = |counts: &mut Vec<(core::ImageHandle, usize)>, h: core::ImageHandle| {
            if let Some(entry) = counts.iter_mut().find(|(k, _)| *k == h) {
                entry.1 += 1;
            } else {
                counts.push((h, 1));
            }
        };
        for rec in &inner.pass_records {
            for h in &rec.reads {
                tally(&mut read_counts, *h);
            }
            for h in &rec.writes {
                tally(&mut write_counts, *h);
            }
        }

        let images = inner
            .images_by_name
            .iter()
            .map(|(name, rec)| {
                let write_count = write_counts
                    .iter()
                    .find(|(h, _)| *h == rec.handle)
                    .map(|(_, c)| *c)
                    .unwrap_or(0);
                let read_count = read_counts
                    .iter()
                    .find(|(h, _)| *h == rec.handle)
                    .map(|(_, c)| *c)
                    .unwrap_or(0);
                GraphImageInfo {
                    name: name.clone(),
                    format: rec.desc.format,
                    extent: rec.desc.extent,
                    write_count,
                    read_count,
                }
            })
            .collect();

        GraphReport { passes, images }
    }

    /// Validate the recorded graph and return any diagnostics.
    ///
    /// Checks for:
    /// - Write-after-write on the same image with no intervening read.
    /// - Images written but never subsequently read (potential unused output).
    pub fn validate(&self) -> Vec<GraphDiagnostic> {
        let inner = self.inner.borrow();
        let mut diagnostics = Vec::new();

        let handle_to_name = |h: core::ImageHandle| -> &str {
            inner
                .images_by_name
                .iter()
                .find(|(_, rec)| rec.handle == h)
                .map(|(name, _)| name.as_str())
                .unwrap_or("<unknown>")
        };

        // Track consecutive-write state: (handle, last-writing-pass-name)
        let mut pending_writes: Vec<(core::ImageHandle, String)> = Vec::new();
        let mut ever_read: Vec<core::ImageHandle> = Vec::new();

        for rec in &inner.pass_records {
            // Reads clear pending write state for those images.
            for h in &rec.reads {
                pending_writes.retain(|(k, _)| k != h);
                if !ever_read.contains(h) {
                    ever_read.push(*h);
                }
            }

            // Writes: flag if the same image is still pending from a previous pass.
            for h in &rec.writes {
                if let Some(pos) = pending_writes.iter().position(|(k, _)| k == h) {
                    let (_, prev_pass) = pending_writes.remove(pos);
                    diagnostics.push(GraphDiagnostic {
                        level: DiagnosticLevel::Warning,
                        message: format!(
                            "image '{}' is written in '{}' and again in '{}' without an intervening read (write-after-write)",
                            handle_to_name(*h),
                            prev_pass,
                            rec.name,
                        ),
                    });
                }
                pending_writes.push((*h, rec.name.clone()));
            }
        }

        // Any image still pending a read that is not "swapchain" is a potential unused output.
        for (h, pass_name) in &pending_writes {
            let name = handle_to_name(*h);
            if name == "swapchain" {
                continue;
            }
            if !ever_read.contains(h) {
                diagnostics.push(GraphDiagnostic {
                    level: DiagnosticLevel::Warning,
                    message: format!(
                        "image '{name}' is written in '{pass_name}' but never read — may be an unused output"
                    ),
                });
            }
        }

        diagnostics
    }

    /// Create a graph image with the same dimensions as `src` but a different format.
    ///
    /// Useful for allocating intermediate images that share a source image's
    /// resolution (e.g. a depth image at the same size as an HDR color buffer).
    pub fn image_sized_to(
        &self,
        name: impl Into<String>,
        format: Format,
        src: &GraphImage,
    ) -> Result<GraphImage> {
        let desc = ImageDesc {
            format,
            usage: crate::ImageUsage::SAMPLED | crate::ImageUsage::RENDER_TARGET,
            ..src.desc()
        };
        self.image(name, desc)
    }

    /// Create a graph image at a fraction of `src`'s dimensions, with the same format.
    ///
    /// `divisor` is clamped to at least 1. Each dimension is divided by `divisor`
    /// and floored to at least 1 pixel. Use this to build downsample chains.
    pub fn image_at_fraction(
        &self,
        name: impl Into<String>,
        src: &GraphImage,
        divisor: u32,
    ) -> Result<GraphImage> {
        let divisor = divisor.max(1);
        let src_desc = src.desc();
        let desc = ImageDesc {
            extent: core::Extent3d {
                width: (src_desc.extent.width / divisor).max(1),
                height: (src_desc.extent.height / divisor).max(1),
                depth: src_desc.extent.depth,
            },
            ..src_desc
        };
        self.image(name, desc)
    }

    pub fn flush(&self) -> Result<core::SubmissionHandle> {
        let mut inner = self.inner.borrow_mut();
        inner.flushed = true;
        submit_pending_passes(&mut inner)?;
        inner.frame.flush()
    }

    pub fn wait(&self) -> Result<()> {
        self.inner.borrow().frame.wait()
    }

    pub fn present_image(&self, image: &GraphImage) -> Result<()> {
        let mut inner = self.inner.borrow_mut();
        inner
            .frame
            .inner
            .graph_mut(|g| g.import_image(image.handle(), image.desc()))?;
        inner.pending_passes.push(PassDesc {
            name: "present".to_owned(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            work: PassWork::None,
            reads: vec![crate::ImageUse {
                image: image.handle(),
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
        });
        Ok(())
    }

    /// Create a swapchain-sized FP16 HDR color image for rendering.
    ///
    /// This is a convenience method that replaces the common pattern:
    /// ```ignore
    /// let desc = ImageBuilder::new_2d(Format::Rgba16Float, width, height)
    ///     .role(ImageRole::ColorAttachment)
    ///     .build()?;
    /// let image = frame.image("name", desc)?;
    /// ```
    ///
    /// The image is sized to match the current swapchain image dimensions.
    pub fn hdr_color_image(&self, name: impl Into<String>) -> Result<GraphImage> {
        let mut inner = self.inner.borrow_mut();
        let extent = inner.swapchain_extent;
        if extent.width == 0 && extent.height == 0 {
            return Err(Error::InvalidInput(
                "no swapchain image available for sizing".to_string(),
            ));
        }

        let desc = ImageDesc {
            dimension: crate::ImageDimension::D2,
            extent,
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Rgba16Float,
            usage: crate::ImageUsage::SAMPLED | crate::ImageUsage::RENDER_TARGET,
            transient: false,
            clear_value: None,
            debug_name: None,
        };
        let name = name.into();
        let key = GraphImageCacheKey::new(name.clone(), desc, inner.swapchain_slot);
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

    /// Create an HDR image sized to a specific surface image.
    ///
    /// This is a more flexible variant of [`hdr_color_image`](Self::hdr_color_image)
    /// that allows specifying a custom format and explicitly providing the source
    /// surface image for sizing.
    pub fn hdr_image_sized_to(
        &self,
        name: impl Into<String>,
        format: Format,
        surface_image: &SurfaceImage,
    ) -> Result<GraphImage> {
        let mut inner = self.inner.borrow_mut();
        let slot = inner.swapchain_slot;

        let desc = ImageDesc {
            dimension: crate::ImageDimension::D2,
            extent: surface_image.desc().extent,
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format,
            usage: crate::ImageUsage::SAMPLED | crate::ImageUsage::RENDER_TARGET,
            transient: false,
            clear_value: None,
            debug_name: None,
        };
        let name = name.into();
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
        let _ = submit_pending_passes(&mut inner);
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

    /// Execute this image as the target of a fullscreen pass, inferring the
    /// shader stage from reflection instead of requiring the caller to pass it.
    ///
    /// Falls back to `FRAGMENT` for programs whose reflection does not expose
    /// a stage.  Keeps the explicit-stage variants for callers that need to
    /// override the inferred stage.
    pub fn execute_shader_auto(&self, shader: &ShaderProgram) -> Result<()> {
        let stages = if shader.reflection().entry_points.is_empty() {
            StageMask::FRAGMENT
        } else {
            shader.stage_mask()
        };
        self.execute_shader_with_push_constants(shader, stages, &[])
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

    /// Typed variant of [`execute_shader_with_push_constants`] that accepts any
    /// `bytemuck::Pod` value directly, eliminating the need for unsafe byte-casting at call sites.
    pub fn execute_shader_with_constants<T: bytemuck::Pod>(
        &self,
        shader: &ShaderProgram,
        stages: StageMask,
        constants: &T,
    ) -> Result<()> {
        self.execute_shader_with_push_constants(shader, stages, bytemuck::bytes_of(constants))
    }

    /// Typed variant of [`execute_shader_auto`] that infers the stage from
    /// shader reflection and accepts a `bytemuck::Pod` push constant directly.
    pub fn execute_shader_with_constants_auto<T: bytemuck::Pod>(
        &self,
        shader: &ShaderProgram,
        constants: &T,
    ) -> Result<()> {
        let stages = if shader.reflection().entry_points.is_empty() {
            StageMask::FRAGMENT
        } else {
            shader.stage_mask()
        };
        self.execute_shader_with_push_constants(shader, stages, bytemuck::bytes_of(constants))
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

        let pass_name = format!("{declaration_index:04}-execute-{}", self.name);
        let pass_reads: Vec<core::ImageHandle> = reads.iter().map(|u| u.image).collect();
        inner.pass_records.push(PassRecord {
            name: pass_name.clone(),
            kind: PassKind::Fullscreen,
            reads: pass_reads,
            writes: vec![self.handle],
        });

        inner.pending_passes.push(PassDesc {
            name: pass_name,
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
        });
        Ok(())
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

        let pass_name = format!("{declaration_index:04}-draw-mesh-{}", self.name);
        inner.pass_records.push(PassRecord {
            name: pass_name.clone(),
            kind: PassKind::Mesh,
            reads: Vec::new(),
            writes: vec![self.handle],
        });

        inner.pending_passes.push(PassDesc {
            name: pass_name,
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
        });
        Ok(())
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

    /// Copy `src` into this image via the built-in passthrough shader.
    ///
    /// `src` is temporarily registered as `"source"` in the frame image registry
    /// before the pass is recorded.  Any prior `"source"` registration is
    /// overwritten and restored to nothing after this call, so avoid relying on
    /// a frame image named `"source"` when using `blit_from`.
    ///
    /// `passthrough` must be a [`ShaderProgram::passthrough`] program (or any
    /// shader that reads a frame image named `"source"`).
    pub fn blit_from(&self, src: &GraphImage, passthrough: &ShaderProgram) -> Result<()> {
        src.register_as("source");
        self.execute_shader_auto(passthrough)
    }

    pub fn execute_compute(&self, program: &ComputeProgram, groups: [u32; 3]) -> Result<()> {
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

        let pass_name = format!("{declaration_index:04}-compute-{}", self.name);
        let pass_reads: Vec<core::ImageHandle> = reads.iter().map(|u| u.image).collect();
        inner.pass_records.push(PassRecord {
            name: pass_name.clone(),
            kind: PassKind::Compute,
            reads: pass_reads,
            writes: vec![self.handle],
        });

        inner.pending_passes.push(PassDesc {
            name: pass_name,
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
        });
        Ok(())
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

/// Drain `inner.pending_passes`, schedule them, and submit to the core frame.
fn submit_pending_passes(inner: &mut RenderFrameInner) -> Result<()> {
    if inner.pending_passes.is_empty() {
        return Ok(());
    }
    let order = schedule_pass_order(&inner.pending_passes);
    // Use Option::take to move each PassDesc out without requiring Clone.
    let mut slots: Vec<Option<PassDesc>> =
        inner.pending_passes.drain(..).map(Some).collect();
    for idx in order {
        let pass = slots[idx].take().expect("scheduler produced duplicate index");
        inner.frame.add_pass(pass)?;
    }
    Ok(())
}

/// Returns true if `later` must execute after `earlier` due to a shared resource.
///
/// Checks all three hazard types on image and buffer resources:
/// - RAW (read-after-write): `earlier` writes X, `later` reads X
/// - WAW (write-after-write): `earlier` writes X, `later` writes X
/// - WAR (write-after-read): `earlier` reads X, `later` writes X
fn has_resource_dependency(earlier: &PassDesc, later: &PassDesc) -> bool {
    let e_img_writes: Vec<_> = earlier.writes.iter().map(|u| u.image).collect();
    let e_img_reads: Vec<_> = earlier.reads.iter().map(|u| u.image).collect();
    let l_img_writes: Vec<_> = later.writes.iter().map(|u| u.image).collect();
    let l_img_reads: Vec<_> = later.reads.iter().map(|u| u.image).collect();

    // RAW and WAW
    for h in &e_img_writes {
        if l_img_reads.contains(h) || l_img_writes.contains(h) {
            return true;
        }
    }
    // WAR
    for h in &e_img_reads {
        if l_img_writes.contains(h) {
            return true;
        }
    }

    // Buffer hazards
    let e_buf_writes: Vec<_> = earlier.buffer_writes.iter().map(|u| u.buffer).collect();
    let e_buf_reads: Vec<_> = earlier.buffer_reads.iter().map(|u| u.buffer).collect();
    let l_buf_writes: Vec<_> = later.buffer_writes.iter().map(|u| u.buffer).collect();
    let l_buf_reads: Vec<_> = later.buffer_reads.iter().map(|u| u.buffer).collect();

    for h in &e_buf_writes {
        if l_buf_reads.contains(h) || l_buf_writes.contains(h) {
            return true;
        }
    }
    for h in &e_buf_reads {
        if l_buf_writes.contains(h) {
            return true;
        }
    }

    false
}

/// Returns the indices of `passes` in dependency-correct execution order.
///
/// Uses Kahn's topological sort.  Passes with no outstanding dependencies are
/// processed in declaration order (their original index) as a deterministic
/// tie-breaker, preserving the user's intent for truly independent passes.
///
/// If a cycle is detected (which should not occur in a valid render graph) the
/// remaining passes are appended in declaration order.
fn schedule_pass_order(passes: &[PassDesc]) -> Vec<usize> {
    let n = passes.len();
    if n <= 1 {
        return (0..n).collect();
    }

    // Build forward adjacency: adj[i] lists every j that depends on i.
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut in_degree: Vec<usize> = vec![0; n];

    for i in 0..n {
        for j in 0..n {
            if i == j {
                continue;
            }
            if has_resource_dependency(&passes[i], &passes[j])
                && !adj[i].contains(&j)
            {
                adj[i].push(j);
                in_degree[j] += 1;
            }
        }
    }

    // Kahn's algorithm — sort each wave by original index for determinism.
    let mut ready: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
    let mut result = Vec::with_capacity(n);

    while !ready.is_empty() {
        ready.sort_unstable();
        let wave = std::mem::take(&mut ready);
        for idx in wave {
            result.push(idx);
            for &dep in &adj[idx] {
                in_degree[dep] -= 1;
                if in_degree[dep] == 0 {
                    ready.push(dep);
                }
            }
        }
    }

    // Cycle fallback: append remaining passes in declaration order.
    if result.len() < n {
        for i in 0..n {
            if in_degree[i] > 0 {
                result.push(i);
            }
        }
    }

    result
}

fn reflected_image_reads(reflection: &ShaderReflection) -> Vec<String> {
    reflected_bindings_of_kind(reflection, core::BindingKind::SampledImage)
}

fn reflected_storage_image_reads(reflection: &ShaderReflection) -> Vec<String> {
    reflected_bindings_of_kind(reflection, core::BindingKind::StorageImage)
}

fn reflected_bindings_of_kind(
    reflection: &ShaderReflection,
    kind: core::BindingKind,
) -> Vec<String> {
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
