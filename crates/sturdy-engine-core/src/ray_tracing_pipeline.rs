use crate::{PipelineHandle, PipelineLayoutHandle, ShaderHandle};

/// A single shader stage in a ray-tracing pipeline.
#[derive(Copy, Clone, Debug)]
pub struct RayTracingStageDesc {
    pub shader: ShaderHandle,
}

/// Kind of shader group in a ray-tracing pipeline.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RtShaderGroupKind {
    /// A single raygen, miss, or callable shader.
    General,
    /// Closest-hit (and optional any-hit) for triangle geometry.
    TrianglesHit,
    /// Intersection + optional closest-hit + optional any-hit for procedural geometry.
    ProceduralHit,
}

/// One shader group within the pipeline. Indices reference `RayTracingPipelineDesc::stages`.
/// Use `u32::MAX` (= `VK_SHADER_UNUSED_KHR`) for absent shaders.
#[derive(Copy, Clone, Debug)]
pub struct RtShaderGroupDesc {
    pub kind: RtShaderGroupKind,
    pub general_shader: u32,
    pub closest_hit_shader: u32,
    pub any_hit_shader: u32,
    pub intersection_shader: u32,
}

impl RtShaderGroupDesc {
    pub const UNUSED: u32 = u32::MAX;

    pub fn general(stage_index: u32) -> Self {
        Self {
            kind: RtShaderGroupKind::General,
            general_shader: stage_index,
            closest_hit_shader: Self::UNUSED,
            any_hit_shader: Self::UNUSED,
            intersection_shader: Self::UNUSED,
        }
    }

    pub fn triangles_hit(closest_hit: u32, any_hit: u32) -> Self {
        Self {
            kind: RtShaderGroupKind::TrianglesHit,
            general_shader: Self::UNUSED,
            closest_hit_shader: closest_hit,
            any_hit_shader: any_hit,
            intersection_shader: Self::UNUSED,
        }
    }
}

/// Descriptor for creating a ray-tracing pipeline.
#[derive(Clone, Debug)]
pub struct RayTracingPipelineDesc {
    pub stages: Vec<RayTracingStageDesc>,
    pub groups: Vec<RtShaderGroupDesc>,
    pub max_recursion_depth: u32,
    pub layout: Option<PipelineLayoutHandle>,
    /// When `true` and `VK_KHR_pipeline_library` is available, pre-compile hit-group shader
    /// libraries once per material and link them into the final pipeline at TLAS build time.
    /// Reduces recompilation cost when only hit shaders change across scene RT pipelines.
    pub use_pipeline_libraries: bool,
}

/// Shader group indices to pack into a shader binding table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShaderBindingTableDesc {
    pub pipeline: PipelineHandle,
    pub raygen_group: u32,
    pub miss_groups: Vec<u32>,
    pub hit_groups: Vec<u32>,
    pub callable_groups: Vec<u32>,
}

/// Backend-reported alignment rules for shader binding table packing.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ShaderBindingTableProperties {
    pub shader_group_handle_size: u32,
    pub shader_group_handle_alignment: u32,
    pub shader_group_base_alignment: u32,
    pub max_shader_group_stride: u32,
}
