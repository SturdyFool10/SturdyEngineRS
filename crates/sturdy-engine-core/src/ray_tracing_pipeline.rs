use crate::{PipelineLayoutHandle, ShaderHandle};

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
}
