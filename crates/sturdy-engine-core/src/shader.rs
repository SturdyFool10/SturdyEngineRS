use std::path::{Path, PathBuf};

use crate::{BindingKind, CanonicalPipelineLayout, Error, Result, StageMask, UpdateRate};

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
    /// Slang source stored in memory. Kept as the ergonomic default for generated shaders.
    Inline(String),
    /// Slang source loaded from a native development file path.
    File(PathBuf),
    /// Slang source loaded from a borrowed native development file path.
    FilePath(&'static Path),
    /// Slang source addressed through the engine asset system. Runtime compilation
    /// requires an asset resolver; direct device creation rejects unresolved virtual paths.
    VirtualAssetPath(&'static Path),
    /// Borrowed UTF-8 Slang source, including `include_str!` output.
    MemoryUtf8(&'static str),
    /// Borrowed bytes, including `include_bytes!` output. UTF-8 bytes are compiled
    /// as Slang source; SPIR-V bytes are accepted for SPIR-V targets.
    MemoryBytes(&'static [u8]),
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
    pub parameters: Vec<ShaderParameterReflection>,
    /// Vertex input attributes reflected from a vertex shader's SPIR-V.
    /// Empty for fragment and compute shaders.
    pub vertex_inputs: Vec<VertexInputReflection>,
}

/// One vertex shader input attribute as reflected from SPIR-V.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VertexInputReflection {
    pub name: String,
    pub location: u32,
    pub format: crate::VertexFormat,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ShaderResourceAccess {
    Read,
    Write,
    ReadWrite,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShaderParameterKind {
    Resource(BindingKind),
    PushConstant,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShaderParameterReflection {
    pub name: String,
    pub kind: ShaderParameterKind,
    pub stage_mask: StageMask,
    pub access: ShaderResourceAccess,
    pub set: Option<u32>,
    pub binding: Option<u32>,
    pub count: u32,
    pub update_rate: Option<UpdateRate>,
    pub size_bytes: Option<u32>,
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
            ShaderSource::MemoryUtf8(source) if source.trim().is_empty() => Err(
                Error::InvalidInput("shader source must be non-empty".into()),
            ),
            ShaderSource::File(path) if path.as_os_str().is_empty() => Err(Error::InvalidInput(
                "shader file path must be non-empty".into(),
            )),
            ShaderSource::FilePath(path) if path.as_os_str().is_empty() => Err(
                Error::InvalidInput("shader file path must be non-empty".into()),
            ),
            ShaderSource::VirtualAssetPath(path) if path.as_os_str().is_empty() => Err(
                Error::InvalidInput("shader virtual asset path must be non-empty".into()),
            ),
            ShaderSource::MemoryBytes(bytes) if bytes.is_empty() => Err(Error::InvalidInput(
                "shader byte source must be non-empty".into(),
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
