use std::path::{Path, PathBuf};

use crate::{
    ComputePipelineDesc, Engine, Pipeline, PipelineLayout, Result, Shader, ShaderCapabilityProfile,
    ShaderDesc, ShaderReflection, ShaderSource, ShaderStage,
};

pub struct ComputeProgram {
    pub(crate) engine: Engine,
    pub(crate) pipeline: Pipeline,
    pub(crate) pipeline_layout: PipelineLayout,
    pub(crate) shader: Shader,
    pub(crate) reflection: ShaderReflection,
    pub(crate) capability: ShaderCapabilityProfile,
    source_path: Option<PathBuf>,
    requires_ray_query: bool,
}

impl ComputeProgram {
    pub fn load(engine: &Engine, path: impl Into<PathBuf>) -> Result<Self> {
        Self::load_with_ray_query(engine, path, false)
    }

    pub fn load_ray_query(engine: &Engine, path: impl Into<PathBuf>) -> Result<Self> {
        Self::load_with_ray_query(engine, path, true)
    }

    fn load_with_ray_query(
        engine: &Engine,
        path: impl Into<PathBuf>,
        requires_ray_query: bool,
    ) -> Result<Self> {
        let path = path.into();
        let shader = engine.create_shader(ShaderDesc {
            source: ShaderSource::File(path.clone()),
            entry_point: "main".to_owned(),
            stage: ShaderStage::Compute,
            requires_ray_query,
            requires_cooperative_matrix: false,
            uses_ser: false,
        })?;
        let reflection = engine.shader_reflection(&shader)?;
        let capability =
            ShaderCapabilityProfile::from_reflection(&reflection, ShaderStage::Compute);
        let pipeline_layout = engine.create_reflected_compute_pipeline_layout(&shader)?;
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
            capability,
            source_path: Some(path),
            requires_ray_query,
        })
    }

    pub fn reflection(&self) -> &ShaderReflection {
        &self.reflection
    }

    pub fn capability_profile(&self) -> &ShaderCapabilityProfile {
        &self.capability
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
            requires_ray_query: self.requires_ray_query,
            requires_cooperative_matrix: false,
            uses_ser: false,
        })?;
        let reflection = self.engine.shader_reflection(&shader)?;
        let capability =
            ShaderCapabilityProfile::from_reflection(&reflection, ShaderStage::Compute);
        let pipeline_layout = self
            .engine
            .create_reflected_compute_pipeline_layout(&shader)?;
        let pipeline = self.engine.create_compute_pipeline(ComputePipelineDesc {
            shader: shader.handle(),
            layout: Some(pipeline_layout.handle()),
        })?;
        self.shader = shader;
        self.reflection = reflection;
        self.capability = capability;
        self.pipeline_layout = pipeline_layout;
        self.pipeline = pipeline;
        Ok(true)
    }
}
