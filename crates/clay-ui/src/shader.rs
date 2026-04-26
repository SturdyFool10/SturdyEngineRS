use sturdy_engine_core::{BufferHandle, ImageHandle, PipelineHandle, SamplerHandle, ShaderHandle};

use crate::UiColor;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ShaderKind {
    SolidColor,
    Gradient,
    Custom,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ShaderSlot {
    Background,
    Border,
    Outline,
    TextFill,
    TextOutline,
    Image,
    Mask,
    Shadow,
    Backdrop,
    Overlay,
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

#[derive(Clone, Debug, PartialEq)]
pub enum UiShaderUniformValue {
    Float(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Int(i32),
    Uint(u32),
    Bool(bool),
    Color(UiColor),
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiShaderUniform {
    pub name: String,
    pub value: UiShaderUniformValue,
}

impl UiShaderUniform {
    pub fn new(name: impl Into<String>, value: UiShaderUniformValue) -> Self {
        Self {
            name: name.into(),
            value,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UiShaderResourceRef {
    Image(ImageHandle),
    NamedImage(String),
    Buffer(BufferHandle),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiShaderResource {
    pub name: String,
    pub resource: UiShaderResourceRef,
    pub sampler: Option<SamplerHandle>,
}

impl UiShaderResource {
    pub fn image(
        name: impl Into<String>,
        image: ImageHandle,
        sampler: Option<SamplerHandle>,
    ) -> Self {
        Self {
            name: name.into(),
            resource: UiShaderResourceRef::Image(image),
            sampler,
        }
    }

    pub fn named_image(name: impl Into<String>, image_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            resource: UiShaderResourceRef::NamedImage(image_name.into()),
            sampler: None,
        }
    }

    pub fn buffer(name: impl Into<String>, buffer: BufferHandle) -> Self {
        Self {
            name: name.into(),
            resource: UiShaderResourceRef::Buffer(buffer),
            sampler: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiShaderSlotBinding {
    pub slot: ShaderSlot,
    pub shader: ShaderRef,
    pub uniforms: Vec<UiShaderUniform>,
    pub resources: Vec<UiShaderResource>,
}

impl UiShaderSlotBinding {
    pub fn new(slot: ShaderSlot, shader: ShaderRef) -> Self {
        Self {
            slot,
            shader,
            uniforms: Vec::new(),
            resources: Vec::new(),
        }
    }

    pub fn with_uniform(mut self, uniform: UiShaderUniform) -> Self {
        self.uniforms.push(uniform);
        self
    }

    pub fn with_resource(mut self, resource: UiShaderResource) -> Self {
        self.resources.push(resource);
        self
    }

    pub fn uniform(&self, name: &str) -> Option<&UiShaderUniform> {
        self.uniforms.iter().find(|uniform| uniform.name == name)
    }
}
