use crate::ShaderDesc;

/// Descriptor for creating a [`ShaderProgram`](super::ShaderProgram).
///
/// Passed to [`ShaderProgram::new`](super::ShaderProgram::new). For convenience,
/// prefer the typed constructors (`from_inline_fragment`, `load_fragment`,
/// `load_compute`, etc.) over constructing this directly.
pub struct ShaderProgramDesc {
    pub fragment: ShaderDesc,
    pub vertex: Option<ShaderDesc>,
}
