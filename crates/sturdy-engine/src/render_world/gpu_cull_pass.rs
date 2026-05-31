use glam::Mat4;

use crate::{
    ComputeProgram, Engine, Error, HizPyramid, RenderFrame, RenderWorld, Result,
    shader_program::builtin_shader_path,
};

use super::{RenderWorldGpuCullDispatchStats, RenderWorldGpuCullSettings};

/// Push constants for the render-world cull compute shader.
///
/// The view_proj matrix replaces the six pre-computed frustum planes: the
/// shader extracts them via Gribb-Hartmann and also uses view_proj for the
/// Hi-Z sphere projection.  Total: 96 bytes.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct RenderWorldCullConstants {
    view_proj: [[f32; 4]; 4], // 64 bytes
    object_count: u32,
    use_hiz: u32,
    screen_width: u32,
    screen_height: u32,
    hiz_mip_count: u32,
    proj_y_scale: f32,
    proj_z_scale: f32,
    _pad: u32,
}

/// Optional Hi-Z occlusion data supplied to the cull pass.
///
/// Pass the **previous** frame's Hi-Z pyramid (from `HizHistoryFrame::previous`)
/// so the cull shader can test objects against last-frame's depth hierarchy.
pub struct HizOcclusionInput<'a> {
    /// Previous-frame Hi-Z pyramid produced by `HizPass::execute_history`.
    pub pyramid: &'a HizPyramid,
    /// `proj[1][1]` from the projection matrix — used to compute screen-space
    /// sphere radius.
    pub proj_y_scale: f32,
    /// `proj[2][2]` from the projection matrix — used to compute near-plane
    /// NDC depth of the sphere front face.
    pub proj_z_scale: f32,
    /// Render-target dimensions in pixels (width, height).
    pub screen_size: [u32; 2],
}

/// Maximum number of Hi-Z pyramid levels the cull shader can consume.
pub const HIZ_MAX_LEVELS: u32 = 8;

/// Scene-wide GPU culling pass for the persistent render world.
///
/// Performs frustum culling and optional Hi-Z occlusion culling in a single
/// compute dispatch over all render-world objects.
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
        hiz: Option<HizOcclusionInput<'_>>,
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

        // Bind Hi-Z pyramid levels and collect projection parameters.
        let (use_hiz, screen_width, screen_height, hiz_mip_count, proj_y_scale, proj_z_scale) =
            match &hiz {
                Some(input) => {
                    let mip_count = input.pyramid.mip_levels().min(HIZ_MAX_LEVELS);
                    for level in 0..mip_count {
                        input
                            .pyramid
                            .mip(level as usize)
                            .register_as(format!("hiz_l{level}"));
                    }
                    (
                        1u32,
                        input.screen_size[0],
                        input.screen_size[1],
                        mip_count,
                        input.proj_y_scale,
                        input.proj_z_scale,
                    )
                }
                None => (0u32, 0u32, 0u32, 0u32, 0.0f32, 0.0f32),
            };

        let vp = view_proj.to_cols_array_2d();
        let constants = RenderWorldCullConstants {
            view_proj: vp,
            object_count,
            use_hiz,
            screen_width,
            screen_height,
            hiz_mip_count,
            proj_y_scale,
            proj_z_scale,
            _pad: 0,
        };
        frame.dispatch_async_compute_auto(
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
