use sturdy_engine_core::{BufferHandle, ImageHandle, PipelineHandle, SamplerHandle, ShaderHandle};

use crate::UiColor;

pub const UI_SHADER_PUSH_CONSTANT_LIMIT: usize = 128;
pub const UI_SHADER_PARAMETER_ALIGNMENT: usize = 16;

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
pub enum UiShaderUniformPackError {
    DuplicateUniform(String),
    PushConstantLimitExceeded { size: usize, limit: usize },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiShaderUniformEntry {
    pub name: String,
    pub offset: u32,
    pub size: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiShaderUniformPacket {
    pub entries: Vec<UiShaderUniformEntry>,
    pub bytes: Vec<u8>,
}

impl UiShaderUniformPacket {
    pub fn pack(uniforms: &[UiShaderUniform]) -> Result<Self, UiShaderUniformPackError> {
        let mut entries = Vec::with_capacity(uniforms.len());
        let mut bytes = Vec::new();

        for uniform in uniforms {
            if entries
                .iter()
                .any(|entry: &UiShaderUniformEntry| entry.name == uniform.name)
            {
                return Err(UiShaderUniformPackError::DuplicateUniform(
                    uniform.name.clone(),
                ));
            }

            align_bytes(&mut bytes, 4);
            let offset = bytes.len() as u32;
            uniform.value.write_bytes(&mut bytes);
            align_bytes(&mut bytes, 4);
            entries.push(UiShaderUniformEntry {
                name: uniform.name.clone(),
                offset,
                size: bytes.len() as u32 - offset,
            });
        }

        Ok(Self { entries, bytes })
    }

    pub fn pack_push_constants(
        uniforms: &[UiShaderUniform],
    ) -> Result<Self, UiShaderUniformPackError> {
        let packet = Self::pack(uniforms)?;

        if packet.bytes.len() > UI_SHADER_PUSH_CONSTANT_LIMIT {
            return Err(UiShaderUniformPackError::PushConstantLimitExceeded {
                size: packet.bytes.len(),
                limit: UI_SHADER_PUSH_CONSTANT_LIMIT,
            });
        }

        Ok(packet)
    }

    pub fn entry(&self, name: &str) -> Option<&UiShaderUniformEntry> {
        self.entries.iter().find(|entry| entry.name == name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiShaderParameterRecord {
    pub command_index: usize,
    pub offset: u32,
    pub size: u32,
    pub entries: Vec<UiShaderUniformEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiShaderParameterBatch {
    pub records: Vec<UiShaderParameterRecord>,
    pub bytes: Vec<u8>,
}

impl UiShaderParameterBatch {
    pub fn pack_commands<'a>(
        commands: impl IntoIterator<Item = (usize, &'a [UiShaderUniform])>,
    ) -> Result<Self, UiShaderUniformPackError> {
        let mut records = Vec::new();
        let mut bytes = Vec::new();

        for (command_index, uniforms) in commands {
            if uniforms.is_empty() {
                continue;
            }

            align_bytes(&mut bytes, UI_SHADER_PARAMETER_ALIGNMENT);
            let offset = bytes.len() as u32;
            let packet = UiShaderUniformPacket::pack(uniforms)?;
            let size = packet.bytes.len() as u32;
            bytes.extend_from_slice(&packet.bytes);
            records.push(UiShaderParameterRecord {
                command_index,
                offset,
                size,
                entries: packet.entries,
            });
        }

        align_bytes(&mut bytes, UI_SHADER_PARAMETER_ALIGNMENT);
        Ok(Self { records, bytes })
    }

    pub fn record_for_command(&self, command_index: usize) -> Option<&UiShaderParameterRecord> {
        self.records
            .iter()
            .find(|record| record.command_index == command_index)
    }
}

impl UiShaderUniformValue {
    fn write_bytes(&self, bytes: &mut Vec<u8>) {
        match self {
            Self::Float(value) => push_f32(bytes, *value),
            Self::Vec2(value) => {
                for component in value {
                    push_f32(bytes, *component);
                }
            }
            Self::Vec3(value) => {
                for component in value {
                    push_f32(bytes, *component);
                }
            }
            Self::Vec4(value) => {
                for component in value {
                    push_f32(bytes, *component);
                }
            }
            Self::Int(value) => bytes.extend_from_slice(&value.to_ne_bytes()),
            Self::Uint(value) => bytes.extend_from_slice(&value.to_ne_bytes()),
            Self::Bool(value) => bytes.extend_from_slice(&u32::from(*value).to_ne_bytes()),
            Self::Color(color) => {
                for component in color.to_f32_array() {
                    push_f32(bytes, component);
                }
            }
        }
    }
}

fn push_f32(bytes: &mut Vec<u8>, value: f32) {
    bytes.extend_from_slice(&value.to_ne_bytes());
}

fn align_bytes(bytes: &mut Vec<u8>, alignment: usize) {
    while bytes.len() % alignment != 0 {
        bytes.push(0);
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

    pub fn push_constant_packet(&self) -> Result<UiShaderUniformPacket, UiShaderUniformPackError> {
        UiShaderUniformPacket::pack_push_constants(&self.uniforms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_uniforms_with_stable_offsets() {
        let packet = UiShaderUniformPacket::pack_push_constants(&[
            UiShaderUniform::new("amount", UiShaderUniformValue::Float(0.5)),
            UiShaderUniform::new("offset", UiShaderUniformValue::Vec2([1.0, 2.0])),
            UiShaderUniform::new("enabled", UiShaderUniformValue::Bool(true)),
        ])
        .unwrap();

        assert_eq!(packet.entry("amount").unwrap().offset, 0);
        assert_eq!(packet.entry("offset").unwrap().offset, 4);
        assert_eq!(packet.entry("enabled").unwrap().offset, 12);
        assert_eq!(packet.bytes.len(), 16);
    }

    #[test]
    fn rejects_duplicate_uniform_names() {
        let err = UiShaderUniformPacket::pack_push_constants(&[
            UiShaderUniform::new("amount", UiShaderUniformValue::Float(0.5)),
            UiShaderUniform::new("amount", UiShaderUniformValue::Float(1.0)),
        ])
        .unwrap_err();

        assert_eq!(
            err,
            UiShaderUniformPackError::DuplicateUniform("amount".into())
        );
    }

    #[test]
    fn rejects_push_constant_packets_over_limit() {
        let uniforms = (0..9)
            .map(|index| {
                UiShaderUniform::new(format!("v{index}"), UiShaderUniformValue::Vec4([0.0; 4]))
            })
            .collect::<Vec<_>>();

        let err = UiShaderUniformPacket::pack_push_constants(&uniforms).unwrap_err();

        assert_eq!(
            err,
            UiShaderUniformPackError::PushConstantLimitExceeded {
                size: 144,
                limit: UI_SHADER_PUSH_CONSTANT_LIMIT
            }
        );
    }

    #[test]
    fn packs_parameter_batch_records_with_aligned_command_offsets() {
        let batch = UiShaderParameterBatch::pack_commands([
            (
                3,
                &[
                    UiShaderUniform::new("amount", UiShaderUniformValue::Float(0.5)),
                    UiShaderUniform::new("offset", UiShaderUniformValue::Vec2([1.0, 2.0])),
                ][..],
            ),
            (
                8,
                &[UiShaderUniform::new(
                    "color",
                    UiShaderUniformValue::Vec4([1.0, 0.0, 0.0, 1.0]),
                )][..],
            ),
        ])
        .unwrap();

        assert_eq!(batch.records.len(), 2);
        assert_eq!(batch.records[0].command_index, 3);
        assert_eq!(batch.records[0].offset, 0);
        assert_eq!(batch.records[0].size, 12);
        assert_eq!(batch.records[1].command_index, 8);
        assert_eq!(batch.records[1].offset, 16);
        assert_eq!(batch.records[1].size, 16);
        assert_eq!(batch.bytes.len(), 32);
    }
}
