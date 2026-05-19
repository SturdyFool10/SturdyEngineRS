use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use crate::{
    Buffer, ColorTargetDesc, CullMode, Engine, Error, Format, FrontFace, GraphicsPipelineDesc,
    Pipeline, PipelineLayout, PrimitiveTopology, RasterState, Result, Shader, ShaderDesc,
    ShaderReflection, ShaderSource, ShaderStage, StageMask, VertexAttributeDesc,
    VertexBufferLayout, VertexFormat, VertexInputRate,
};

use super::{FullscreenVertex, ShaderProgramDesc, builtin_shader_path, create_fullscreen_triangle};

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
    /// This remains available for generated shaders. Built-in shaders should use
    /// file-backed loading so shader source stays out of Rust implementation files.
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
                    requires_cooperative_matrix: false,
                    uses_ser: false,
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
                    requires_cooperative_matrix: false,
                    uses_ser: false,
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
                    requires_cooperative_matrix: false,
                    uses_ser: false,
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
        Self::load_fragment(engine, builtin_shader_path("passthrough_fragment.slang"))
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
                    requires_cooperative_matrix: false,
                    uses_ser: false,
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
            requires_cooperative_matrix: false,
            uses_ser: false,
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
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
                    polygon_mode: Default::default(),
                    depth_clamp: false,
                    rasterizer_discard: false,
                    cull_mode: CullMode::None,
                    front_face: FrontFace::CounterClockwise,
                },
                conservative_raster: sturdy_engine_core::ConservativeRasterMode::Off,
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

fn default_vertex_desc() -> ShaderDesc {
    ShaderDesc {
        source: ShaderSource::File(builtin_shader_path("fullscreen_vertex.slang")),
        entry_point: "main".to_owned(),
        stage: ShaderStage::Vertex,
        requires_ray_query: false,
        requires_cooperative_matrix: false,
        uses_ser: false,
    }
}
