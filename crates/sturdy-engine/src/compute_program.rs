use std::path::{Path, PathBuf};

use crate::{
    ComputePipelineDesc, Engine, Pipeline, PipelineLayout, Result, Shader, ShaderDesc,
    ShaderReflection, ShaderSource, ShaderStage,
};

pub struct ComputeProgram {
    #[allow(dead_code)]
    pub(crate) engine: Engine,
    pub(crate) pipeline: Pipeline,
    pub(crate) pipeline_layout: PipelineLayout,
    pub(crate) shader: Shader,
    pub(crate) reflection: ShaderReflection,
    source_path: Option<PathBuf>,
}

impl ComputeProgram {
    pub fn load(engine: &Engine, path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let shader = engine.create_shader(ShaderDesc {
            source: ShaderSource::File(path.clone()),
            entry_point: "main".to_owned(),
            stage: ShaderStage::Compute,
        })?;
        let reflection = engine.shader_reflection(&shader)?;
        let pipeline_layout = engine.create_pipeline_layout(reflection.layout.clone())?;
        let pipeline = engine.create_compute_pipeline(ComputePipelineDesc {
            shader: shader.handle(),
            layout: Some(pipeline_layout.handle()),
        })?;
        Ok(Self {
            engine: engine.clone(),
            pipeline,
            pipeline_layout,
            shader,
            reflection,
            source_path: Some(path),
        })
    }

    pub fn reflection(&self) -> &ShaderReflection {
        &self.reflection
    }

    /// Return the source file path if this program was loaded from a file.
    pub fn source_path(&self) -> Option<&Path> {
        self.source_path.as_deref()
    }

    /// Recompile from the original source file and replace the internal pipeline.
    ///
    /// Returns `Ok(true)` when the pipeline was successfully reloaded, `Ok(false)`
    /// when there is no source file path to reload from, and `Err` when compilation
    /// fails. On failure the previous pipeline remains active.
    pub fn reload(&mut self) -> Result<bool> {
        let path = match &self.source_path {
            Some(p) => p.clone(),
            None => return Ok(false),
        };
        let shader = self.engine.create_shader(ShaderDesc {
            source: ShaderSource::File(path),
            entry_point: "main".to_owned(),
            stage: ShaderStage::Compute,
        })?;
        let reflection = self.engine.shader_reflection(&shader)?;
        let pipeline_layout = self
            .engine
            .create_pipeline_layout(reflection.layout.clone())?;
        let pipeline = self.engine.create_compute_pipeline(ComputePipelineDesc {
            shader: shader.handle(),
            layout: Some(pipeline_layout.handle()),
        })?;
        self.shader = shader;
        self.reflection = reflection;
        self.pipeline_layout = pipeline_layout;
        self.pipeline = pipeline;
        Ok(true)
    }
}
