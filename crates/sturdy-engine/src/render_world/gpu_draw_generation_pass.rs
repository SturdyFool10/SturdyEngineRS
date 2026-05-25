use crate::{
    ComputeProgram, Engine, Error, RenderFrame, RenderWorld, Result,
    shader_program::builtin_shader_path,
};

use super::RenderWorldGpuDrawGenerationDispatchStats;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct DrawGenerationConstants {
    bin_count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

/// GPU pass that converts render-world visibility flags and persistent bins into indirect draws.
pub struct RenderWorldGpuDrawGenerationPass {
    program: ComputeProgram,
}

impl RenderWorldGpuDrawGenerationPass {
    pub fn new(engine: &Engine) -> Result<Self> {
        Ok(Self {
            program: ComputeProgram::load(
                engine,
                builtin_shader_path("render_world_draw_generate.slang"),
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
    ) -> Result<RenderWorldGpuDrawGenerationDispatchStats> {
        let Some(plan) = render_world.gpu_draw_generation_plan() else {
            return Err(Error::InvalidInput(
                "RenderWorld GPU draw generation requires prepare_gpu_draw_generation first".into(),
            ));
        };

        if plan.bin_count == 0 || plan.workgroup_count == 0 {
            return Ok(RenderWorldGpuDrawGenerationDispatchStats {
                skipped_reason: Some(
                    "no persistent render-world bins to generate draws for".into(),
                ),
                ..RenderWorldGpuDrawGenerationDispatchStats::default()
            });
        }

        let mut missing = Vec::new();
        render_world.with_gpu_draw_bin_buffer(|buffer| match buffer {
            Some(buffer) => {
                frame.bind_buffer("render_world_bins", buffer);
            }
            None => missing.push("render_world_bins"),
        });
        render_world.with_gpu_visibility_flags_buffer(|buffer| match buffer {
            Some(buffer) => {
                frame.bind_buffer("visible_object_flags", buffer);
            }
            None => missing.push("visible_object_flags"),
        });
        render_world.with_gpu_draw_indirect_buffer(|buffer| match buffer {
            Some(buffer) => {
                frame.bind_buffer("render_world_indirect_commands", buffer);
            }
            None => missing.push("render_world_indirect_commands"),
        });
        render_world.with_gpu_draw_count_buffer(|buffer| match buffer {
            Some(buffer) => {
                let zero = 0_u32;
                buffer.write_pod(0, &zero)?;
                frame.bind_buffer("render_world_visible_draw_count", buffer);
                Ok(())
            }
            None => {
                missing.push("render_world_visible_draw_count");
                Ok(())
            }
        })?;
        render_world.with_gpu_visible_instance_buffer(|buffer| match buffer {
            Some(buffer) => {
                frame.bind_buffer("render_world_visible_instances", buffer);
            }
            None => missing.push("render_world_visible_instances"),
        });

        if !missing.is_empty() {
            return Err(Error::ResourceStateCorruption(format!(
                "RenderWorld GPU draw generation missing prepared buffers: {}",
                missing.join(", ")
            )));
        }

        let constants = DrawGenerationConstants {
            bin_count: plan.bin_count,
            _pad0: 0,
            _pad1: 0,
            _pad2: 0,
        };
        frame.dispatch_compute_auto(
            "render_world_draw_generate",
            &self.program,
            &constants,
            [plan.workgroup_count, 1, 1],
        )?;

        Ok(RenderWorldGpuDrawGenerationDispatchStats {
            dispatched: true,
            bin_count: plan.bin_count,
            object_count: plan.object_count,
            workgroup_count: plan.workgroup_count,
            bins_per_workgroup: plan.bins_per_workgroup,
            skipped_reason: None,
        })
    }
}
