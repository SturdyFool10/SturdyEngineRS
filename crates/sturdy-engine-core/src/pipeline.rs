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

/// Polygon fill mode for rasterization.
///
/// `Line` and `Point` modes can be set dynamically when `BackendFeatures::extended_dynamic_state3`
/// is available; otherwise they require pipeline recreation.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum PolygonMode {
    /// Fill polygons — standard solid rendering.
    #[default]
    Fill,
    /// Wireframe rendering.
    Line,
    /// Point cloud rendering.
    Point,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RasterState {
    pub cull_mode: CullMode,
    pub front_face: FrontFace,
    /// Polygon fill mode. Default is `Fill`. Can be set dynamically when
    /// `BackendFeatures::extended_dynamic_state3_polygon_mode` is available.
    pub polygon_mode: PolygonMode,
    /// Clamp depth values to [0, 1] instead of discarding out-of-range fragments.
    /// Useful for shadow volume rendering and other techniques. Requires
    /// `BackendFeatures::extended_dynamic_state3` when set dynamically.
    pub depth_clamp: bool,
    /// Discard all primitives before rasterization (e.g. for transform feedback without rendering).
    /// Set dynamically via `cmd_set_rasterizer_discard_enable` when EDS3 is available.
    pub rasterizer_discard: bool,
}

impl Default for RasterState {
    fn default() -> Self {
        Self {
            cull_mode: CullMode::Back,
            front_face: FrontFace::CounterClockwise,
            polygon_mode: PolygonMode::Fill,
            depth_clamp: false,
            rasterizer_discard: false,
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

    pub const fn with_blend(format: Format, blend: BlendMode) -> Self {
        Self { format, blend }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum BlendMode {
    #[default]
    Opaque,
    Alpha,
    Negative,
}

/// Conservative rasterization mode for graphics pipelines.
///
/// Requires `BackendFeatures::conservative_rasterization_overestimate` or
/// `conservative_rasterization_underestimate` respectively.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum ConservativeRasterMode {
    /// Standard (non-conservative) rasterization.
    #[default]
    Off,
    /// Overestimate: a fragment is generated if the primitive partially covers the pixel.
    Overestimate,
    /// Underestimate: a fragment is generated only if the primitive fully covers the pixel.
    Underestimate,
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
    /// Conservative rasterization mode. Default is `Off`.
    pub conservative_raster: ConservativeRasterMode,
}
