use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use sturdy_engine_core as core;

use crate::{
    ColorTargetDesc, CullMode, Engine, Error, Format, FrontFace, GraphicsPipelineDesc, Pipeline,
    PipelineLayout, PrimitiveTopology, RasterState, Result, Shader, ShaderDesc, ShaderReflection,
    ShaderSource, ShaderStage, VertexBufferLayout, VertexInputRate,
    mesh::{Vertex2d, Vertex3d, vertex2d_attributes, vertex3d_attributes},
};

const DEFAULT_VERTEX_2D: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/mesh_vertex_2d.slang"
));

const DEFAULT_VERTEX_3D: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/mesh_vertex_3d.slang"
));

pub struct MeshProgramDesc {
    pub fragment: ShaderDesc,
    /// Custom vertex shader. None uses the built-in for the chosen kind.
    pub vertex: Option<ShaderDesc>,
    pub vertex_kind: MeshVertexKind,
    pub alpha_blend: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MeshVertexKind {
    V2d,
    V3d,
}

pub struct MeshProgram {
    pub(crate) engine: Engine,
    pub(crate) pipelines: Mutex<HashMap<(Format, u8), Pipeline>>,
    pub(crate) pipeline_layout: PipelineLayout,
    pub(crate) vertex: Shader,
    pub(crate) fragment: Shader,
    pub(crate) vertex_kind: MeshVertexKind,
    pub(crate) alpha_blend: bool,
    pub(crate) reflection: ShaderReflection,
    fragment_path: Option<PathBuf>,
    vertex_path: Option<PathBuf>,
}

impl MeshProgram {
    /// Load a 2D mesh program from a fragment shader file.
    ///
    /// The default vertex shader passes position, uv, and per-vertex color straight
    /// through to clip space with no transform applied. Positions are expected in NDC.
    pub fn load_2d(engine: &Engine, fragment_path: impl Into<PathBuf>) -> Result<Self> {
        Self::new(
            engine,
            MeshProgramDesc {
                fragment: ShaderDesc {
                    source: ShaderSource::File(fragment_path.into()),
                    entry_point: "main".to_owned(),
                    stage: ShaderStage::Fragment,
                },
                vertex: None,
                vertex_kind: MeshVertexKind::V2d,
                alpha_blend: false,
            },
        )
    }

    /// Load a 2D mesh program that alpha-blends over the render target.
    pub fn load_2d_alpha(engine: &Engine, fragment_path: impl Into<PathBuf>) -> Result<Self> {
        Self::new(
            engine,
            MeshProgramDesc {
                fragment: ShaderDesc {
                    source: ShaderSource::File(fragment_path.into()),
                    entry_point: "main".to_owned(),
                    stage: ShaderStage::Fragment,
                },
                vertex: None,
                vertex_kind: MeshVertexKind::V2d,
                alpha_blend: true,
            },
        )
    }

    /// Load a 3D mesh program from vertex and fragment shader files.
    ///
    /// The default vertex shader passes position, normal, and uv through with no transform.
    /// Provide `vertex_path` to use your own vertex shader (e.g. one that applies an MVP matrix).
    pub fn load_3d(
        engine: &Engine,
        fragment_path: impl Into<PathBuf>,
        vertex_path: Option<impl Into<PathBuf>>,
    ) -> Result<Self> {
        Self::new(
            engine,
            MeshProgramDesc {
                fragment: ShaderDesc {
                    source: ShaderSource::File(fragment_path.into()),
                    entry_point: "main".to_owned(),
                    stage: ShaderStage::Fragment,
                },
                vertex: vertex_path.map(|p| ShaderDesc {
                    source: ShaderSource::File(p.into()),
                    entry_point: "main".to_owned(),
                    stage: ShaderStage::Vertex,
                }),
                vertex_kind: MeshVertexKind::V3d,
                alpha_blend: false,
            },
        )
    }

    pub fn new(engine: &Engine, desc: MeshProgramDesc) -> Result<Self> {
        let default_vertex_src = match desc.vertex_kind {
            MeshVertexKind::V2d => DEFAULT_VERTEX_2D,
            MeshVertexKind::V3d => DEFAULT_VERTEX_3D,
        };
        let vertex_path = desc.vertex.as_ref().and_then(|v| match &v.source {
            ShaderSource::File(p) => Some(p.clone()),
            _ => None,
        });
        let fragment_path = match &desc.fragment.source {
            ShaderSource::File(p) => Some(p.clone()),
            _ => None,
        };
        let vertex_desc = desc.vertex.unwrap_or_else(|| ShaderDesc {
            source: ShaderSource::Inline(default_vertex_src.to_owned()),
            entry_point: "main".to_owned(),
            stage: ShaderStage::Vertex,
        });
        let vertex = engine.create_shader(vertex_desc)?;
        let fragment = engine.create_shader(desc.fragment)?;
        let reflection = engine.shader_reflection(&fragment)?;
        let pipeline_layout = engine.create_pipeline_layout(reflection.layout.clone())?;
        Ok(Self {
            engine: engine.clone(),
            pipelines: Mutex::new(HashMap::new()),
            pipeline_layout,
            vertex,
            fragment,
            vertex_kind: desc.vertex_kind,
            alpha_blend: desc.alpha_blend,
            reflection,
            fragment_path,
            vertex_path,
        })
    }

    pub fn reflection(&self) -> &ShaderReflection {
        &self.reflection
    }

    /// Return the fragment shader source path if loaded from a file.
    pub fn fragment_path(&self) -> Option<&Path> {
        self.fragment_path.as_deref()
    }

    /// Return the vertex shader source path if loaded from a file (None for the built-in default).
    pub fn vertex_path(&self) -> Option<&Path> {
        self.vertex_path.as_deref()
    }

    /// Recompile the fragment shader from its original source file and rebuild all cached pipelines.
    ///
    /// Returns `Ok(true)` on success, `Ok(false)` when there is no file path to reload from,
    /// and `Err` on compile failure. The previous pipeline remains active on failure.
    pub fn reload_fragment(&mut self) -> Result<bool> {
        let path = match &self.fragment_path {
            Some(p) => p.clone(),
            None => return Ok(false),
        };
        let fragment = self.engine.create_shader(ShaderDesc {
            source: ShaderSource::File(path),
            entry_point: "main".to_owned(),
            stage: ShaderStage::Fragment,
        })?;
        let reflection = self.engine.shader_reflection(&fragment)?;
        let pipeline_layout = self
            .engine
            .create_pipeline_layout(reflection.layout.clone())?;
        self.fragment = fragment;
        self.reflection = reflection;
        self.pipeline_layout = pipeline_layout;
        self.pipelines
            .lock()
            .expect("mesh program pipeline mutex poisoned")
            .clear();
        Ok(true)
    }

    /// Recompile the vertex shader from its original source file and rebuild all cached pipelines.
    ///
    /// Returns `Ok(true)` on success, `Ok(false)` when there is no file path to reload from,
    /// and `Err` on compile failure.
    pub fn reload_vertex(&mut self) -> Result<bool> {
        let path = match &self.vertex_path {
            Some(p) => p.clone(),
            None => return Ok(false),
        };
        let vertex = self.engine.create_shader(ShaderDesc {
            source: ShaderSource::File(path),
            entry_point: "main".to_owned(),
            stage: ShaderStage::Vertex,
        })?;
        self.vertex = vertex;
        self.pipelines
            .lock()
            .expect("mesh program pipeline mutex poisoned")
            .clear();
        Ok(true)
    }

    pub(crate) fn pipeline_handle(
        &self,
        format: Format,
        samples: u8,
    ) -> Result<core::PipelineHandle> {
        let mut pipelines = self
            .pipelines
            .lock()
            .expect("mesh program pipeline mutex poisoned");
        let key = (format, samples.max(1));
        if !pipelines.contains_key(&key) {
            let (vertex_stride, attributes) = match self.vertex_kind {
                MeshVertexKind::V2d => (
                    std::mem::size_of::<Vertex2d>() as u32,
                    vertex2d_attributes(),
                ),
                MeshVertexKind::V3d => (
                    std::mem::size_of::<Vertex3d>() as u32,
                    vertex3d_attributes(),
                ),
            };
            let pipeline = self.engine.create_graphics_pipeline(GraphicsPipelineDesc {
                vertex_shader: self.vertex.handle(),
                fragment_shader: Some(self.fragment.handle()),
                layout: Some(self.pipeline_layout.handle()),
                vertex_buffers: vec![VertexBufferLayout {
                    binding: 0,
                    stride: vertex_stride,
                    input_rate: VertexInputRate::Vertex,
                }],
                vertex_attributes: attributes,
                color_targets: vec![if self.alpha_blend {
                    ColorTargetDesc::alpha_blend(format)
                } else {
                    ColorTargetDesc::opaque(format)
                }],
                depth_format: None,
                samples: key.1,
                topology: PrimitiveTopology::TriangleList,
                raster: RasterState {
                    cull_mode: CullMode::None,
                    front_face: FrontFace::CounterClockwise,
                },
            })?;
            pipeline.set_debug_name("mesh-program")?;
            pipelines.insert(key, pipeline);
        }
        pipelines
            .get(&key)
            .map(Pipeline::handle)
            .ok_or_else(|| Error::Unknown("mesh program pipeline cache miss".into()))
    }
}
