use glam::Mat4;

use crate::{
    ComputeProgram, Engine, Error, Frustum, RenderFrame, RenderWorld, Result,
    shader_program::builtin_shader_path,
};

use super::{RenderWorldGpuCullDispatchStats, RenderWorldGpuCullSettings};

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct RenderWorldCullConstants {
    frustum_planes: [[f32; 4]; 6],
    object_count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

/// Scene-wide GPU culling pass for the persistent render world.
///
/// This pass reads the GPU-derived `world_bounds` table produced by
/// [`RenderWorldGpuTransformBuildPass`](super::RenderWorldGpuTransformBuildPass) and writes one
/// visibility flag per object. Later draw-generation/compaction passes can consume that compact
/// visibility result without issuing per-batch cull dispatches.
pub struct RenderWorldGpuCullPass {
    program: ComputeProgram,
    settings: RenderWorldGpuCullSettings,
}

impl RenderWorldGpuCullPass {
    pub fn new(engine: &Engine) -> Result<Self> {
        Self::with_settings(engine, RenderWorldGpuCullSettings::default())
    }

    pub fn with_settings(engine: &Engine, settings: RenderWorldGpuCullSettings) -> Result<Self> {
        Ok(Self {
            program: ComputeProgram::load(engine, builtin_shader_path("render_world_cull.slang"))?,
            settings,
        })
    }

    pub fn settings(&self) -> RenderWorldGpuCullSettings {
        self.settings
    }

    pub fn settings_mut(&mut self) -> &mut RenderWorldGpuCullSettings {
        &mut self.settings
    }

    pub fn program(&self) -> &ComputeProgram {
        &self.program
    }

    pub fn execute(
        &self,
        frame: &RenderFrame,
        render_world: &RenderWorld,
        view_proj: Mat4,
    ) -> Result<RenderWorldGpuCullDispatchStats> {
        let Some(plan) = render_world.gpu_cull_plan() else {
            return Err(Error::InvalidInput(
                "RenderWorld GPU cull requires prepare_gpu_cull_outputs first".into(),
            ));
        };

        if plan.workgroup_count == 0 || plan.dispatch_count == 0 {
            return Ok(RenderWorldGpuCullDispatchStats {
                object_count: 0,
                skipped_reason: Some("no render-world objects to cull".into()),
                ..RenderWorldGpuCullDispatchStats::default()
            });
        }
        if !plan.uses_gpu_culling {
            return Ok(RenderWorldGpuCullDispatchStats {
                object_count: render_world
                    .gpu_matrix_plan()
                    .map(|p| p.object_count)
                    .unwrap_or(0),
                workgroup_count: plan.workgroup_count,
                objects_per_workgroup: plan.objects_per_workgroup,
                skipped_reason: Some(
                    plan.degraded_reason
                        .unwrap_or_else(|| "GPU render-world culling disabled".into()),
                ),
                ..RenderWorldGpuCullDispatchStats::default()
            });
        }
        if !plan.single_dispatch {
            return Ok(RenderWorldGpuCullDispatchStats {
                object_count: render_world
                    .gpu_matrix_plan()
                    .map(|p| p.object_count)
                    .unwrap_or(0),
                workgroup_count: plan.workgroup_count,
                objects_per_workgroup: plan.objects_per_workgroup,
                skipped_reason: Some("single-dispatch render-world culling disabled".into()),
                ..RenderWorldGpuCullDispatchStats::default()
            });
        }

        let object_count = render_world
            .gpu_matrix_plan()
            .map(|p| p.object_count)
            .unwrap_or(0);
        if object_count == 0 {
            return Ok(RenderWorldGpuCullDispatchStats {
                skipped_reason: Some("no render-world bounds to cull".into()),
                ..RenderWorldGpuCullDispatchStats::default()
            });
        }

        let mut missing = Vec::new();
        render_world.with_gpu_world_bounds_buffer(|buffer| match buffer {
            Some(buffer) => {
                frame.bind_buffer("world_bounds", buffer);
            }
            None => missing.push("world_bounds"),
        });
        render_world.with_gpu_visibility_flags_buffer(|buffer| match buffer {
            Some(buffer) => {
                frame.bind_buffer("visible_object_flags", buffer);
            }
            None => missing.push("visible_object_flags"),
        });
        if !missing.is_empty() {
            return Err(Error::ResourceStateCorruption(format!(
                "RenderWorld GPU cull missing prepared buffers: {}",
                missing.join(", ")
            )));
        }

        let frustum = Frustum::from_view_proj(view_proj);
        let raw = frustum.planes_raw();
        let constants = RenderWorldCullConstants {
            frustum_planes: [
                raw[0].to_array(),
                raw[1].to_array(),
                raw[2].to_array(),
                raw[3].to_array(),
                raw[4].to_array(),
                raw[5].to_array(),
            ],
            object_count,
            _pad0: 0,
            _pad1: 0,
            _pad2: 0,
        };
        frame.dispatch_compute_auto(
            "render_world_cull",
            &self.program,
            &constants,
            [plan.workgroup_count, 1, 1],
        )?;

        Ok(RenderWorldGpuCullDispatchStats {
            dispatched: true,
            object_count,
            workgroup_count: plan.workgroup_count,
            objects_per_workgroup: plan.objects_per_workgroup,
            skipped_reason: None,
        })
    }
}
