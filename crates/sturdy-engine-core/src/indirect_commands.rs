use crate::{BufferHandle, IndirectCommandLayoutHandle, PipelineHandle};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum IndirectCommandToken {
    Draw,
    DrawIndexed,
    Dispatch,
    PushConstant { offset: u32, size: u32 },
    IndexBuffer,
    VertexBuffer { binding: u32 },
    Pipeline,
}

#[derive(Clone, Debug)]
pub struct IndirectCommandLayoutDesc {
    pub tokens: Vec<IndirectCommandToken>,
    pub stride: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DgcExecuteDesc {
    pub layout: IndirectCommandLayoutHandle,
    pub commands_buffer: BufferHandle,
    pub commands_offset: u64,
    pub max_command_count: u32,
    pub state_pipeline: Option<PipelineHandle>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DgcPreprocessDesc {
    pub layout: IndirectCommandLayoutHandle,
    pub input_buffer: BufferHandle,
    pub input_offset: u64,
    pub output_buffer: BufferHandle,
    pub output_offset: u64,
    pub max_command_count: u32,
}
