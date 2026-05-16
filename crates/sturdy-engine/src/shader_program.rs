// ShaderProgram — fullscreen and compute shader program type.
//
// A `ShaderProgram` wraps a compiled Slang vertex+fragment or compute shader
// pair together with its reflected pipeline layout and a per-target-format
// pipeline cache.  It is the primary input to fullscreen and compute passes
// recorded through `RenderFrame` / `GraphImage`.
//
// ## Fullscreen shaders
//
// Fragment-stage programs run over a single fullscreen triangle. The built-in
// vertex shader (`fullscreen_vertex.slang`) provides `SV_POSITION` and UV
// coordinates to the fragment stage. Fragment shaders declare named texture
// and buffer bindings; the render graph resolves them by name at flush time.
//
// ## Compute shaders
//
// Compute-stage programs are dispatched via `frame.dispatch_compute_auto(…)`
// or `frame.dispatch_compute(…)`. They do not use the fullscreen triangle.
//
// ## Hot reload
//
// File-backed programs (loaded via `load_fragment` or `load_compute`) can be
// hot-reloaded by calling `program.reload()`. The vertex shader, reflection,
// and pipeline cache are rebuilt; all pipeline objects are re-created lazily
// on the next draw that uses the updated program.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use crate::{
    Buffer, BufferDesc, BufferUsage, ColorTargetDesc, CullMode, Engine, Error, Format, FrontFace,
    GraphicsPipelineDesc, Pipeline, PipelineLayout, PrimitiveTopology, RasterState, Result, Shader,
    ShaderDesc, ShaderReflection, ShaderSource, ShaderStage, StageMask, VertexAttributeDesc,
    VertexBufferLayout, VertexFormat, VertexInputRate,
};

// ── Embedded shader sources ───────────────────────────────────────────────────

pub(crate) const FULLSCREEN_VERTEX_SHADER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/fullscreen_vertex.slang"
));

pub(crate) const PASSTHROUGH_FRAGMENT_SHADER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/passthrough_fragment.slang"
));

// ── Internal vertex type ──────────────────────────────────────────────────────

/// Vertex layout for the fullscreen triangle buffer.
///
/// Three vertices at (-1,-3), (-1,1), (3,1) in clip space. The triangle covers
/// the whole screen with UV coordinates correctly mapped to [0,1]×[0,1].
#[repr(C)]
pub(crate) struct FullscreenVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
}

// ── Public descriptor types ───────────────────────────────────────────────────

/// Descriptor for creating a [`ShaderProgram`].
///
/// Passed to [`ShaderProgram::new`]. For convenience, prefer the typed
/// constructors (`from_inline_fragment`, `load_fragment`, `load_compute`, etc.)
/// over constructing this directly.
pub struct ShaderProgramDesc {
    pub fragment: ShaderDesc,
    pub vertex: Option<ShaderDesc>,
}

/// An opaque shader program name for diagnostics and hot-reload tracking.
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

/// Entry point specification for `Engine::load_slang_source`.
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

// ── ShaderProgram ─────────────────────────────────────────────────────────────

/// A compiled Slang shader program ready for use in fullscreen or compute passes.
///
/// Create via the typed constructors (`from_inline_fragment`, `load_fragment`,
/// `load_compute`, …) or via [`Engine::create_shader_program`].
pub struct ShaderProgram {
    pub(crate) engine: Engine,
    pub(crate) pipelines: Mutex<HashMap<(Format, u8), Pipeline>>,
    pub(crate) pipeline_layout: PipelineLayout,
    pub(crate) vertex: Shader,
    pub(crate) fragment: Shader,
    pub(crate) fullscreen_triangle: Buffer,
    pub(crate) reflection: ShaderReflection,
    pub(crate) stage: ShaderStage,
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
                    requires_ray_query: false,
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
                    requires_ray_query: false,
                },
            },
        )
    }

    /// Load a fragment shader from `path`.
    ///
    /// If the path has a `.spv` extension the file is read as pre-compiled
    /// SPIR-V (via [`ShaderSource::Spirv`]). Any other extension is compiled
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
                    requires_ray_query: false,
                },
            },
        )?;
        if !path.extension().map_or(false, |e| e == "spv") {
            program.source_path = Some(path);
        }
        Ok(program)
    }

    /// Create a passthrough shader that copies `source` to the render target.
    ///
    /// Use with [`GraphImage::blit_from`](crate::GraphImage::blit_from).
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
                    requires_ray_query: false,
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
            requires_ray_query: false,
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
    pub fn stage_mask(&self) -> StageMask {
        match self.stage {
            ShaderStage::Vertex => StageMask::VERTEX,
            ShaderStage::Fragment => StageMask::FRAGMENT,
            ShaderStage::Compute => StageMask::COMPUTE,
            ShaderStage::Mesh => StageMask::MESH,
            ShaderStage::Task => StageMask::TASK,
            ShaderStage::RayGeneration | ShaderStage::Miss | ShaderStage::ClosestHit => {
                StageMask::RAY_TRACING
            }
        }
    }

    pub(crate) fn pipeline_handle(
        &self,
        format: Format,
        samples: u8,
    ) -> Result<sturdy_engine_core::PipelineHandle> {
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

// ── Internal helpers ──────────────────────────────────────────────────────────

pub(crate) fn create_fullscreen_triangle(engine: &Engine) -> Result<Buffer> {
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

pub(crate) fn bytes_of_slice<T>(values: &[T]) -> &[u8] {
    //panic allowed, reason = "reinterpreting T as bytes is always safe for POD vertex data"
    unsafe {
        std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), std::mem::size_of_val(values))
    }
}

fn default_vertex_desc() -> ShaderDesc {
    ShaderDesc {
        source: ShaderSource::Inline(FULLSCREEN_VERTEX_SHADER.to_owned()),
        entry_point: "main".to_owned(),
        stage: ShaderStage::Vertex,
        requires_ray_query: false,
    }
}
