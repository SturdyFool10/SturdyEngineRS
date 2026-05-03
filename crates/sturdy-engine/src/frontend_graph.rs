use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Mutex,
};

use sturdy_engine_core as core;

use crate::{
    Access, BindGroup, BindGroupDesc, BindGroupEntry, BindingKind, Buffer, BufferDesc, BufferUsage,
    BufferUse, ColorTargetDesc, CopyImageToBufferDesc, CullMode, DispatchDesc, DispatchIndirectDesc,
    DrawDesc, DrawIndirectDesc, Engine, Error, Format, FrontFace, GraphicsPipelineDesc, ImageDesc,
    ImageHandle, ImageRef, ImageUse, IndexBufferBinding, PassDesc, PassWork, Pipeline,
    PipelineLayout, PrimitiveTopology, PushConstants, QueueType, RasterState, ResolveImageDesc,
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
    /// Hardware image resolve pass.
    Resolve,
    /// Compute dispatch pass.
    Compute,
    /// Mesh draw pass.
    Mesh,
}

struct PassRecord {
    name: String,
    kind: PassKind,
    queue: core::QueueType,
    reads: Vec<crate::ImageUse>,
    writes: Vec<crate::ImageUse>,
    /// Named buffers read by this pass (for diagnostics / describe()).
    buffer_read_names: Vec<String>,
    /// Named buffers written by this pass (for diagnostics / describe()).
    buffer_write_names: Vec<String>,
    /// Deferred read names for passes whose bind groups are resolved at flush time.
    /// Resolved lazily by `validate()` and `describe()` against `images_by_name`.
    deferred_read_names: Vec<String>,
    skip_read_name: String,
}

/// Data needed to finish building a pass at flush time when all frame images are registered.
///
/// Reads split into two buckets at declaration time:
/// - `eager_bindings`: names that were already registered (via `register_as` or earlier
///   `frame.image` calls) — captured with the correct handle at declaration time so that
///   re-used alias names like `"source_tex"` don't get overwritten by later declarations.
/// - `unresolved_read_names`: names not yet registered — resolved against `images_by_name`
///   at flush time when all frame images exist.
struct DeferredPassResolve {
    layout_handle: core::PipelineLayoutHandle,
    reflection: ShaderReflection,
    /// Binding names captured with their correct handles at declaration time.
    /// Preferred over `images_by_name` when building the bind group at flush time.
    eager_bindings: HashMap<String, ImageBinding>,
    /// Per-pass sampler overrides captured by a shader intent.
    eager_samplers: HashMap<String, core::SamplerHandle>,
    /// Per-pass buffer overrides captured by a shader intent.
    eager_buffers: HashMap<String, (core::BufferHandle, crate::BufferDesc)>,
    /// Binding names that could not be resolved at declaration time.
    /// Appended to `PassDesc.reads` and the bind group at flush time.
    unresolved_read_names: Vec<String>,
    /// Name of the output image — excluded from the read list.
    skip_name: String,
    /// For compute passes: the storage-image output bound explicitly.
    storage_output: Option<(String, ImageHandle)>,
}

#[derive(Clone, Default)]
struct ExplicitPassResources {
    images: HashMap<String, ImageBinding>,
    samplers: HashMap<String, core::SamplerHandle>,
    buffers: HashMap<String, (core::BufferHandle, crate::BufferDesc)>,
}

/// A pass queued for deferred scheduling and submission.
struct PendingPass {
    desc: PassDesc,
    /// If Some, `desc.reads` and `desc.bind_groups` are incomplete until flush.
    deferred: Option<DeferredPassResolve>,
}

/// Per-pass information returned by [`RenderFrame::describe`].
pub struct GraphPassInfo {
    pub name: String,
    pub kind: PassKind,
    /// GPU queue this pass executes on.
    pub queue: core::QueueType,
    /// Names of frame images read by this pass.
    pub reads: Vec<String>,
    /// Names of frame images written by this pass.
    pub writes: Vec<String>,
    /// Names of buffers read by this pass.
    pub buffer_reads: Vec<String>,
    /// Names of buffers written by this pass.
    pub buffer_writes: Vec<String>,
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

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ShaderName(String);

impl ShaderName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SlangEntryPoints {
    Graphics { vertex: String, fragment: String },
    Fragment { fragment: String },
    Compute { compute: String },
}

impl SlangEntryPoints {
    pub fn graphics(vertex: impl Into<String>, fragment: impl Into<String>) -> Self {
        Self::Graphics {
            vertex: vertex.into(),
            fragment: fragment.into(),
        }
    }

    pub fn fragment(fragment: impl Into<String>) -> Self {
        Self::Fragment {
            fragment: fragment.into(),
        }
    }

    pub fn compute(compute: impl Into<String>) -> Self {
        Self::Compute {
            compute: compute.into(),
        }
    }
}

pub struct ShaderProgram {
    engine: Engine,
    pipelines: Mutex<HashMap<(Format, u8), Pipeline>>,
    pub(crate) pipeline_layout: PipelineLayout,
    vertex: Shader,
    fragment: Shader,
    fullscreen_triangle: Buffer,
    reflection: ShaderReflection,
    stage: ShaderStage,
    source_path: Option<PathBuf>,
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
            ShaderSource::File(path.clone())
        };
        let mut program = Self::new(
            engine,
            ShaderProgramDesc {
                vertex: None,
                fragment: ShaderDesc {
                    source,
                    entry_point: "main".to_owned(),
                    stage: ShaderStage::Fragment,
                },
            },
        )?;
        if !path.extension().map_or(false, |e| e == "spv") {
            program.source_path = Some(path);
        }
        Ok(program)
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
            ShaderSource::File(path.clone())
        };
        let mut program = Self::new(
            engine,
            ShaderProgramDesc {
                vertex: None,
                fragment: ShaderDesc {
                    source,
                    entry_point: "main".to_owned(),
                    stage: ShaderStage::Compute,
                },
            },
        )?;
        if !path.extension().map_or(false, |e| e == "spv") {
            program.source_path = Some(path);
        }
        Ok(program)
    }

    pub fn new(engine: &Engine, desc: ShaderProgramDesc) -> Result<Self> {
        let vertex = engine.create_shader(desc.vertex.unwrap_or_else(default_vertex_desc))?;
        let fragment_stage = desc.fragment.stage;
        let fragment = engine.create_shader(desc.fragment)?;
        let (reflection, pipeline_layout) = if fragment_stage == ShaderStage::Compute {
            (
                engine.shader_reflection(&fragment)?,
                engine.create_reflected_compute_pipeline_layout(&fragment)?,
            )
        } else {
            (
                engine.graphics_shader_reflection(&vertex, Some(&fragment))?,
                engine.create_reflected_graphics_pipeline_layout(&vertex, Some(&fragment))?,
            )
        };
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
            source_path: None,
        })
    }

    pub fn reflection(&self) -> &ShaderReflection {
        &self.reflection
    }

    /// Return the source file path if this program was loaded from a file.
    pub fn source_path(&self) -> Option<&Path> {
        self.source_path.as_deref()
    }

    /// Recompile from the original source file and rebuild all cached pipelines.
    ///
    /// Returns `Ok(true)` on success, `Ok(false)` when there is no file path, and
    /// `Err` on compile failure. The previous pipeline remains active on failure.
    pub fn reload(&mut self) -> Result<bool> {
        let path = match &self.source_path {
            Some(p) => p.clone(),
            None => return Ok(false),
        };
        let fragment = self.engine.create_shader(ShaderDesc {
            source: ShaderSource::File(path),
            entry_point: "main".to_owned(),
            stage: self.stage,
        })?;
        let (reflection, pipeline_layout) = if self.stage == ShaderStage::Compute {
            (
                self.engine.shader_reflection(&fragment)?,
                self.engine
                    .create_reflected_compute_pipeline_layout(&fragment)?,
            )
        } else {
            (
                self.engine
                    .graphics_shader_reflection(&self.vertex, Some(&fragment))?,
                self.engine
                    .create_reflected_graphics_pipeline_layout(&self.vertex, Some(&fragment))?,
            )
        };
        self.fragment = fragment;
        self.reflection = reflection;
        self.pipeline_layout = pipeline_layout;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.pipelines
            .lock()
            .expect("shader program pipeline mutex poisoned")
            .clear();
        Ok(true)
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

    fn pipeline_handle(&self, format: Format, samples: u8) -> Result<core::PipelineHandle> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut pipelines = self
            .pipelines
            .lock()
            .expect("shader program pipeline mutex poisoned");
        let key = (format, samples.max(1));
        if !pipelines.contains_key(&key) {
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
                color_targets: vec![ColorTargetDesc::opaque(format)],
                depth_format: None,
                samples: key.1,
                topology: PrimitiveTopology::TriangleList,
                raster: RasterState {
                    cull_mode: CullMode::None,
                    front_face: FrontFace::CounterClockwise,
                },
            })?;
            pipeline.set_debug_name("reflected-fullscreen-program")?;
            pipelines.insert(key, pipeline);
        }
        pipelines
            .get(&key)
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
    buffers_by_name: HashMap<String, (core::BufferHandle, crate::BufferDesc)>,
    held_bind_groups: Vec<BindGroup>,
    pass_records: Vec<PassRecord>,
    /// Passes queued for submission. Flushed through the scheduler on `flush()`.
    pending_passes: Vec<PendingPass>,
    /// User-declared ordering constraints: (writer_of_before, writer_of_after).
    /// Any pass that writes `before` must execute before any pass that writes `after`.
    ordering_constraints: Vec<(ImageHandle, ImageHandle)>,
    /// Images explicitly imported from outside the frame graph (e.g. persistent CPU textures).
    /// These are intentionally read without a same-frame write; suppress the RBW validator warning.
    externally_imported: std::collections::HashSet<ImageHandle>,
    declaration_index: u32,
    swapchain_slot: u64,
    flushed: bool,
    /// Set when this frame has been presented (either via auto-present or explicit call).
    /// Prevents the `Drop` impl from double-presenting.
    presented: bool,
    /// If set, `Drop` will call `device.present_surface(handle)` automatically after flushing.
    /// Configured by `Engine::begin_frame_for_surface` so the caller doesn't have to.
    auto_present: Option<(core::Device, core::SurfaceHandle)>,
    /// Keeps a swapchain-acquired image alive for the duration of the frame.
    /// Needed when `Engine::begin_frame_for_surface` acquires the image internally.
    held_surface_image: Option<crate::SurfaceImage>,
    swapchain_extent: core::Extent3d,
}

#[derive(Copy, Clone)]
struct GraphImageRecord {
    handle: ImageHandle,
    desc: ImageDesc,
    subresource: SubresourceRange,
}

#[derive(Copy, Clone)]
struct ImageBinding {
    handle: ImageHandle,
    subresource: SubresourceRange,
}

fn single_subresource() -> SubresourceRange {
    SubresourceRange {
        base_mip: 0,
        mip_count: 1,
        base_layer: 0,
        layer_count: 1,
    }
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
                buffers_by_name: HashMap::new(),
                held_bind_groups: Vec::new(),
                pass_records: Vec::new(),
                pending_passes: Vec::new(),
                ordering_constraints: Vec::new(),
                externally_imported: std::collections::HashSet::new(),
                declaration_index: 0,
                swapchain_slot,
                flushed: false,
                presented: false,
                auto_present: None,
                held_surface_image: None,
                swapchain_extent: core::Extent3d::default(),
            })),
        })
    }

    pub fn engine(&self) -> Engine {
        self.inner.borrow().engine.clone()
    }

    /// Configure this frame to call `device.present_surface(handle)` automatically
    /// when it is dropped, after flushing all queued passes.
    pub(crate) fn configure_auto_present(
        &self,
        device: core::Device,
        handle: core::SurfaceHandle,
    ) {
        self.inner.borrow_mut().auto_present = Some((device, handle));
    }

    /// Mark this frame as already presented so `Drop` will not attempt a second present.
    pub(crate) fn mark_presented(&self) {
        self.inner.borrow_mut().presented = true;
    }

    /// Hold a surface image for the duration of this frame, ensuring it is not
    /// destroyed before the frame is flushed and presented.
    pub(crate) fn hold_surface_image(&self, image: crate::SurfaceImage) {
        self.inner.borrow_mut().held_surface_image = Some(image);
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
        inner.images_by_name.insert(
            name.clone(),
            GraphImageRecord {
                handle,
                desc,
                subresource: single_subresource(),
            },
        );
        Ok(GraphImage {
            frame: self.inner.clone(),
            name,
            handle,
            desc,
        })
    }

    pub fn swapchain_image(&self, image: &SurfaceImage) -> Result<GraphImage> {
        if !image.desc().usage.contains(crate::ImageUsage::PRESENT) {
            return Err(Error::InvalidInput(format!(
                "swapchain_image requires ImageUsage::PRESENT but image was created with {:?}",
                image.desc().usage
            )));
        }
        let name = "swapchain".to_owned();
        let mut inner = self.inner.borrow_mut();
        inner.frame.import_surface_image(image)?;
        inner.swapchain_extent = image.desc().extent;
        inner.images_by_name.insert(
            name.clone(),
            GraphImageRecord {
                handle: image.handle(),
                desc: image.desc(),
                subresource: single_subresource(),
            },
        );
        Ok(GraphImage {
            frame: self.inner.clone(),
            name,
            handle: image.handle(),
            desc: image.desc(),
        })
    }

    /// Register a pre-existing [`Image`] as a named frame image.
    ///
    /// Use this to make textures created outside the frame (e.g. via
    /// [`Engine::generate_texture_2d`]) visible to shaders by name.
    /// After calling this, any shader that declares a binding with the same
    /// name will receive this image.
    ///
    /// The image is not cached — it is re-registered every frame at the handle
    /// it was created with. Call this once per frame before the first
    /// `execute_shader` that needs it.
    pub fn import_image(
        &self,
        name: impl Into<String>,
        image: &crate::Image,
    ) -> Result<GraphImage> {
        let name = name.into();
        let mut inner = self.inner.borrow_mut();
        inner
            .frame
            .inner
            .graph_mut(|graph| graph.import_image(image.handle(), image.desc()))?;
        inner.images_by_name.insert(
            name.clone(),
            GraphImageRecord {
                handle: image.handle(),
                desc: image.desc(),
                subresource: single_subresource(),
            },
        );
        inner.externally_imported.insert(image.handle());
        Ok(GraphImage {
            frame: self.inner.clone(),
            name,
            handle: image.handle(),
            desc: image.desc(),
        })
    }

    /// Upload new CPU pixel data into an existing image and register it as a named frame image.
    ///
    /// `fill` receives `(x, y)` for each pixel and returns `[r, g, b, a]` as `u8`.
    /// Records a transfer pass into the frame before returning; any shader pass
    /// that reads the image will be scheduled after the upload.
    ///
    /// The image must have been created with `ImageUsage::COPY_DST` (images from
    /// [`Engine::generate_texture_2d`] satisfy this). Call once per frame before
    /// the first `execute_shader` that needs the updated data.
    pub fn update_texture_2d(
        &self,
        name: impl Into<String>,
        image: &crate::Image,
        fill: impl Fn(u32, u32) -> [u8; 4],
    ) -> Result<GraphImage> {
        let desc = image.desc();
        let w = desc.extent.width;
        let h = desc.extent.height;
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let rgba = fill(x, y);
                let i = ((y * w + x) * 4) as usize;
                pixels[i..i + 4].copy_from_slice(&rgba);
            }
        }
        let name = name.into();
        {
            let mut inner = self.inner.borrow_mut();
            inner
                .frame
                .upload_pixels_to_image(format!("update-{name}"), image, &pixels)?;
        }
        self.import_image(name, image)
    }

    /// Upload contiguous RGBA8 pixel bytes into an existing image and register
    /// it as a named frame image.
    pub fn update_texture_2d_pixels(
        &self,
        name: impl Into<String>,
        image: &crate::Image,
        pixels: &[u8],
    ) -> Result<GraphImage> {
        let desc = image.desc();
        let expected_len = desc
            .extent
            .width
            .saturating_mul(desc.extent.height)
            .saturating_mul(4) as usize;
        if pixels.len() != expected_len {
            return Err(crate::Error::InvalidInput(format!(
                "texture upload expected {expected_len} RGBA8 bytes for {}x{}, got {}",
                desc.extent.width,
                desc.extent.height,
                pixels.len()
            )));
        }
        let name = name.into();
        {
            let mut inner = self.inner.borrow_mut();
            inner
                .frame
                .upload_pixels_to_image(format!("update-{name}"), image, pixels)?;
        }
        self.import_image(name, image)
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

    /// Register a GPU buffer under a name for the current frame.
    ///
    /// When the engine auto-creates bind groups from shader reflection, any
    /// `StructuredBuffer` or `RWStructuredBuffer` binding whose variable name
    /// matches `name` will receive this buffer. Call this before the first
    /// `draw_mesh_instanced` (or other draw) that needs it.
    pub fn bind_buffer(&self, name: impl Into<String>, buffer: &crate::Buffer) -> &Self {
        let mut inner = self.inner.borrow_mut();
        inner
            .buffers_by_name
            .insert(name.into(), (buffer.handle(), buffer.desc()));
        self
    }

    /// Start a reflected shader pass intent.
    ///
    /// The intent builder records a fullscreen or compute pass while deriving
    /// queue choice, bind groups, resource reads/writes, and push-constant
    /// stages from shader reflection where possible.
    pub fn shader_pass(&self, name: impl Into<String>) -> ShaderPassIntent<'_> {
        ShaderPassIntent::new(self, name.into())
    }

    /// Declare that the pass writing `before` must execute before the pass writing `after`.
    ///
    /// Use this when resource dependencies cannot be inferred from reads and writes alone —
    /// for example, when two passes write to different images but must run in a specific
    /// order for correctness (e.g. a shadow map pass before a lighting pass that uses
    /// a different input image).
    ///
    /// The constraint is a no-op if either image has no writer in the current frame.
    pub fn order_before(&self, before: &GraphImage, after: &GraphImage) {
        self.inner
            .borrow_mut()
            .ordering_constraints
            .push((before.handle(), after.handle()));
    }

    /// Look up a named graph image registered in this frame.
    ///
    /// Returns `None` if no image with that name exists in the current frame.
    /// Use this together with [`ScreenshotCapture`] to read back a specific named
    /// image after the frame has been flushed and waited on.
    pub fn find_image_by_name(&self, name: &str) -> Option<GraphImage> {
        let inner = self.inner.borrow();
        let rec = *inner.images_by_name.get(name)?;
        drop(inner);
        Some(GraphImage {
            frame: self.inner.clone(),
            name: name.to_owned(),
            handle: rec.handle,
            desc: rec.desc,
        })
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

        // Resolve effective read names for a record, including deferred names.
        let effective_read_names = |rec: &PassRecord| -> Vec<String> {
            let mut names: Vec<String> = rec
                .reads
                .iter()
                .filter_map(|use_| handle_to_name(use_.image))
                .collect();
            for n in &rec.deferred_read_names {
                if *n != rec.skip_read_name && !names.contains(n) {
                    names.push(n.clone());
                }
            }
            names
        };

        let passes = inner
            .pass_records
            .iter()
            .map(|rec| GraphPassInfo {
                name: rec.name.clone(),
                kind: rec.kind,
                queue: rec.queue,
                reads: effective_read_names(rec),
                writes: rec
                    .writes
                    .iter()
                    .filter_map(|use_| handle_to_name(use_.image))
                    .collect(),
                buffer_reads: rec.buffer_read_names.clone(),
                buffer_writes: rec.buffer_write_names.clone(),
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
        let name_to_handle = |n: &str| -> Option<core::ImageHandle> {
            inner.images_by_name.get(n).map(|r| r.handle)
        };
        for rec in &inner.pass_records {
            for use_ in &rec.reads {
                tally(&mut read_counts, use_.image);
            }
            for n in &rec.deferred_read_names {
                if *n != rec.skip_read_name {
                    if let Some(h) = name_to_handle(n) {
                        tally(&mut read_counts, h);
                    }
                }
            }
            for use_ in &rec.writes {
                tally(&mut write_counts, use_.image);
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

        // Track consecutive-write state: (image use, last-writing-pass-name)
        let mut pending_writes: Vec<(crate::ImageUse, String)> = Vec::new();
        let mut ever_read: Vec<crate::ImageUse> = Vec::new();

        let name_to_use = |n: &str| -> Option<crate::ImageUse> {
            inner.images_by_name.get(n).map(|r| crate::ImageUse {
                image: r.handle,
                access: Access::Read,
                state: RgState::ShaderRead,
                subresource: r.subresource,
            })
        };

        for rec in &inner.pass_records {
            // Collect effective reads: resolved handles + deferred name lookups.
            let mut effective_reads: Vec<crate::ImageUse> = rec.reads.clone();
            for n in &rec.deferred_read_names {
                if *n != rec.skip_read_name {
                    if let Some(use_) = name_to_use(n) {
                        if !effective_reads
                            .iter()
                            .any(|existing| image_uses_overlap(existing, &use_))
                        {
                            effective_reads.push(use_);
                        }
                    }
                }
            }

            // Reads clear pending write state for those images.
            for read in &effective_reads {
                pending_writes.retain(|(write, _)| !image_uses_overlap(write, read));
                if !ever_read
                    .iter()
                    .any(|existing| image_uses_overlap(existing, read))
                {
                    ever_read.push(*read);
                }
            }

            // Writes: flag if the same image is still pending from a previous pass.
            for write in &rec.writes {
                if let Some(pos) = pending_writes
                    .iter()
                    .position(|(pending, _)| image_uses_overlap(pending, write))
                {
                    let (_, prev_pass) = pending_writes.remove(pos);
                    diagnostics.push(GraphDiagnostic {
                        level: DiagnosticLevel::Warning,
                        message: format!(
                            "image '{}' is written in '{}' and again in '{}' without an intervening read (write-after-write)",
                            handle_to_name(write.image),
                            prev_pass,
                            rec.name,
                        ),
                    });
                }
                pending_writes.push((*write, rec.name.clone()));
            }
        }

        // Any image still pending a read that is not "swapchain" is a potential unused output.
        for (write, pass_name) in &pending_writes {
            let name = handle_to_name(write.image);
            if name == "swapchain" {
                continue;
            }
            if !ever_read.iter().any(|read| image_uses_overlap(write, read)) {
                diagnostics.push(GraphDiagnostic {
                    level: DiagnosticLevel::Warning,
                    message: format!(
                        "image '{name}' is written in '{pass_name}' but never read — may be an unused output"
                    ),
                });
            }
        }

        // Collect all images written at least once this frame.
        let ever_written: Vec<crate::ImageUse> = inner
            .pass_records
            .iter()
            .flat_map(|rec| rec.writes.iter().copied())
            .collect();

        // Warn about reads of images that are never written in this frame.
        // Persistent images carry data from the previous frame, so this is a
        // warning rather than an error, but it often indicates a missing pass.
        for rec in &inner.pass_records {
            let mut effective_reads: Vec<crate::ImageUse> = rec.reads.clone();
            for n in &rec.deferred_read_names {
                if *n != rec.skip_read_name {
                    if let Some(use_) = name_to_use(n) {
                        if !effective_reads
                            .iter()
                            .any(|existing| image_uses_overlap(existing, &use_))
                        {
                            effective_reads.push(use_);
                        }
                    }
                }
            }
            for read in &effective_reads {
                let name = handle_to_name(read.image);
                if name == "swapchain" {
                    continue;
                }
                if inner.externally_imported.contains(&read.image) {
                    continue;
                }
                if !ever_written
                    .iter()
                    .any(|write| image_uses_overlap(write, read))
                {
                    diagnostics.push(GraphDiagnostic {
                        level: DiagnosticLevel::Warning,
                        message: format!(
                            "pass '{}' reads image '{name}' but no pass in this frame writes to it — reading previous frame data",
                            rec.name,
                        ),
                    });
                }
            }
        }

        // Phase 11: pre-flight binding validation.
        // For passes with unresolved deferred read names, check that those names
        // exist in images_by_name now (after all declarations). If not, the pass
        // will fail at flush time — surface the error here for earlier diagnosis.
        for rec in &inner.pass_records {
            for name in &rec.deferred_read_names {
                if *name == rec.skip_read_name {
                    continue;
                }
                if !inner.images_by_name.contains_key(name.as_str()) {
                    diagnostics.push(GraphDiagnostic {
                        level: DiagnosticLevel::Error,
                        message: format!(
                            "pass '{}' requires image '{}' but it is not registered in this frame",
                            rec.name, name,
                        ),
                    });
                }
            }
        }
        for pending in &inner.pending_passes {
            let Some(deferred) = &pending.deferred else {
                continue;
            };
            diagnostics.extend(validate_deferred_reflected_resources(
                &pending.desc.name,
                deferred,
                &inner.images_by_name,
                &inner.buffers_by_name,
            ));
            if deferred.reflection.layout.push_constants_bytes > 0
                && pending.desc.push_constants.is_none()
            {
                diagnostics.push(GraphDiagnostic {
                    level: DiagnosticLevel::Warning,
                    message: format!(
                        "pass '{}' shader declares {} push constant bytes but no push constants were provided",
                        pending.desc.name,
                        deferred.reflection.layout.push_constants_bytes,
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

    /// Create a single-sample image with the same extent/format as `src`.
    pub fn resolve_target_sized_to(
        &self,
        name: impl Into<String>,
        src: &GraphImage,
    ) -> Result<GraphImage> {
        let src_desc = src.desc();
        let desc = ImageDesc {
            dimension: crate::ImageDimension::D2,
            extent: src_desc.extent,
            mip_levels: 1,
            layers: src_desc.layers,
            samples: 1,
            format: src_desc.format,
            usage: crate::ImageUsage::SAMPLED
                | crate::ImageUsage::RENDER_TARGET
                | crate::ImageUsage::COPY_DST,
            transient: false,
            clear_value: None,
            debug_name: Some("resolve-target"),
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

    pub fn flush_with_reason(
        &self,
        reason: crate::FrameSyncReason,
    ) -> Result<crate::FrameSyncReport> {
        let submission = self.flush()?;
        Ok(crate::FrameSyncReport::submitted(reason, submission))
    }

    pub fn wait(&self) -> Result<()> {
        self.inner.borrow().frame.wait()
    }

    pub fn wait_with_reason(
        &self,
        reason: crate::FrameSyncReason,
    ) -> Result<crate::FrameSyncReport> {
        let inner = self.inner.borrow();
        let submission = inner.frame.last_submission();
        inner.frame.wait()?;
        Ok(crate::FrameSyncReport::waited(
            reason,
            submission.is_some(),
            submission,
        ))
    }

    pub fn present_image(&self, image: &GraphImage) -> Result<()> {
        let mut inner = self.inner.borrow_mut();
        inner
            .frame
            .inner
            .graph_mut(|g| g.import_image(image.handle(), image.desc()))?;
        inner.pending_passes.push(PendingPass {
            desc: PassDesc {
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
                    subresource: single_subresource(),
                }],
                writes: Vec::new(),
                buffer_reads: Vec::new(),
                buffer_writes: Vec::new(),
                clear_colors: Vec::new(),
                clear_depth: None,
            },
            deferred: None,
        });
        Ok(())
    }

    /// Add a readback copy from a graph image into a CPU-readable buffer.
    pub fn copy_image_to_buffer(
        &self,
        name: impl Into<String>,
        image: &GraphImage,
        buffer: &Buffer,
        width: u32,
        height: u32,
    ) -> Result<()> {
        let mut inner = self.inner.borrow_mut();
        inner
            .frame
            .inner
            .graph_mut(|g| g.import_buffer(buffer.handle(), buffer.desc()))?;
        let image_handle: ImageHandle = image.handle();
        let buffer_handle: core::BufferHandle = buffer.handle();
        inner.pending_passes.push(PendingPass {
            desc: PassDesc {
                name: name.into(),
                queue: QueueType::Graphics,
                shader: None,
                pipeline: None,
                bind_groups: Vec::new(),
                push_constants: None,
                work: PassWork::CopyImageToBuffer(CopyImageToBufferDesc {
                    image: image_handle,
                    buffer: buffer_handle,
                    buffer_offset: 0,
                    mip_level: 0,
                    base_layer: 0,
                    layer_count: 1,
                    width,
                    height,
                    depth: 1,
                }),
                reads: vec![ImageUse {
                    image: image_handle,
                    access: Access::Read,
                    state: RgState::CopySrc,
                    subresource: single_subresource(),
                }],
                writes: Vec::new(),
                buffer_reads: Vec::new(),
                buffer_writes: vec![BufferUse {
                    buffer: buffer_handle,
                    access: Access::Write,
                    state: RgState::CopyDst,
                    offset: 0,
                    size: buffer.desc().size,
                }],
                clear_colors: Vec::new(),
                clear_depth: None,
            },
            deferred: None,
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
        self.hdr_color_image_with_samples(name, 1)
    }

    /// Create a swapchain-sized FP16 HDR color image for rendering with an explicit sample count.
    pub fn hdr_color_image_with_samples(
        &self,
        name: impl Into<String>,
        samples: u8,
    ) -> Result<GraphImage> {
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
            samples: samples
                .clamp(1, inner.engine.caps().max_color_sample_count.max(1))
                .min(16),
            format: Format::Rgba16Float,
            usage: crate::ImageUsage::SAMPLED
                | crate::ImageUsage::RENDER_TARGET
                | crate::ImageUsage::COPY_SRC,
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
        inner.images_by_name.insert(
            name.clone(),
            GraphImageRecord {
                handle,
                desc,
                subresource: single_subresource(),
            },
        );
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
        inner.images_by_name.insert(
            name.clone(),
            GraphImageRecord {
                handle,
                desc,
                subresource: single_subresource(),
            },
        );
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
        if inner.presented {
            return;
        }
        if !inner.flushed {
            let _ = submit_pending_passes(&mut inner);
            let _ = inner.frame.flush();
            inner.flushed = true;
        }
        if let Some((ref device, handle)) = inner.auto_present {
            let _ = device.present_surface(handle);
            inner.presented = true;
        }
    }
}

pub struct ShaderPassIntent<'a> {
    frame: &'a RenderFrame,
    name: String,
    target: Option<GraphImage>,
    resources: ExplicitPassResources,
    push_constants: Option<PushConstants>,
}

impl<'a> ShaderPassIntent<'a> {
    fn new(frame: &'a RenderFrame, name: String) -> Self {
        Self {
            frame,
            name,
            target: None,
            resources: ExplicitPassResources::default(),
            push_constants: None,
        }
    }

    pub fn target(mut self, image: &GraphImage) -> Self {
        self.target = Some(image.clone());
        self
    }

    pub fn image(mut self, name: impl Into<String>, image: &GraphImage) -> Self {
        self.resources.images.insert(
            name.into(),
            ImageBinding {
                handle: image.handle,
                subresource: single_subresource(),
            },
        );
        self
    }

    pub fn image_view(mut self, name: impl Into<String>, image: &GraphImageView) -> Self {
        self.resources.images.insert(
            name.into(),
            ImageBinding {
                handle: image.handle,
                subresource: image.subresource,
            },
        );
        self
    }

    pub fn sampler(mut self, name: impl Into<String>, preset: SamplerPreset) -> Self {
        let handle = self.frame.inner.borrow().engine.sampler_handle(preset);
        self.resources.samplers.insert(name.into(), handle);
        self
    }

    pub fn buffer(mut self, name: impl Into<String>, buffer: &crate::Buffer) -> Self {
        self.resources
            .buffers
            .insert(name.into(), (buffer.handle(), buffer.desc()));
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

    pub fn constants<T: bytemuck::Pod>(self, stages: StageMask, constants: &T) -> Self {
        self.push_constants(stages, bytemuck::bytes_of(constants))
    }

    pub fn fullscreen(self, program: &ShaderProgram) -> Result<()> {
        let target = self.target.ok_or_else(|| {
            Error::InvalidInput(format!(
                "shader pass '{}' needs a target image before fullscreen()",
                self.name
            ))
        })?;
        let push_constants = self.push_constants.map(|mut pc| {
            if pc.stages.0 == 0 {
                pc.stages =
                    reflected_push_constant_stages(program.reflection(), program.stage_mask());
            }
            pc
        });
        record_fullscreen_shader_pass(
            &self.frame.inner,
            Some(self.name),
            &target,
            program,
            push_constants,
            single_subresource(),
            self.resources,
        )
    }

    pub fn compute(self, program: &ComputeProgram, groups: [u32; 3]) -> Result<()> {
        let target = self.target.ok_or_else(|| {
            Error::InvalidInput(format!(
                "shader pass '{}' needs a target image before compute()",
                self.name
            ))
        })?;
        let push_constants = self.push_constants.map(|mut pc| {
            if pc.stages.0 == 0 {
                pc.stages =
                    reflected_push_constant_stages(program.reflection(), StageMask::COMPUTE);
            }
            pc
        });
        record_compute_shader_pass(
            &self.frame.inner,
            Some(self.name),
            &target,
            program,
            push_constants,
            groups,
            self.resources,
        )
    }

    pub fn constants_auto<T: bytemuck::Pod>(mut self, constants: &T) -> Self {
        self.push_constants = Some(PushConstants {
            offset: 0,
            stages: StageMask::default(),
            bytes: bytemuck::bytes_of(constants).to_vec(),
        });
        self
    }
}

#[derive(Clone)]
pub struct GraphImage {
    frame: Rc<RefCell<RenderFrameInner>>,
    name: String,
    handle: ImageHandle,
    desc: ImageDesc,
}

#[derive(Clone)]
pub struct GraphImageView {
    frame: Rc<RefCell<RenderFrameInner>>,
    name: String,
    handle: ImageHandle,
    desc: ImageDesc,
    subresource: SubresourceRange,
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

    pub fn whole_view(&self) -> GraphImageView {
        GraphImageView {
            frame: self.frame.clone(),
            name: self.name.clone(),
            handle: self.handle,
            desc: self.desc,
            subresource: SubresourceRange::WHOLE,
        }
    }

    pub fn mip(&self, mip_level: u16) -> Result<GraphImageView> {
        self.subresource_view(SubresourceRange::new(mip_level, 1, 0, self.desc.layers))
    }

    pub fn mip_range(&self, base_mip: u16, mip_count: u16) -> Result<GraphImageView> {
        self.subresource_view(SubresourceRange::new(
            base_mip,
            mip_count,
            0,
            self.desc.layers,
        ))
    }

    pub fn layer(&self, layer: u16) -> Result<GraphImageView> {
        self.subresource_view(SubresourceRange::new(0, self.desc.mip_levels, layer, 1))
    }

    pub fn layer_range(&self, base_layer: u16, layer_count: u16) -> Result<GraphImageView> {
        self.subresource_view(SubresourceRange::new(
            0,
            self.desc.mip_levels,
            base_layer,
            layer_count,
        ))
    }

    pub fn subresource_view(&self, subresource: SubresourceRange) -> Result<GraphImageView> {
        validate_subresource(self.desc, subresource)?;
        Ok(GraphImageView {
            frame: self.frame.clone(),
            name: self.name.clone(),
            handle: self.handle,
            desc: self.desc,
            subresource,
        })
    }

    pub fn execute_shader(&self, shader: &ShaderProgram) -> Result<()> {
        self.execute_shader_inner(shader, None, single_subresource())
    }

    /// Execute this image as the target of a fullscreen pass, inferring the
    /// shader stage from reflection instead of requiring the caller to pass it.
    ///
    /// Falls back to `FRAGMENT` for programs whose reflection does not expose
    /// a stage.  Keeps the explicit-stage variants for callers that need to
    /// override the inferred stage.
    pub fn execute_shader_auto(&self, shader: &ShaderProgram) -> Result<()> {
        self.execute_shader(shader)
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
            single_subresource(),
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
        let stages = reflected_push_constant_stages(shader.reflection(), shader.stage_mask());
        self.execute_shader_with_push_constants(shader, stages, bytemuck::bytes_of(constants))
    }

    fn execute_shader_inner(
        &self,
        shader: &ShaderProgram,
        push_constants: Option<PushConstants>,
        target_subresource: SubresourceRange,
    ) -> Result<()> {
        record_fullscreen_shader_pass(
            &self.frame,
            None,
            self,
            shader,
            push_constants,
            target_subresource,
            ExplicitPassResources::default(),
        )
    }

    pub fn draw_mesh(&self, mesh: &Mesh, program: &MeshProgram) -> Result<()> {
        self.draw_mesh_inner(mesh, program, None, None, 1, None)
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
            None,
            1,
            None,
        )
    }

    /// Draw `instance_count` instances of `mesh` using `program`.
    ///
    /// `instances` is a storage buffer (`BufferUsage::STORAGE`) whose elements
    /// match the `StructuredBuffer<InstanceData>` declaration in the vertex shader.
    /// The buffer must be named `"instances"` in the shader source; that name is
    /// registered in the frame so the auto-bind system can resolve it.
    pub fn draw_mesh_instanced(
        &self,
        mesh: &Mesh,
        program: &MeshProgram,
        instances: &crate::Buffer,
        instance_count: u32,
    ) -> Result<()> {
        self.draw_mesh_inner(
            mesh,
            program,
            None,
            Some((instances, instance_count)),
            instance_count,
            None,
        )
    }

    /// Like `draw_mesh_instanced` but with typed push constants (e.g. a camera matrix).
    pub fn draw_mesh_instanced_with_push_constants<T: bytemuck::Pod>(
        &self,
        mesh: &Mesh,
        program: &MeshProgram,
        instances: &crate::Buffer,
        instance_count: u32,
        constants: &T,
    ) -> Result<()> {
        self.draw_mesh_instanced_with_push_constants_and_depth(
            mesh,
            program,
            instances,
            instance_count,
            constants,
            None,
        )
    }

    /// Like `draw_mesh_instanced_with_push_constants` but with an explicit depth buffer.
    ///
    /// Required when the `MeshProgram` was created with `uses_depth: true` (e.g. `MeshProgram::unlit`
    /// or `MeshProgram::load_3d`). The depth image must have the same extent and sample count as
    /// `self` and use `Format::Depth32Float`.
    pub fn draw_mesh_instanced_with_push_constants_and_depth<T: bytemuck::Pod>(
        &self,
        mesh: &Mesh,
        program: &MeshProgram,
        instances: &crate::Buffer,
        instance_count: u32,
        constants: &T,
        depth: Option<&GraphImage>,
    ) -> Result<()> {
        let stage = reflected_push_constant_stages(
            program.reflection(),
            StageMask::VERTEX | StageMask::FRAGMENT,
        );
        self.draw_mesh_inner(
            mesh,
            program,
            Some(PushConstants {
                offset: 0,
                stages: stage,
                bytes: bytemuck::bytes_of(constants).to_vec(),
            }),
            Some((instances, instance_count)),
            instance_count,
            depth,
        )
    }

    /// Like `draw_mesh_instanced_with_push_constants_and_depth` but driven by a
    /// GPU indirect command buffer.
    ///
    /// `indirect_commands` must contain `draw_count` tightly-packed
    /// `DrawIndexedIndirectCommand` structs written by a prior CPU or compute
    /// pass. Each command encodes `(index_count, 1, 0, 0, first_instance)` where
    /// `first_instance` is the index of the surviving instance in the
    /// `instances` storage buffer — the vertex shader reads
    /// `instances[SV_InstanceID]` which equals `first_instance` when
    /// `instance_count == 1`.
    pub fn draw_mesh_indirect_with_push_constants_and_depth<T: bytemuck::Pod>(
        &self,
        mesh: &Mesh,
        program: &MeshProgram,
        instances: &crate::Buffer,
        indirect_commands: &crate::Buffer,
        draw_count: u32,
        constants: &T,
        depth: Option<&GraphImage>,
    ) -> Result<()> {
        if draw_count == 0 {
            return Ok(());
        }
        let stage = reflected_push_constant_stages(
            program.reflection(),
            StageMask::VERTEX | StageMask::FRAGMENT,
        );
        self.draw_mesh_indirect_inner(
            mesh,
            program,
            Some(PushConstants {
                offset: 0,
                stages: stage,
                bytes: bytemuck::bytes_of(constants).to_vec(),
            }),
            instances,
            indirect_commands,
            draw_count,
            depth,
        )
    }

    fn draw_mesh_indirect_inner(
        &self,
        mesh: &Mesh,
        program: &MeshProgram,
        push_constants: Option<PushConstants>,
        instance_buf: &crate::Buffer,
        indirect_buf: &crate::Buffer,
        draw_count: u32,
        depth: Option<&GraphImage>,
    ) -> Result<()> {
        let mut inner = self.frame.borrow_mut();
        let declaration_index = inner.declaration_index;
        inner.declaration_index = inner.declaration_index.saturating_add(1);

        inner.frame.inner.graph_mut(|g| g.import_image(self.handle, self.desc))?;
        inner.frame.inner.graph_mut(|g| {
            g.import_buffer(mesh.vertex_buffer.handle(), mesh.vertex_buffer.desc())
        })?;
        if let Some(ib) = &mesh.index_buffer {
            inner.frame.inner.graph_mut(|g| g.import_buffer(ib.handle(), ib.desc()))?;
        }
        inner.frame.inner.graph_mut(|g| g.import_buffer(instance_buf.handle(), instance_buf.desc()))?;
        inner.frame.inner.graph_mut(|g| g.import_buffer(indirect_buf.handle(), indirect_buf.desc()))?;

        let pipeline = program.pipeline_handle(self.desc.format, self.desc.samples)?;

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
        buffer_reads.push(crate::BufferUse {
            buffer: instance_buf.handle(),
            access: Access::Read,
            state: RgState::ShaderRead,
            offset: 0,
            size: instance_buf.desc().size,
        });
        buffer_reads.push(crate::BufferUse {
            buffer: indirect_buf.handle(),
            access: Access::Read,
            state: RgState::IndirectRead,
            offset: 0,
            size: indirect_buf.desc().size,
        });

        inner.buffers_by_name
            .insert("instances".to_owned(), (instance_buf.handle(), instance_buf.desc()));

        if let Some(d) = depth {
            inner.frame.inner.graph_mut(|g| g.import_image(d.handle, d.desc))?;
        }

        let pass_name = format!("{declaration_index:04}-draw-indirect-{}", self.name);
        let mesh_read_names = reflected_image_reads(program.reflection());
        let (eager_bindings, unresolved_read_names, eager_uses) =
            split_read_names(&mesh_read_names, &self.name, &inner.images_by_name);

        let mut writes = vec![crate::ImageUse {
            image: self.handle,
            access: Access::Write,
            state: RgState::RenderTarget,
            subresource: single_subresource(),
        }];
        let mut clear_depth = None;
        if let Some(d) = depth {
            writes.push(crate::ImageUse {
                image: d.handle,
                access: Access::Write,
                state: RgState::DepthWrite,
                subresource: single_subresource(),
            });
            clear_depth = Some((d.handle, f32::to_bits(1.0), 0u8));
        }

        // DrawIndexedIndirectCommand: 5 × u32 = 20 bytes.
        const INDIRECT_STRIDE: u32 = 20;

        inner.pending_passes.push(PendingPass {
            desc: PassDesc {
                name: pass_name,
                queue: crate::QueueType::Graphics,
                shader: Some(program.fragment.handle()),
                pipeline: Some(pipeline),
                bind_groups: Vec::new(),
                push_constants,
                work: PassWork::DrawIndirect(DrawIndirectDesc {
                    indirect_buffer: indirect_buf.handle(),
                    offset: 0,
                    draw_count,
                    stride: INDIRECT_STRIDE,
                    indexed: true,
                    vertex_buffer,
                    index_buffer,
                }),
                reads: eager_uses,
                writes,
                buffer_reads,
                buffer_writes: Vec::new(),
                clear_colors: Vec::new(),
                clear_depth,
            },
            deferred: Some(DeferredPassResolve {
                layout_handle: program.pipeline_layout.handle(),
                reflection: program.reflection().clone(),
                eager_bindings,
                eager_samplers: HashMap::new(),
                eager_buffers: HashMap::new(),
                unresolved_read_names,
                skip_name: self.name.clone(),
                storage_output: None,
            }),
        });

        Ok(())
    }

    fn draw_mesh_inner(
        &self,
        mesh: &Mesh,
        program: &MeshProgram,
        push_constants: Option<PushConstants>,
        instance_buf: Option<(&crate::Buffer, u32)>,
        instance_count: u32,
        depth: Option<&GraphImage>,
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

        let pipeline = program.pipeline_handle(self.desc.format, self.desc.samples)?;
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

        // Register the instance storage buffer under "instances" so the
        // reflected bind group builder can resolve it by name.
        if let Some((ibuf, _)) = instance_buf {
            inner
                .frame
                .inner
                .graph_mut(|g| g.import_buffer(ibuf.handle(), ibuf.desc()))?;
            inner
                .buffers_by_name
                .insert("instances".to_owned(), (ibuf.handle(), ibuf.desc()));
            buffer_reads.push(crate::BufferUse {
                buffer: ibuf.handle(),
                access: Access::Read,
                state: RgState::ShaderRead,
                offset: 0,
                size: ibuf.desc().size,
            });
        }

        // Import and register depth image if present.
        if let Some(d) = depth {
            inner
                .frame
                .inner
                .graph_mut(|g| g.import_image(d.handle, d.desc))?;
        }

        let pass_name = format!("{declaration_index:04}-draw-mesh-{}", self.name);
        let mesh_read_names = reflected_image_reads(program.reflection());
        let (eager_bindings, unresolved_read_names, mut eager_uses) =
            split_read_names(&mesh_read_names, &self.name, &inner.images_by_name);

        if program.alpha_blend {
            eager_uses.push(crate::ImageUse {
                image: self.handle,
                access: Access::Read,
                state: RgState::RenderTarget,
                subresource: single_subresource(),
            });
        }
        let target_use = crate::ImageUse {
            image: self.handle,
            access: Access::Write,
            state: RgState::RenderTarget,
            subresource: single_subresource(),
        };
        let depth_write = depth.map(|d| crate::ImageUse {
            image: d.handle,
            access: Access::Write,
            state: RgState::DepthWrite,
            subresource: single_subresource(),
        });
        let clear_depth = depth.map(|d| (d.handle, 1.0f32.to_bits(), 0u8));

        let mut writes = vec![target_use];
        if let Some(dw) = depth_write {
            writes.push(dw);
        }

        inner.pass_records.push(PassRecord {
            name: pass_name.clone(),
            kind: PassKind::Mesh,
            queue: core::QueueType::Graphics,
            reads: eager_uses.clone(),
            writes: writes.clone(),
            buffer_read_names: Vec::new(),
            buffer_write_names: Vec::new(),
            deferred_read_names: unresolved_read_names.clone(),
            skip_read_name: self.name.clone(),
        });

        inner.pending_passes.push(PendingPass {
            desc: PassDesc {
                name: pass_name,
                queue: crate::QueueType::Graphics,
                shader: Some(program.fragment.handle()),
                pipeline: Some(pipeline),
                bind_groups: Vec::new(), // filled at flush time
                push_constants,
                work: PassWork::Draw(DrawDesc {
                    vertex_count: draw_count,
                    instance_count,
                    first_vertex: 0,
                    first_instance: 0,
                    vertex_buffer,
                    index_buffer,
                }),
                reads: eager_uses,
                writes,
                buffer_reads,
                buffer_writes: Vec::new(),
                clear_colors: Vec::new(),
                clear_depth,
            },
            deferred: Some(DeferredPassResolve {
                layout_handle: program.pipeline_layout.handle(),
                reflection: program.reflection().clone(),
                eager_bindings,
                eager_samplers: HashMap::new(),
                eager_buffers: HashMap::new(),
                unresolved_read_names,
                skip_name: self.name.clone(),
                storage_output: None,
            }),
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
                subresource: single_subresource(),
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

    /// Resolve a multisampled image into this single-sample image.
    pub fn resolve_from(&self, src: &GraphImage) -> Result<()> {
        let src_desc = src.desc();
        let dst_desc = self.desc();
        if src_desc.samples <= 1 {
            return Err(Error::InvalidInput(
                "resolve source image must have more than one sample".into(),
            ));
        }
        if dst_desc.samples != 1 {
            return Err(Error::InvalidInput(
                "resolve destination image must have exactly one sample".into(),
            ));
        }
        if src_desc.format != dst_desc.format {
            return Err(Error::InvalidInput(
                "resolve source and destination formats must match".into(),
            ));
        }
        let width = src_desc.extent.width.min(dst_desc.extent.width).max(1);
        let height = src_desc.extent.height.min(dst_desc.extent.height).max(1);
        let layer_count = u32::from(src_desc.layers.min(dst_desc.layers)).max(1);

        let mut inner = self.frame.borrow_mut();
        let declaration_index = inner.declaration_index;
        inner.declaration_index = inner.declaration_index.saturating_add(1);
        inner
            .frame
            .inner
            .graph_mut(|graph| graph.import_image(src.handle(), src.desc()))?;
        inner
            .frame
            .inner
            .graph_mut(|graph| graph.import_image(self.handle, self.desc))?;

        let src_use = crate::ImageUse {
            image: src.handle(),
            access: Access::Read,
            state: RgState::CopySrc,
            subresource: single_subresource(),
        };
        let dst_use = crate::ImageUse {
            image: self.handle,
            access: Access::Write,
            state: RgState::CopyDst,
            subresource: single_subresource(),
        };
        let pass_name = format!("{declaration_index:04}-resolve-{}", self.name);
        inner.pass_records.push(PassRecord {
            name: pass_name.clone(),
            kind: PassKind::Resolve,
            queue: core::QueueType::Graphics,
            reads: vec![src_use],
            writes: vec![dst_use],
            buffer_read_names: Vec::new(),
            buffer_write_names: Vec::new(),
            deferred_read_names: Vec::new(),
            skip_read_name: self.name.clone(),
        });
        inner.pending_passes.push(PendingPass {
            desc: PassDesc {
                name: pass_name,
                queue: QueueType::Graphics,
                shader: None,
                pipeline: None,
                bind_groups: Vec::new(),
                push_constants: None,
                work: PassWork::ResolveImage(ResolveImageDesc {
                    src: src.handle(),
                    dst: self.handle,
                    src_mip_level: 0,
                    dst_mip_level: 0,
                    src_base_layer: 0,
                    dst_base_layer: 0,
                    layer_count,
                    width,
                    height,
                }),
                reads: vec![src_use],
                writes: vec![dst_use],
                buffer_reads: Vec::new(),
                buffer_writes: Vec::new(),
                clear_colors: Vec::new(),
                clear_depth: None,
            },
            deferred: None,
        });
        Ok(())
    }

    /// Return a single-sample image: resolves `self` when needed, otherwise clones it.
    pub fn resolve_msaa(&self, frame: &RenderFrame, name: impl Into<String>) -> Result<GraphImage> {
        if self.desc.samples <= 1 {
            return Ok(self.clone());
        }
        let resolved = frame.resolve_target_sized_to(name, self)?;
        resolved.resolve_from(self)?;
        Ok(resolved)
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

    pub fn execute_compute_with_push_constants_auto(
        &self,
        program: &ComputeProgram,
        bytes: &[u8],
        groups: [u32; 3],
    ) -> Result<()> {
        self.execute_compute_with_push_constants(
            program,
            reflected_push_constant_stages(program.reflection(), StageMask::COMPUTE),
            bytes,
            groups,
        )
    }

    pub fn execute_compute_with_constants<T: bytemuck::Pod>(
        &self,
        program: &ComputeProgram,
        stages: StageMask,
        constants: &T,
        groups: [u32; 3],
    ) -> Result<()> {
        self.execute_compute_with_push_constants(
            program,
            stages,
            bytemuck::bytes_of(constants),
            groups,
        )
    }

    pub fn execute_compute_with_constants_auto<T: bytemuck::Pod>(
        &self,
        program: &ComputeProgram,
        constants: &T,
        groups: [u32; 3],
    ) -> Result<()> {
        self.execute_compute_with_push_constants_auto(
            program,
            bytemuck::bytes_of(constants),
            groups,
        )
    }

    fn execute_compute_inner(
        &self,
        program: &ComputeProgram,
        push_constants: Option<PushConstants>,
        groups: [u32; 3],
    ) -> Result<()> {
        record_compute_shader_pass(
            &self.frame,
            None,
            self,
            program,
            push_constants,
            groups,
            ExplicitPassResources::default(),
        )
    }
}

fn record_fullscreen_shader_pass(
    frame: &Rc<RefCell<RenderFrameInner>>,
    pass_name_override: Option<String>,
    target: &GraphImage,
    shader: &ShaderProgram,
    push_constants: Option<PushConstants>,
    target_subresource: SubresourceRange,
    explicit: ExplicitPassResources,
) -> Result<()> {
    validate_pass_target_usage(&target.name, target.desc, crate::ImageUsage::RENDER_TARGET)?;
    let mut inner = frame.borrow_mut();
    let declaration_index = inner.declaration_index;
    inner.declaration_index = inner.declaration_index.saturating_add(1);

    inner
        .frame
        .inner
        .graph_mut(|graph| graph.import_image(target.handle, target.desc))?;
    inner.frame.inner.graph_mut(|graph| {
        graph.import_buffer(
            shader.fullscreen_triangle.handle(),
            shader.fullscreen_triangle.desc(),
        )
    })?;
    import_explicit_resources(&mut inner, &explicit)?;

    let pipeline = shader.pipeline_handle(target.desc.format, target.desc.samples)?;
    let read_names = reflected_image_reads(shader.reflection());
    let (eager_bindings, unresolved_read_names, eager_uses) = split_read_names_with_explicit(
        &read_names,
        &target.name,
        &inner.images_by_name,
        &explicit.images,
    );

    let pass_name = pass_name_override
        .unwrap_or_else(|| format!("{declaration_index:04}-execute-{}", target.name));
    let target_use = crate::ImageUse {
        image: target.handle,
        access: Access::Write,
        state: RgState::RenderTarget,
        subresource: target_subresource,
    };
    inner.pass_records.push(PassRecord {
        name: pass_name.clone(),
        kind: PassKind::Fullscreen,
        queue: core::QueueType::Graphics,
        reads: eager_uses.clone(),
        writes: vec![target_use],
        buffer_read_names: reflected_buffer_read_names(shader.reflection()),
        buffer_write_names: Vec::new(),
        deferred_read_names: unresolved_read_names.clone(),
        skip_read_name: target.name.clone(),
    });

    inner.pending_passes.push(PendingPass {
        desc: PassDesc {
            name: pass_name,
            queue: QueueType::Graphics,
            shader: Some(shader.fragment.handle()),
            pipeline: Some(pipeline),
            bind_groups: Vec::new(),
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
            reads: eager_uses,
            writes: vec![target_use],
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
        },
        deferred: Some(DeferredPassResolve {
            layout_handle: shader.pipeline_layout.handle(),
            reflection: shader.reflection().clone(),
            eager_bindings,
            eager_samplers: explicit.samplers,
            eager_buffers: explicit.buffers,
            unresolved_read_names,
            skip_name: target.name.clone(),
            storage_output: None,
        }),
    });
    Ok(())
}

fn record_compute_shader_pass(
    frame: &Rc<RefCell<RenderFrameInner>>,
    pass_name_override: Option<String>,
    target: &GraphImage,
    program: &ComputeProgram,
    push_constants: Option<PushConstants>,
    groups: [u32; 3],
    explicit: ExplicitPassResources,
) -> Result<()> {
    validate_pass_target_usage(&target.name, target.desc, crate::ImageUsage::STORAGE)?;
    let mut inner = frame.borrow_mut();
    let declaration_index = inner.declaration_index;
    inner.declaration_index = inner.declaration_index.saturating_add(1);

    inner
        .frame
        .inner
        .graph_mut(|graph| graph.import_image(target.handle, target.desc))?;
    import_explicit_resources(&mut inner, &explicit)?;

    let read_names = reflected_storage_image_reads(program.reflection());
    let (eager_bindings, unresolved_read_names, eager_uses) = split_read_names_with_explicit(
        &read_names,
        &target.name,
        &inner.images_by_name,
        &explicit.images,
    );

    let pass_name = pass_name_override
        .unwrap_or_else(|| format!("{declaration_index:04}-compute-{}", target.name));
    let target_use = crate::ImageUse {
        image: target.handle,
        access: Access::Write,
        state: RgState::ShaderWrite,
        subresource: single_subresource(),
    };
    inner.pass_records.push(PassRecord {
        name: pass_name.clone(),
        kind: PassKind::Compute,
        queue: core::QueueType::Compute,
        reads: eager_uses.clone(),
        writes: vec![target_use],
        buffer_read_names: reflected_buffer_read_names(program.reflection()),
        buffer_write_names: reflected_buffer_write_names(program.reflection()),
        deferred_read_names: unresolved_read_names.clone(),
        skip_read_name: target.name.clone(),
    });

    inner.pending_passes.push(PendingPass {
        desc: PassDesc {
            name: pass_name,
            queue: QueueType::Compute,
            shader: Some(program.shader.handle()),
            pipeline: Some(program.pipeline.handle()),
            bind_groups: Vec::new(),
            push_constants,
            work: PassWork::Dispatch(DispatchDesc {
                x: groups[0],
                y: groups[1],
                z: groups[2],
            }),
            reads: eager_uses,
            writes: vec![target_use],
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
        },
        deferred: Some(DeferredPassResolve {
            layout_handle: program.pipeline_layout.handle(),
            reflection: program.reflection().clone(),
            eager_bindings,
            eager_samplers: explicit.samplers,
            eager_buffers: explicit.buffers,
            unresolved_read_names,
            skip_name: target.name.clone(),
            storage_output: Some((target.name.clone(), target.handle)),
        }),
    });
    Ok(())
}

fn import_explicit_resources(
    inner: &mut RenderFrameInner,
    explicit: &ExplicitPassResources,
) -> Result<()> {
    for image in explicit.images.values() {
        let Some(record) = inner
            .images_by_name
            .values()
            .find(|record| record.handle == image.handle)
            .copied()
        else {
            continue;
        };
        inner
            .frame
            .inner
            .graph_mut(|g| g.import_image(record.handle, record.desc))?;
    }
    for (buffer, desc) in explicit.buffers.values() {
        inner
            .frame
            .inner
            .graph_mut(|g| g.import_buffer(*buffer, *desc))?;
    }
    Ok(())
}

impl GraphImageView {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn handle(&self) -> ImageHandle {
        self.handle
    }

    pub fn desc(&self) -> ImageDesc {
        self.desc
    }

    pub fn subresource(&self) -> SubresourceRange {
        self.subresource
    }

    pub fn mip_extent(&self) -> core::Extent3d {
        let mip = u32::from(self.subresource.base_mip);
        core::Extent3d {
            width: (self.desc.extent.width >> mip).max(1),
            height: (self.desc.extent.height >> mip).max(1),
            depth: (self.desc.extent.depth >> mip).max(1),
        }
    }

    pub fn execute_shader(&self, shader: &ShaderProgram) -> Result<()> {
        self.as_image()
            .execute_shader_inner(shader, None, self.subresource)
    }

    pub fn execute_shader_auto(&self, shader: &ShaderProgram) -> Result<()> {
        self.execute_shader(shader)
    }

    pub fn execute_shader_with_push_constants(
        &self,
        shader: &ShaderProgram,
        stages: StageMask,
        bytes: &[u8],
    ) -> Result<()> {
        self.as_image().execute_shader_inner(
            shader,
            Some(PushConstants {
                offset: 0,
                stages,
                bytes: bytes.to_vec(),
            }),
            self.subresource,
        )
    }

    pub fn execute_shader_with_constants<T: bytemuck::Pod>(
        &self,
        shader: &ShaderProgram,
        stages: StageMask,
        constants: &T,
    ) -> Result<()> {
        self.execute_shader_with_push_constants(shader, stages, bytemuck::bytes_of(constants))
    }

    pub fn execute_shader_with_constants_auto<T: bytemuck::Pod>(
        &self,
        shader: &ShaderProgram,
        constants: &T,
    ) -> Result<()> {
        let stages = reflected_push_constant_stages(shader.reflection(), shader.stage_mask());
        self.execute_shader_with_push_constants(shader, stages, bytemuck::bytes_of(constants))
    }

    pub fn register_as(&self, name: impl Into<String>) {
        let mut inner = self.frame.borrow_mut();
        inner.images_by_name.insert(
            name.into(),
            GraphImageRecord {
                handle: self.handle,
                desc: self.desc,
                subresource: self.subresource,
            },
        );
    }

    fn as_image(&self) -> GraphImage {
        GraphImage {
            frame: self.frame.clone(),
            name: self.name.clone(),
            handle: self.handle,
            desc: self.desc,
        }
    }
}

fn validate_subresource(desc: ImageDesc, subresource: SubresourceRange) -> Result<()> {
    validate_subresource_axis(
        "mip",
        subresource.base_mip,
        subresource.mip_count,
        desc.mip_levels,
    )?;
    validate_subresource_axis(
        "layer",
        subresource.base_layer,
        subresource.layer_count,
        desc.layers,
    )
}

fn validate_subresource_axis(name: &str, base: u16, count: u16, limit: u16) -> Result<()> {
    if count == 0 {
        return Err(Error::InvalidInput(format!(
            "{name} subresource count must be at least 1"
        )));
    }
    if base >= limit {
        return Err(Error::InvalidInput(format!(
            "{name} subresource base {base} is outside image limit {limit}"
        )));
    }
    let end = u32::from(base).saturating_add(u32::from(count));
    if end > u32::from(limit) {
        return Err(Error::InvalidInput(format!(
            "{name} subresource range [{base}, {end}) exceeds image limit {limit}"
        )));
    }
    Ok(())
}

impl ImageRef for GraphImage {
    fn image_handle(&self) -> ImageHandle {
        self.handle
    }

    fn image_desc(&self) -> ImageDesc {
        self.desc
    }
}

/// Split `read_names` against the current `images_by_name` snapshot.
///
/// Returns `(eager, unresolved, eager_uses)`:
/// - `eager`: name → handle for names already registered — captured now so alias
///   rewrites from later `register_as` calls don't corrupt per-pass bindings.
/// - `unresolved`: names not yet in the registry — resolved at flush time.
/// - `eager_uses`: `ImageUse` entries ready to append to `PassDesc.reads`.
fn split_read_names(
    read_names: &[String],
    skip_name: &str,
    images_by_name: &HashMap<String, GraphImageRecord>,
) -> (
    HashMap<String, ImageBinding>,
    Vec<String>,
    Vec<crate::ImageUse>,
) {
    split_read_names_with_explicit(read_names, skip_name, images_by_name, &HashMap::new())
}

fn split_read_names_with_explicit(
    read_names: &[String],
    skip_name: &str,
    images_by_name: &HashMap<String, GraphImageRecord>,
    explicit_images: &HashMap<String, ImageBinding>,
) -> (
    HashMap<String, ImageBinding>,
    Vec<String>,
    Vec<crate::ImageUse>,
) {
    let mut eager: HashMap<String, ImageBinding> = HashMap::new();
    let mut unresolved: Vec<String> = Vec::new();
    let mut uses: Vec<crate::ImageUse> = Vec::new();

    for name in read_names {
        if name == skip_name {
            continue;
        }
        if let Some(binding) = explicit_images.get(name.as_str()).copied() {
            eager.insert(name.clone(), binding);
            uses.push(crate::ImageUse {
                image: binding.handle,
                access: Access::Read,
                state: RgState::ShaderRead,
                subresource: binding.subresource,
            });
        } else if let Some(rec) = images_by_name.get(name.as_str()) {
            let binding = ImageBinding {
                handle: rec.handle,
                subresource: rec.subresource,
            };
            eager.insert(name.clone(), binding);
            uses.push(crate::ImageUse {
                image: binding.handle,
                access: Access::Read,
                state: RgState::ShaderRead,
                subresource: binding.subresource,
            });
        } else {
            unresolved.push(name.clone());
        }
    }

    (eager, unresolved, uses)
}

/// Drain `inner.pending_passes`, resolve deferred bindings, schedule, and submit.
///
/// Three phases:
/// 1. Resolve deferred reads and build bind groups against the fully-populated
///    `images_by_name` map (all images declared during the frame are registered).
/// 2. Run the scheduler on the resolved passes.
/// 3. Submit passes to the core frame in scheduled order.
fn submit_pending_passes(inner: &mut RenderFrameInner) -> Result<()> {
    if inner.pending_passes.is_empty() {
        return Ok(());
    }

    // Phase 1: resolve deferred data.
    let mut resolved: Vec<PassDesc> = Vec::with_capacity(inner.pending_passes.len());
    for pending in inner.pending_passes.drain(..) {
        let mut desc = pending.desc;
        if let Some(deferred) = pending.deferred {
            // Resolve image read names → ImageUse handles.
            for name in &deferred.unresolved_read_names {
                if *name == deferred.skip_name {
                    continue;
                }
                let record = inner.images_by_name.get(name).copied().ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "shader requires image '{name}', but no frame image with that name exists"
                    ))
                })?;
                inner
                    .frame
                    .inner
                    .graph_mut(|g| g.import_image(record.handle, record.desc))?;
                desc.reads.push(crate::ImageUse {
                    image: record.handle,
                    access: Access::Read,
                    state: RgState::ShaderRead,
                    subresource: record.subresource,
                });
            }
            let (buffer_reads, buffer_writes) = reflected_buffer_uses(
                &deferred.reflection,
                &inner.buffers_by_name,
                &deferred.eager_buffers,
            )?;
            append_unique_buffer_uses(&mut desc.buffer_reads, buffer_reads);
            append_unique_buffer_uses(&mut desc.buffer_writes, buffer_writes);

            // Build the bind group now that all images are known.
            let bind_groups = build_reflected_bind_group(
                &inner.engine,
                deferred.layout_handle,
                &deferred.reflection,
                &deferred.eager_bindings,
                &inner.images_by_name,
                &inner.samplers_by_name,
                &inner.buffers_by_name,
                &deferred.eager_samplers,
                &deferred.eager_buffers,
                deferred
                    .storage_output
                    .as_ref()
                    .map(|(s, h)| (s.as_str(), *h)),
            )?;
            desc.bind_groups = bind_groups.iter().map(|bg| bg.handle()).collect();
            inner.held_bind_groups.extend(bind_groups);
        }
        resolved.push(desc);
    }

    // Phase 2: schedule.
    let order = schedule_pass_order(&resolved, &inner.ordering_constraints);

    // Phase 3: submit in scheduled order (Option::take avoids needing Clone on PassDesc).
    let mut slots: Vec<Option<PassDesc>> = resolved.into_iter().map(Some).collect();
    for idx in order {
        //panic allowed, reason = "scheduler invariant: each slot index appears exactly once in the schedule"
        let pass = slots[idx]
            .take()
            .expect("scheduler produced duplicate index");
        inner.frame.add_pass(pass)?;
    }
    Ok(())
}

/// Returns true if `reader` must execute after `writer` due to data flow.
///
/// RAW dependencies are directional regardless of declaration order: a pass
/// that reads a resource must run after the pass that writes that resource.
/// Add a directed edge i→j to the scheduler's adjacency list.
/// Deduplicates edges so duplicate resource accesses don't inflate in-degree.
fn sched_add_edge(adj: &mut Vec<Vec<usize>>, in_degree: &mut Vec<usize>, i: usize, j: usize) {
    if !adj[i].contains(&j) {
        adj[i].push(j);
        in_degree[j] += 1;
    }
}

#[cfg(test)]
fn has_read_after_write_dependency(writer: &PassDesc, reader: &PassDesc) -> bool {
    for write in &writer.writes {
        if reader
            .reads
            .iter()
            .any(|read| image_uses_overlap(write, read))
        {
            return true;
        }
    }

    let writer_buf_writes: Vec<_> = writer.buffer_writes.iter().map(|u| u.buffer).collect();
    let reader_buf_reads: Vec<_> = reader.buffer_reads.iter().map(|u| u.buffer).collect();

    for h in &writer_buf_writes {
        if reader_buf_reads.contains(h) {
            return true;
        }
    }

    false
}

/// Returns true when `later` must preserve declaration order after `earlier`.
///
/// WAW and WAR hazards do not express data flow, but they still need a stable
/// ordering. Adding these edges in both directions creates cycles, so callers
/// only check this for declaration-ordered pass pairs.
#[cfg(test)]
fn has_declaration_order_hazard(earlier: &PassDesc, later: &PassDesc) -> bool {
    // WAW
    for earlier_write in &earlier.writes {
        if later
            .writes
            .iter()
            .any(|later_write| image_uses_overlap(earlier_write, later_write))
        {
            return true;
        }
    }
    // WAR
    for earlier_read in &earlier.reads {
        if later
            .writes
            .iter()
            .any(|later_write| image_uses_overlap(earlier_read, later_write))
        {
            return true;
        }
    }

    // Buffer hazards
    let e_buf_writes: Vec<_> = earlier.buffer_writes.iter().map(|u| u.buffer).collect();
    let e_buf_reads: Vec<_> = earlier.buffer_reads.iter().map(|u| u.buffer).collect();
    let l_buf_writes: Vec<_> = later.buffer_writes.iter().map(|u| u.buffer).collect();

    // WAW
    for h in &e_buf_writes {
        if l_buf_writes.contains(h) {
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

fn image_uses_overlap(a: &crate::ImageUse, b: &crate::ImageUse) -> bool {
    a.image == b.image && a.subresource.overlaps(b.subresource)
}

/// Returns the indices of `passes` in dependency-correct execution order.
///
/// Uses Kahn's topological sort.  Passes with no outstanding dependencies are
/// processed in declaration order (their original index) as a deterministic
/// tie-breaker, preserving the user's intent for truly independent passes.
///
/// Dependency edges are derived from an inverted resource index (image/buffer
/// handle → accessing passes) rather than the O(n²) all-pairs check, reducing
/// graph construction from O(n²·k) to O(n·k) where k is bindings per pass.
///
/// `ordering_constraints` is a list of `(before_image, after_image)` pairs declared
/// via [`RenderFrame::order_before`]: any pass writing `before_image` is forced to
/// precede any pass writing `after_image`, regardless of resource dependency inference.
///
/// If a cycle is detected (which should not occur in a valid render graph) the
/// remaining passes are appended in declaration order.
fn schedule_pass_order(
    passes: &[PassDesc],
    ordering_constraints: &[(ImageHandle, ImageHandle)],
) -> Vec<usize> {
    let n = passes.len();
    if n <= 1 {
        return (0..n).collect();
    }

    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut in_degree: Vec<usize> = vec![0; n];

    // --- Build inverted resource index ---
    // Each entry is (pass_index, subresource, is_write).
    let mut image_access: HashMap<ImageHandle, Vec<(usize, SubresourceRange, bool)>> =
        HashMap::new();
    // Buffer accesses: (pass_index, is_write).
    let mut buffer_access: HashMap<core::BufferHandle, Vec<(usize, bool)>> = HashMap::new();

    for (i, pass) in passes.iter().enumerate() {
        for u in &pass.writes {
            image_access
                .entry(u.image)
                .or_default()
                .push((i, u.subresource, true));
        }
        for u in &pass.reads {
            image_access
                .entry(u.image)
                .or_default()
                .push((i, u.subresource, false));
        }
        for u in &pass.buffer_writes {
            buffer_access.entry(u.buffer).or_default().push((i, true));
        }
        for u in &pass.buffer_reads {
            buffer_access.entry(u.buffer).or_default().push((i, false));
        }
    }

    // --- Emit edges from resource conflicts ---
    // For each resource, iterate its accesses once and add edges for RAW / WAW / WAR.
    // This is O(|accesses_per_resource|²) per resource, but each resource
    // typically has 1–3 accesses, making the total cost O(n) in practice.
    for accesses in image_access.values() {
        for a in 0..accesses.len() {
            for b in (a + 1)..accesses.len() {
                let (ai, a_sub, a_write) = accesses[a];
                let (bi, b_sub, b_write) = accesses[b];
                if !a_sub.overlaps(b_sub) {
                    continue;
                }
                match (a_write, b_write) {
                    // RAW: writer → reader
                    (true, false) => sched_add_edge(&mut adj, &mut in_degree, ai, bi),
                    (false, true) => sched_add_edge(&mut adj, &mut in_degree, bi, ai),
                    // WAW / WAR: preserve declaration order
                    _ => {
                        let (earlier, later) = if ai < bi { (ai, bi) } else { (bi, ai) };
                        sched_add_edge(&mut adj, &mut in_degree, earlier, later);
                    }
                }
            }
        }
    }

    for accesses in buffer_access.values() {
        for a in 0..accesses.len() {
            for b in (a + 1)..accesses.len() {
                let (ai, a_write) = accesses[a];
                let (bi, b_write) = accesses[b];
                match (a_write, b_write) {
                    (true, false) => sched_add_edge(&mut adj, &mut in_degree, ai, bi),
                    (false, true) => sched_add_edge(&mut adj, &mut in_degree, bi, ai),
                    _ => {
                        let (earlier, later) = if ai < bi { (ai, bi) } else { (bi, ai) };
                        sched_add_edge(&mut adj, &mut in_degree, earlier, later);
                    }
                }
            }
        }
    }

    // User-declared ordering constraints.
    for (before_img, after_img) in ordering_constraints {
        let before_pass = passes
            .iter()
            .position(|p| p.writes.iter().any(|u| u.image == *before_img));
        let after_pass = passes
            .iter()
            .position(|p| p.writes.iter().any(|u| u.image == *after_img));
        if let (Some(i), Some(j)) = (before_pass, after_pass) {
            if i != j {
                sched_add_edge(&mut adj, &mut in_degree, i, j);
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

fn reflected_buffer_read_names(reflection: &ShaderReflection) -> Vec<String> {
    reflected_bindings_of_kind(reflection, core::BindingKind::UniformBuffer)
}

fn reflected_buffer_write_names(reflection: &ShaderReflection) -> Vec<String> {
    reflected_bindings_of_kind(reflection, core::BindingKind::StorageBuffer)
}

fn reflected_push_constant_stages(reflection: &ShaderReflection, fallback: StageMask) -> StageMask {
    let mask = reflection.layout.push_constants_stage_mask;
    if mask.0 == 0 { fallback } else { mask }
}

fn reflected_bindings_of_kind(
    reflection: &ShaderReflection,
    kind: core::BindingKind,
) -> Vec<String> {
    if !reflection.parameters.is_empty() {
        return reflection
            .parameters
            .iter()
            .filter(|parameter| parameter.kind == crate::ShaderParameterKind::Resource(kind))
            .map(|parameter| parameter.name.clone())
            .collect();
    }

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
/// `eager_bindings`: name→image binding captured at pass-declaration time; takes priority over
/// `images_by_name` so that alias rewrites from later `register_as` calls don't corrupt
/// per-pass bindings (e.g. the bloom downsample chain reusing `"source_tex"`).
///
/// `output_image`: for compute passes, the image this pass writes to so it can
/// be bound as a StorageImage under its frame name.
fn build_reflected_bind_group(
    engine: &Engine,
    layout_handle: core::PipelineLayoutHandle,
    reflection: &ShaderReflection,
    eager_bindings: &HashMap<String, ImageBinding>,
    images_by_name: &HashMap<String, GraphImageRecord>,
    samplers_by_name: &HashMap<String, core::SamplerHandle>,
    buffers_by_name: &HashMap<String, (core::BufferHandle, crate::BufferDesc)>,
    eager_samplers: &HashMap<String, core::SamplerHandle>,
    eager_buffers: &HashMap<String, (core::BufferHandle, crate::BufferDesc)>,
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

    // Resolve a binding name to an image handle: prefer eager_bindings (declaration-time
    // snapshot) over images_by_name (which may have been overwritten by later register_as).
    let resolve_image = |path: &str| -> Option<ImageBinding> {
        eager_bindings.get(path).copied().or_else(|| {
            images_by_name.get(path).map(|r| ImageBinding {
                handle: r.handle,
                subresource: r.subresource,
            })
        })
    };
    let image_desc = |handle: ImageHandle| -> Option<ImageDesc> {
        images_by_name
            .values()
            .find(|record| record.handle == handle)
            .map(|record| record.desc)
    };

    let mut missing = Vec::new();

    let mut entries = Vec::new();
    for group in &reflection.layout.groups {
        for binding in &group.bindings {
            match binding.kind {
                BindingKind::SampledImage => {
                    if let Some(image) = resolve_image(&binding.path) {
                        if let Some(desc) = image_desc(image.handle) {
                            validate_reflected_image_usage(&binding.path, binding.kind, desc)?;
                        }
                        entries.push(BindGroupEntry {
                            path: binding.path.clone(),
                            resource: ResourceBinding::ImageView {
                                image: image.handle,
                                subresource: image.subresource,
                            },
                        });
                    } else {
                        missing.push(format!(
                            "{} ({:?} set binding {})",
                            binding.path, binding.kind, binding.binding
                        ));
                    }
                }
                BindingKind::StorageImage => {
                    let image = if let Some((name, h)) = output_image {
                        if binding.path == name {
                            Some(ImageBinding {
                                handle: h,
                                subresource: single_subresource(),
                            })
                        } else {
                            resolve_image(&binding.path)
                        }
                    } else {
                        resolve_image(&binding.path)
                    };
                    if let Some(image) = image {
                        if let Some(desc) = image_desc(image.handle) {
                            validate_reflected_image_usage(&binding.path, binding.kind, desc)?;
                        }
                        entries.push(BindGroupEntry {
                            path: binding.path.clone(),
                            resource: ResourceBinding::ImageView {
                                image: image.handle,
                                subresource: image.subresource,
                            },
                        });
                    } else {
                        missing.push(format!(
                            "{} ({:?} set binding {})",
                            binding.path, binding.kind, binding.binding
                        ));
                    }
                }
                BindingKind::Sampler => {
                    let handle = eager_samplers
                        .get(&binding.path)
                        .or_else(|| samplers_by_name.get(&binding.path))
                        .copied()
                        .unwrap_or_else(|| engine.default_sampler());
                    entries.push(BindGroupEntry {
                        path: binding.path.clone(),
                        resource: ResourceBinding::Sampler(handle),
                    });
                }
                BindingKind::StorageBuffer | BindingKind::UniformBuffer => {
                    if let Some((handle, _)) = eager_buffers
                        .get(&binding.path)
                        .or_else(|| buffers_by_name.get(&binding.path))
                    {
                        let (_, desc) = eager_buffers
                            .get(&binding.path)
                            .or_else(|| buffers_by_name.get(&binding.path))
                            .expect("buffer desc present with buffer handle");
                        validate_reflected_buffer_usage(&binding.path, binding.kind, *desc)?;
                        entries.push(BindGroupEntry {
                            path: binding.path.clone(),
                            resource: ResourceBinding::Buffer(*handle),
                        });
                    } else {
                        missing.push(format!(
                            "{} ({:?} set binding {})",
                            binding.path, binding.kind, binding.binding
                        ));
                    }
                }
                _ => {}
            }
        }
    }

    if !missing.is_empty() {
        return Err(Error::InvalidInput(format!(
            "shader reflection requires unbound resources: {}",
            missing.join(", ")
        )));
    }

    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let bind_group = engine.create_bind_group(BindGroupDesc {
        layout: layout_handle,
        entries,
    })?;
    Ok(vec![bind_group])
}

fn validate_pass_target_usage(name: &str, desc: ImageDesc, required: crate::ImageUsage) -> Result<()> {
    if desc.usage.contains(required) {
        return Ok(());
    }
    Err(Error::InvalidInput(format!(
        "pass target '{name}' requires {required:?} but image was created with {:?}",
        desc.usage
    )))
}

fn validate_reflected_image_usage(path: &str, kind: BindingKind, desc: ImageDesc) -> Result<()> {
    let required = match kind {
        BindingKind::SampledImage => crate::ImageUsage::SAMPLED,
        BindingKind::StorageImage => crate::ImageUsage::STORAGE,
        _ => return Ok(()),
    };
    if desc.usage.contains(required) {
        return Ok(());
    }
    Err(Error::InvalidInput(format!(
        "shader parameter '{path}' reflected as {kind:?}, but bound image usage {:?} does not include {:?}",
        desc.usage, required
    )))
}

fn validate_reflected_buffer_usage(path: &str, kind: BindingKind, desc: BufferDesc) -> Result<()> {
    let required = match kind {
        BindingKind::UniformBuffer => BufferUsage::UNIFORM,
        BindingKind::StorageBuffer => BufferUsage::STORAGE,
        _ => return Ok(()),
    };
    if desc.usage.contains(required) {
        return Ok(());
    }
    Err(Error::InvalidInput(format!(
        "shader parameter '{path}' reflected as {kind:?}, but bound buffer usage {:?} does not include {:?}",
        desc.usage, required
    )))
}

fn validate_deferred_reflected_resources(
    pass_name: &str,
    deferred: &DeferredPassResolve,
    images_by_name: &HashMap<String, GraphImageRecord>,
    buffers_by_name: &HashMap<String, (core::BufferHandle, BufferDesc)>,
) -> Vec<GraphDiagnostic> {
    let mut diagnostics = Vec::new();
    let image_desc_for = |binding: ImageBinding| -> Option<ImageDesc> {
        images_by_name
            .values()
            .find(|record| record.handle == binding.handle)
            .map(|record| record.desc)
    };
    let image_desc_for_output = |handle: ImageHandle| -> Option<ImageDesc> {
        images_by_name
            .values()
            .find(|record| record.handle == handle)
            .map(|record| record.desc)
    };

    for parameter in &deferred.reflection.parameters {
        let crate::ShaderParameterKind::Resource(kind) = parameter.kind else {
            continue;
        };
        match kind {
            BindingKind::SampledImage | BindingKind::StorageImage => {
                let binding = deferred
                    .eager_bindings
                    .get(&parameter.name)
                    .copied()
                    .or_else(|| {
                        images_by_name
                            .get(&parameter.name)
                            .map(|record| ImageBinding {
                                handle: record.handle,
                                subresource: record.subresource,
                            })
                    });
                let desc = binding.and_then(image_desc_for).or_else(|| {
                    deferred.storage_output.as_ref().and_then(|(name, handle)| {
                        (name == &parameter.name)
                            .then_some(*handle)
                            .and_then(image_desc_for_output)
                    })
                });
                let Some(desc) = desc else {
                    diagnostics.push(GraphDiagnostic {
                        level: DiagnosticLevel::Error,
                        message: format!(
                            "pass '{pass_name}' requires reflected image '{}' but no image with that name was bound",
                            parameter.name
                        ),
                    });
                    continue;
                };
                if let Err(error) = validate_reflected_image_usage(&parameter.name, kind, desc) {
                    diagnostics.push(GraphDiagnostic {
                        level: DiagnosticLevel::Error,
                        message: format!("pass '{pass_name}': {error}"),
                    });
                }
            }
            BindingKind::UniformBuffer | BindingKind::StorageBuffer => {
                let desc = deferred
                    .eager_buffers
                    .get(&parameter.name)
                    .or_else(|| buffers_by_name.get(&parameter.name))
                    .map(|(_, desc)| *desc);
                let Some(desc) = desc else {
                    diagnostics.push(GraphDiagnostic {
                        level: DiagnosticLevel::Error,
                        message: format!(
                            "pass '{pass_name}' requires reflected buffer '{}' but no buffer with that name was bound",
                            parameter.name
                        ),
                    });
                    continue;
                };
                if let Err(error) = validate_reflected_buffer_usage(&parameter.name, kind, desc) {
                    diagnostics.push(GraphDiagnostic {
                        level: DiagnosticLevel::Error,
                        message: format!("pass '{pass_name}': {error}"),
                    });
                }
            }
            BindingKind::Sampler | BindingKind::AccelerationStructure => {}
        }
    }
    diagnostics
}

fn reflected_buffer_uses(
    reflection: &ShaderReflection,
    buffers_by_name: &HashMap<String, (core::BufferHandle, BufferDesc)>,
    eager_buffers: &HashMap<String, (core::BufferHandle, BufferDesc)>,
) -> Result<(Vec<crate::BufferUse>, Vec<crate::BufferUse>)> {
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    for parameter in &reflection.parameters {
        let crate::ShaderParameterKind::Resource(kind) = parameter.kind else {
            continue;
        };
        if !matches!(
            kind,
            BindingKind::UniformBuffer | BindingKind::StorageBuffer
        ) {
            continue;
        }
        let Some((handle, desc)) = eager_buffers
            .get(&parameter.name)
            .or_else(|| buffers_by_name.get(&parameter.name))
            .copied()
        else {
            return Err(Error::InvalidInput(format!(
                "shader reflection requires buffer '{}', but no buffer with that name was bound",
                parameter.name
            )));
        };
        validate_reflected_buffer_usage(&parameter.name, kind, desc)?;
        let read_state = match kind {
            BindingKind::UniformBuffer => RgState::UniformRead,
            BindingKind::StorageBuffer => RgState::ShaderRead,
            _ => RgState::ShaderRead,
        };
        let use_ = crate::BufferUse {
            buffer: handle,
            access: Access::Read,
            state: read_state,
            offset: 0,
            size: desc.size,
        };
        match parameter.access {
            crate::ShaderResourceAccess::Read => reads.push(use_),
            crate::ShaderResourceAccess::Write => writes.push(crate::BufferUse {
                access: Access::Write,
                state: RgState::ShaderWrite,
                ..use_
            }),
            crate::ShaderResourceAccess::ReadWrite => {
                reads.push(use_);
                writes.push(crate::BufferUse {
                    access: Access::Write,
                    state: RgState::ShaderWrite,
                    ..use_
                });
            }
        }
    }
    Ok((reads, writes))
}

fn append_unique_buffer_uses(target: &mut Vec<crate::BufferUse>, uses: Vec<crate::BufferUse>) {
    for use_ in uses {
        if !target.iter().any(|existing| {
            existing.buffer == use_.buffer
                && existing.offset == use_.offset
                && existing.size == use_.size
                && existing.access == use_.access
        }) {
            target.push(use_);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_use(image: u64, access: Access, state: RgState) -> crate::ImageUse {
        crate::ImageUse {
            image: core::ImageHandle(image),
            access,
            state,
            subresource: single_subresource(),
        }
    }

    fn image_use_mip(image: u64, mip: u16, access: Access, state: RgState) -> crate::ImageUse {
        crate::ImageUse {
            image: core::ImageHandle(image),
            access,
            state,
            subresource: SubresourceRange::new(mip, 1, 0, 1),
        }
    }

    fn pass(name: &str, reads: &[u64], writes: &[u64]) -> PassDesc {
        PassDesc {
            name: name.to_owned(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            work: PassWork::None,
            reads: reads
                .iter()
                .copied()
                .map(|image| image_use(image, Access::Read, RgState::ShaderRead))
                .collect(),
            writes: writes
                .iter()
                .copied()
                .map(|image| image_use(image, Access::Write, RgState::RenderTarget))
                .collect(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
        }
    }

    fn pass_with_uses(
        name: &str,
        reads: Vec<crate::ImageUse>,
        writes: Vec<crate::ImageUse>,
    ) -> PassDesc {
        PassDesc {
            name: name.to_owned(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            work: PassWork::None,
            reads,
            writes,
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
        }
    }

    #[test]
    fn scheduler_keeps_raw_edges_through_declaration_order_waw() {
        let passes = vec![
            pass("tonemap", &[2], &[1]),
            pass("composite", &[], &[2]),
            pass("hud", &[], &[1]),
        ];

        assert!(has_read_after_write_dependency(&passes[1], &passes[0]));
        let order = schedule_pass_order(&passes, &[]);
        assert_eq!(order, vec![1, 0, 2]);
    }

    #[test]
    fn declaration_order_hazards_do_not_create_reverse_waw_edges() {
        let first = pass("first", &[], &[1]);
        let second = pass("second", &[], &[1]);

        assert!(has_declaration_order_hazard(&first, &second));
        assert!(!has_read_after_write_dependency(&first, &second));
        assert!(!has_read_after_write_dependency(&second, &first));
    }

    #[test]
    fn alpha_overlay_read_write_creates_dependency_on_previous_target_write() {
        let tonemap = pass("tonemap", &[2], &[1]);
        let overlay = pass("hud", &[1], &[1]);

        assert!(has_read_after_write_dependency(&tonemap, &overlay));
    }

    #[test]
    fn non_overlapping_mip_writes_do_not_create_declaration_hazard() {
        let mip0 = pass_with_uses(
            "mip0",
            Vec::new(),
            vec![image_use_mip(1, 0, Access::Write, RgState::RenderTarget)],
        );
        let mip1 = pass_with_uses(
            "mip1",
            Vec::new(),
            vec![image_use_mip(1, 1, Access::Write, RgState::RenderTarget)],
        );

        assert!(!has_declaration_order_hazard(&mip0, &mip1));
    }

    #[test]
    fn overlapping_mip_write_and_read_create_raw_dependency() {
        let writer = pass_with_uses(
            "writer",
            Vec::new(),
            vec![image_use_mip(1, 2, Access::Write, RgState::RenderTarget)],
        );
        let reader = pass_with_uses(
            "reader",
            vec![image_use_mip(1, 2, Access::Read, RgState::ShaderRead)],
            Vec::new(),
        );

        assert!(has_read_after_write_dependency(&writer, &reader));
    }

    #[test]
    fn full_resource_access_overlaps_selected_mip() {
        let full = crate::ImageUse {
            image: core::ImageHandle(1),
            access: Access::Write,
            state: RgState::RenderTarget,
            subresource: SubresourceRange::WHOLE,
        };
        let mip = image_use_mip(1, 3, Access::Read, RgState::ShaderRead);

        assert!(image_uses_overlap(&full, &mip));
    }

    #[test]
    fn scheduler_allows_independent_mip_writes_before_dependent_read() {
        let passes = vec![
            pass_with_uses(
                "read-mip1",
                vec![image_use_mip(1, 1, Access::Read, RgState::ShaderRead)],
                Vec::new(),
            ),
            pass_with_uses(
                "write-mip0",
                Vec::new(),
                vec![image_use_mip(1, 0, Access::Write, RgState::RenderTarget)],
            ),
            pass_with_uses(
                "write-mip1",
                Vec::new(),
                vec![image_use_mip(1, 1, Access::Write, RgState::RenderTarget)],
            ),
        ];

        let order = schedule_pass_order(&passes, &[]);
        assert_eq!(order, vec![1, 2, 0]);
    }

    #[test]
    fn subresource_validation_rejects_out_of_bounds_mips_and_layers() {
        let desc = ImageDesc {
            dimension: crate::ImageDimension::D2,
            extent: core::Extent3d {
                width: 64,
                height: 64,
                depth: 1,
            },
            mip_levels: 4,
            layers: 2,
            samples: 1,
            format: Format::Rgba8Unorm,
            usage: crate::ImageUsage::SAMPLED,
            transient: false,
            clear_value: None,
            debug_name: None,
        };

        assert!(validate_subresource(desc, SubresourceRange::new(3, 1, 1, 1)).is_ok());
        assert!(validate_subresource(desc, SubresourceRange::new(4, 1, 0, 1)).is_err());
        assert!(validate_subresource(desc, SubresourceRange::new(2, 3, 0, 1)).is_err());
        assert!(validate_subresource(desc, SubresourceRange::new(0, 1, 2, 1)).is_err());
        assert!(validate_subresource(desc, SubresourceRange::new(0, 1, 1, 2)).is_err());
    }
}
