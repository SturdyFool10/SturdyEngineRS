use crate::{Format, PipelineLayoutHandle, ShaderHandle};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ComputePipelineDesc {
    pub shader: ShaderHandle,
    /// `None` causes the device to derive the layout from shader reflection automatically.
    pub layout: Option<PipelineLayoutHandle>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PrimitiveTopology {
    TriangleList,
    TriangleStrip,
    LineList,
    LineStrip,
    PointList,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum VertexFormat {
    Float32x2,
    Float32x3,
    Float32x4,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum VertexInputRate {
    Vertex,
    Instance,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct VertexBufferLayout {
    pub binding: u32,
    pub stride: u32,
    pub input_rate: VertexInputRate,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct VertexAttributeDesc {
    pub location: u32,
    pub binding: u32,
    pub format: VertexFormat,
    pub offset: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CullMode {
    None,
    Front,
    Back,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FrontFace {
    CounterClockwise,
    Clockwise,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RasterState {
    pub cull_mode: CullMode,
    pub front_face: FrontFace,
}

impl Default for RasterState {
    fn default() -> Self {
        Self {
            cull_mode: CullMode::Back,
            front_face: FrontFace::CounterClockwise,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ColorTargetDesc {
    pub format: Format,
    pub blend: BlendMode,
}

impl ColorTargetDesc {
    pub const fn opaque(format: Format) -> Self {
        Self {
            format,
            blend: BlendMode::Opaque,
        }
    }

    pub const fn alpha_blend(format: Format) -> Self {
        Self {
            format,
            blend: BlendMode::Alpha,
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum BlendMode {
    #[default]
    Opaque,
    Alpha,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphicsPipelineDesc {
    pub vertex_shader: ShaderHandle,
    pub fragment_shader: Option<ShaderHandle>,
    /// `None` causes the device to derive the layout from shader reflection automatically.
    pub layout: Option<PipelineLayoutHandle>,
    pub vertex_buffers: Vec<VertexBufferLayout>,
    pub vertex_attributes: Vec<VertexAttributeDesc>,
    pub color_targets: Vec<ColorTargetDesc>,
    pub depth_format: Option<Format>,
    pub samples: u8,
    pub topology: PrimitiveTopology,
    pub raster: RasterState,
}
