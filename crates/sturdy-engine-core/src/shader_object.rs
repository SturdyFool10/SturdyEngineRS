use crate::{PipelineLayoutHandle, ShaderHandle};

/// A compiled standalone shader object that can be bound directly without a pipeline.
///
/// Requires `BackendFeatures::shader_object` (`VK_EXT_shader_object`).
#[derive(Clone, Debug)]
pub struct ShaderObjectDesc {
    /// The shader stage this object was compiled for.
    pub shader: ShaderHandle,
    /// Pipeline layout defining the descriptor set and push constant interfaces.
    pub layout: Option<PipelineLayoutHandle>,
}

/// Opaque handle to a compiled shader object.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ShaderObjectHandle(pub u64);
