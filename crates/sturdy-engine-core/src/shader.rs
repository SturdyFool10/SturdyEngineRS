use std::path::PathBuf;

use crate::{CanonicalPipelineLayout, Error, Result};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ShaderStage {
    Vertex,
    Fragment,
    Compute,
    Mesh,
    Task,
    RayGeneration,
    Miss,
    ClosestHit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShaderSource {
    Inline(String),
    File(PathBuf),
    Spirv(Vec<u32>),
    /// Pre-compiled DXIL bytecode for D3D12 backends.
    Dxil(Vec<u8>),
    /// Pre-compiled MSL source or Metal library bytes for Metal backends.
    Msl(Vec<u8>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShaderDesc {
    pub source: ShaderSource,
    pub entry_point: String,
    pub stage: ShaderStage,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ShaderTarget {
    Spirv,
    Dxil,
    Msl,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledShaderArtifact {
    pub target: ShaderTarget,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ShaderReflection {
    pub layout: CanonicalPipelineLayout,
    pub entry_points: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShaderModule {
    pub desc: ShaderDesc,
    pub reflection: ShaderReflection,
    pub artifacts: Vec<CompiledShaderArtifact>,
}

impl ShaderDesc {
    pub fn validate(&self) -> Result<()> {
        if self.entry_point.trim().is_empty() {
            return Err(Error::InvalidInput(
                "shader entry_point must be non-empty".into(),
            ));
        }
        match &self.source {
            ShaderSource::Inline(source) if source.trim().is_empty() => Err(Error::InvalidInput(
                "shader source must be non-empty".into(),
            )),
            ShaderSource::File(path) if path.as_os_str().is_empty() => Err(Error::InvalidInput(
                "shader file path must be non-empty".into(),
            )),
            ShaderSource::Spirv(words) if words.is_empty() => Err(Error::InvalidInput(
                "SPIR-V shader source must be non-empty".into(),
            )),
            ShaderSource::Spirv(words) if words.first().copied() != Some(0x0723_0203) => Err(
                Error::InvalidInput("SPIR-V shader source has an invalid magic number".into()),
            ),
            ShaderSource::Dxil(bytes) if bytes.is_empty() => Err(Error::InvalidInput(
                "DXIL shader source must be non-empty".into(),
            )),
            ShaderSource::Msl(bytes) if bytes.is_empty() => Err(Error::InvalidInput(
                "MSL shader source must be non-empty".into(),
            )),
            _ => Ok(()),
        }
    }
}
