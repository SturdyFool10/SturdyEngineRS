use crate::{
    ComputeProgram, Engine, Error, RenderFrame, RenderWorld, Result,
    shader_program::builtin_shader_path,
};

/// Dispatch result for [`RenderWorldGpuTransformBuildPass`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderWorldGpuTransformBuildStats {
    pub dispatched: bool,
    pub object_count: u32,
    pub workgroup_count: u32,
    pub objects_per_workgroup: u32,
    pub skipped_reason: Option<String>,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct TransformBuildConstants {
    object_count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

/// Compute pass that expands compact render-world transform sources into GPU-derived tables.
///
/// This pass is the execution half of `RenderWorld::prepare_gpu_transform_sources`: the CPU uploads
/// compact TRS/previous-TRS source data, then this compute shader writes current matrices, previous
/// matrices, normal matrices, and world-space bounds for later culling/draw paths.
pub struct RenderWorldGpuTransformBuildPass {
    program: ComputeProgram,
}

impl RenderWorldGpuTransformBuildPass {
    pub fn new(engine: &Engine) -> Result<Self> {
        Ok(Self {
            program: ComputeProgram::load(
                engine,
                builtin_shader_path("render_world_transform_build.slang"),
            )?,
        })
    }

    pub fn program(&self) -> &ComputeProgram {
        &self.program
    }

    pub fn execute(
        &self,
        frame: &RenderFrame,
        render_world: &RenderWorld,
    ) -> Result<RenderWorldGpuTransformBuildStats> {
        let Some(plan) = render_world.gpu_matrix_plan() else {
            return Err(Error::InvalidInput(
                "RenderWorld GPU transform build requires prepare_gpu_transform_sources first"
                    .into(),
            ));
        };

        if plan.object_count == 0 {
            return Ok(RenderWorldGpuTransformBuildStats {
                object_count: 0,
                skipped_reason: Some("no render-world transform sources".into()),
                ..RenderWorldGpuTransformBuildStats::default()
            });
        }
        if !plan.uses_gpu_generation {
            return Ok(RenderWorldGpuTransformBuildStats {
                object_count: plan.object_count,
                workgroup_count: plan.workgroup_count,
                objects_per_workgroup: plan.objects_per_workgroup,
                skipped_reason: Some(
                    plan.degraded_reason
                        .unwrap_or_else(|| "GPU matrix generation disabled".into()),
                ),
                ..RenderWorldGpuTransformBuildStats::default()
            });
        }

        let mut missing = Vec::new();
        render_world.with_gpu_transform_source_buffer(|buffer| match buffer {
            Some(buffer) => {
                frame.bind_buffer("transform_sources", buffer);
            }
            None => missing.push("transform_sources"),
        });
        render_world.with_gpu_current_matrix_buffer(|buffer| match buffer {
            Some(buffer) => {
                frame.bind_buffer("current_matrices", buffer);
            }
            None => missing.push("current_matrices"),
        });
        render_world.with_gpu_previous_matrix_buffer(|buffer| match buffer {
            Some(buffer) => {
                frame.bind_buffer("previous_matrices", buffer);
            }
            None => missing.push("previous_matrices"),
        });
        render_world.with_gpu_normal_matrix_buffer(|buffer| match buffer {
            Some(buffer) => {
                frame.bind_buffer("normal_matrices", buffer);
            }
            None => missing.push("normal_matrices"),
        });
        render_world.with_gpu_world_bounds_buffer(|buffer| match buffer {
            Some(buffer) => {
                frame.bind_buffer("world_bounds", buffer);
            }
            None => missing.push("world_bounds"),
        });

        if !missing.is_empty() {
            return Err(Error::ResourceStateCorruption(format!(
                "RenderWorld GPU transform build missing prepared buffers: {}",
                missing.join(", ")
            )));
        }

        let constants = TransformBuildConstants {
            object_count: plan.object_count,
            _pad0: 0,
            _pad1: 0,
            _pad2: 0,
        };
        frame.dispatch_compute_auto(
            "render_world_transform_build",
            &self.program,
            &constants,
            [plan.workgroup_count, 1, 1],
        )?;

        Ok(RenderWorldGpuTransformBuildStats {
            dispatched: true,
            object_count: plan.object_count,
            workgroup_count: plan.workgroup_count,
            objects_per_workgroup: plan.objects_per_workgroup,
            skipped_reason: None,
        })
    }
}
