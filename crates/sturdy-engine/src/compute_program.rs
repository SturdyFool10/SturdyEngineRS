use std::path::PathBuf;

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
}

impl ComputeProgram {
    pub fn load(engine: &Engine, path: impl Into<PathBuf>) -> Result<Self> {
        let shader = engine.create_shader(ShaderDesc {
            source: ShaderSource::File(path.into()),
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
        })
    }

    pub fn reflection(&self) -> &ShaderReflection {
        &self.reflection
    }
}
