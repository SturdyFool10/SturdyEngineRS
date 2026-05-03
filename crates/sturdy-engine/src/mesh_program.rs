use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use sturdy_engine_core as core;

use crate::{
    ColorTargetDesc, CullMode, Engine, Error, Format, FrontFace, GraphicsPipelineDesc, Pipeline,
    PipelineLayout, PrimitiveTopology, RasterState, Result, Shader, ShaderDesc, ShaderReflection,
    ShaderSource, ShaderStage, VertexAttributeDesc, VertexBufferLayout, VertexInputRate,
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

const UNLIT_FRAGMENT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/unlit_fragment.slang"
));

const LIT_FRAGMENT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/lit_fragment.slang"
));

pub struct MeshProgramDesc {
    pub fragment: ShaderDesc,
    /// Custom vertex shader. None uses the built-in for the chosen kind.
    pub vertex: Option<ShaderDesc>,
    pub vertex_kind: MeshVertexKind,
    pub alpha_blend: bool,
    /// Enable depth testing and writing with `D32Float`. Should be true for
    /// all 3D programs. The draw call must supply a matching depth attachment.
    pub uses_depth: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MeshVertexKind {
    V2d,
    V3d,
}

pub struct MeshProgram {
    pub(crate) engine: Engine,
    pub(crate) pipelines: Mutex<HashMap<(Format, u8, bool), Pipeline>>,
    /// Pipeline cache for multi-render-target variants.
    /// Key: (sorted color formats, samples, uses_depth).
    pub(crate) pipelines_mrt: Mutex<HashMap<(Vec<Format>, u8, bool), Pipeline>>,
    pub(crate) pipeline_layout: PipelineLayout,
    pub(crate) vertex: Shader,
    pub(crate) fragment: Shader,
    pub(crate) vertex_kind: MeshVertexKind,
    pub(crate) alpha_blend: bool,
    pub(crate) uses_depth: bool,
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
                uses_depth: false,
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
                uses_depth: false,
            },
        )
    }

    /// Built-in 3D program that colours each fragment by its world-space normal
    /// (remapped to [0, 1]).
    ///
    /// No shader files or push constants are needed — call this to get something
    /// visible on screen while iterating on geometry, camera, or scene setup.
    /// Depth testing is enabled; use with `Scene::draw` or supply a depth image.
    pub fn unlit(engine: &Engine) -> Result<Self> {
        Self::new(
            engine,
            MeshProgramDesc {
                fragment: ShaderDesc {
                    source: ShaderSource::Inline(UNLIT_FRAGMENT.to_owned()),
                    entry_point: "main".to_owned(),
                    stage: ShaderStage::Fragment,
                },
                vertex: None,
                vertex_kind: MeshVertexKind::V3d,
                alpha_blend: false,
                uses_depth: true,
            },
        )
    }

    /// Built-in 3D program with Lambert diffuse + Blinn-Phong specular shading.
    ///
    /// Requires a storage buffer named `"lighting"` containing one [`LightingUniforms`]
    /// element, which [`Scene::draw`] writes and binds automatically. Use this with
    /// [`Scene`] rather than driving the draw calls manually.
    ///
    /// Depth testing is enabled; supply a depth image (or use `Scene::draw`).
    ///
    /// [`LightingUniforms`]: crate::scene::DirectionalLight
    /// [`Scene`]: crate::scene::Scene
    /// [`Scene::draw`]: crate::scene::Scene::draw
    pub fn lit(engine: &Engine) -> Result<Self> {
        Self::new(
            engine,
            MeshProgramDesc {
                fragment: ShaderDesc {
                    source: ShaderSource::Inline(LIT_FRAGMENT.to_owned()),
                    entry_point: "main".to_owned(),
                    stage: ShaderStage::Fragment,
                },
                vertex: None,
                vertex_kind: MeshVertexKind::V3d,
                alpha_blend: false,
                uses_depth: true,
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
                uses_depth: true,
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
        let reflection = engine.graphics_shader_reflection(&vertex, Some(&fragment))?;
        let expected_attributes = match desc.vertex_kind {
            MeshVertexKind::V2d => vertex2d_attributes(),
            MeshVertexKind::V3d => vertex3d_attributes(),
        };
        validate_vertex_inputs_match_layout(&reflection, &expected_attributes)?;
        let pipeline_layout =
            engine.create_reflected_graphics_pipeline_layout(&vertex, Some(&fragment))?;
        Ok(Self {
            engine: engine.clone(),
            pipelines: Mutex::new(HashMap::new()),
            pipelines_mrt: Mutex::new(HashMap::new()),
            pipeline_layout,
            vertex,
            fragment,
            vertex_kind: desc.vertex_kind,
            alpha_blend: desc.alpha_blend,
            uses_depth: desc.uses_depth,
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
        let reflection = self
            .engine
            .graphics_shader_reflection(&self.vertex, Some(&fragment))?;
        let pipeline_layout = self
            .engine
            .create_reflected_graphics_pipeline_layout(&self.vertex, Some(&fragment))?;
        self.fragment = fragment;
        self.reflection = reflection;
        self.pipeline_layout = pipeline_layout;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.pipelines
            .lock()
            .expect("mesh program pipeline mutex poisoned")
            .clear();
        self.pipelines_mrt
            .lock()
            .expect("mesh program MRT pipeline mutex poisoned")
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
        let reflection = self
            .engine
            .graphics_shader_reflection(&vertex, Some(&self.fragment))?;
        let pipeline_layout = self
            .engine
            .create_reflected_graphics_pipeline_layout(&vertex, Some(&self.fragment))?;
        self.vertex = vertex;
        self.reflection = reflection;
        self.pipeline_layout = pipeline_layout;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.pipelines
            .lock()
            .expect("mesh program pipeline mutex poisoned")
            .clear();
        self.pipelines_mrt
            .lock()
            .expect("mesh program MRT pipeline mutex poisoned")
            .clear();
        Ok(true)
    }

    pub(crate) fn pipeline_handle(
        &self,
        format: Format,
        samples: u8,
    ) -> Result<core::PipelineHandle> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut pipelines = self
            .pipelines
            .lock()
            .expect("mesh program pipeline mutex poisoned");
        let key = (format, samples.max(1), self.uses_depth);
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
                depth_format: if self.uses_depth {
                    Some(Format::Depth32Float)
                } else {
                    None
                },
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

    /// Return a pipeline handle for a multi-render-target draw.
    ///
    /// `color_formats` is the ordered list of render target formats (e.g.
    /// `[Rgba8Unorm, Rgba16Float, Rgba16Float, Rgba16Float]` for a G-Buffer fill).
    /// The fragment shader must write to `SV_TARGET0..N-1` in the same order.
    pub(crate) fn pipeline_handle_mrt(
        &self,
        color_formats: &[Format],
        samples: u8,
    ) -> Result<core::PipelineHandle> {
        let mut pipelines = self
            .pipelines_mrt
            .lock()
            .expect("mesh program MRT pipeline mutex poisoned");
        let key = (color_formats.to_vec(), samples.max(1), self.uses_depth);
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
            let color_targets = color_formats
                .iter()
                .map(|&fmt| ColorTargetDesc::opaque(fmt))
                .collect();
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
                color_targets,
                depth_format: if self.uses_depth {
                    Some(Format::Depth32Float)
                } else {
                    None
                },
                samples: key.1,
                topology: PrimitiveTopology::TriangleList,
                raster: RasterState {
                    cull_mode: CullMode::None,
                    front_face: FrontFace::CounterClockwise,
                },
            })?;
            pipeline.set_debug_name("mesh-program-mrt")?;
            pipelines.insert(key.clone(), pipeline);
        }
        pipelines
            .get(&key)
            .map(Pipeline::handle)
            .ok_or_else(|| Error::Unknown("mesh program MRT pipeline cache miss".into()))
    }
}

/// Validate that every reflected vertex input attribute in `reflection` has a
/// matching entry in `layout` with the same location and format.
///
/// Only runs when the shader actually exposes vertex input reflection (non-empty
/// `vertex_inputs`). Skips silently if the vertex shader did not produce inputs
/// (e.g. built-in fullscreen triangle vertex shaders).
fn validate_vertex_inputs_match_layout(
    reflection: &ShaderReflection,
    layout: &[VertexAttributeDesc],
) -> Result<()> {
    if reflection.vertex_inputs.is_empty() {
        return Ok(());
    }
    for input in &reflection.vertex_inputs {
        let declared = layout.iter().find(|a| a.location == input.location);
        match declared {
            None => {
                return Err(Error::InvalidInput(format!(
                    "vertex shader input '{}' at location {} has no matching attribute in the declared vertex layout",
                    input.name, input.location
                )));
            }
            Some(attr) if attr.format != input.format => {
                return Err(Error::InvalidInput(format!(
                    "vertex shader input '{}' at location {} expects {:?} but the declared layout has {:?}",
                    input.name, input.location, input.format, attr.format
                )));
            }
            _ => {}
        }
    }
    Ok(())
}
