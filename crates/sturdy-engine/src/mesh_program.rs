use std::{collections::HashMap, path::PathBuf, sync::Mutex};

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
    pub(crate) pipelines: Mutex<HashMap<Format, Pipeline>>,
    pub(crate) pipeline_layout: PipelineLayout,
    pub(crate) vertex: Shader,
    pub(crate) fragment: Shader,
    pub(crate) vertex_kind: MeshVertexKind,
    pub(crate) alpha_blend: bool,
    pub(crate) reflection: ShaderReflection,
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
        })
    }

    pub fn reflection(&self) -> &ShaderReflection {
        &self.reflection
    }

    pub(crate) fn pipeline_handle(&self, format: Format) -> Result<core::PipelineHandle> {
        let mut pipelines = self
            .pipelines
            .lock()
            .expect("mesh program pipeline mutex poisoned");
        if !pipelines.contains_key(&format) {
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
                topology: PrimitiveTopology::TriangleList,
                raster: RasterState {
                    cull_mode: CullMode::None,
                    front_face: FrontFace::CounterClockwise,
                },
            })?;
            pipeline.set_debug_name("mesh-program")?;
            pipelines.insert(format, pipeline);
        }
        pipelines
            .get(&format)
            .map(Pipeline::handle)
            .ok_or_else(|| Error::Unknown("mesh program pipeline cache miss".into()))
    }
}
