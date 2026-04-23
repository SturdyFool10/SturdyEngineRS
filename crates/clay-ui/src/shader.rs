use sturdy_engine_core::{PipelineHandle, ShaderHandle};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ShaderKind {
    SolidColor,
    Gradient,
    Custom,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ShaderSlot {
    Background,
    Outline,
    TextFill,
    TextOutline,
    Image,
    Custom,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ShaderRef {
    pub shader: Option<ShaderHandle>,
    pub pipeline: Option<PipelineHandle>,
    pub kind: ShaderKind,
}

impl ShaderRef {
    pub const SOLID_COLOR: Self = Self {
        shader: None,
        pipeline: None,
        kind: ShaderKind::SolidColor,
    };

    pub const fn custom(shader: ShaderHandle, pipeline: PipelineHandle) -> Self {
        Self {
            shader: Some(shader),
            pipeline: Some(pipeline),
            kind: ShaderKind::Custom,
        }
    }

    pub const fn gradient(shader: ShaderHandle, pipeline: PipelineHandle) -> Self {
        Self {
            shader: Some(shader),
            pipeline: Some(pipeline),
            kind: ShaderKind::Gradient,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShaderBinding<T> {
    pub slot: ShaderSlot,
    pub shader: ShaderRef,
    pub data: T,
}

impl<T> ShaderBinding<T> {
    pub const fn new(slot: ShaderSlot, shader: ShaderRef, data: T) -> Self {
        Self { slot, shader, data }
    }
}
