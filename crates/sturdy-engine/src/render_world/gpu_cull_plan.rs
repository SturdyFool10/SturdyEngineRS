/// Renderer-facing policy for GPU render-world culling and draw generation.
///
/// The roadmap target is one GPU cull dispatch per view/pass over the persistent
/// render world, with Hi-Z occlusion and GPU-written visible counts feeding
/// indirect-count draws. This module captures that choice and its explicit
/// fallbacks without requiring the Vulkan/D3D12/Metal command paths to exist yet.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RenderWorldGpuCullCaps {
    /// Compute shaders can read the render-world object table and write cull output.
    pub compute: bool,
    /// Backend can consume GPU-written draw counts (`draw_indirect_count` or equivalent).
    pub indirect_count: bool,
    /// Backend can submit more than one indirect command in a single API call.
    pub multi_draw_indirect: bool,
    /// Hi-Z depth pyramid is available for occlusion tests.
    pub hiz_occlusion: bool,
}

impl Default for RenderWorldGpuCullCaps {
    fn default() -> Self {
        Self {
            compute: true,
            indirect_count: false,
            multi_draw_indirect: false,
            hiz_occlusion: false,
        }
    }
}

impl RenderWorldGpuCullCaps {
    pub fn from_caps(caps: &sturdy_engine_core::Caps, hiz_occlusion: bool) -> Self {
        Self {
            compute: true,
            indirect_count: caps.features.draw_indirect_count,
            multi_draw_indirect: caps.features.multi_draw_indirect,
            hiz_occlusion,
        }
    }
}

/// Runtime knobs for render-world GPU culling.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RenderWorldGpuCullSettings {
    /// Prefer a single global cull dispatch over per-batch culling.
    pub prefer_single_dispatch: bool,
    /// Use Hi-Z occlusion when a depth pyramid is available.
    pub enable_hiz_occlusion: bool,
    /// Use previous-visible first, then refresh newly-visible after a depth update.
    pub enable_two_phase_occlusion: bool,
    /// Workgroup size used by the object culling compute shader.
    pub objects_per_workgroup: u32,
}

impl Default for RenderWorldGpuCullSettings {
    fn default() -> Self {
        Self {
            prefer_single_dispatch: true,
            enable_hiz_occlusion: true,
            enable_two_phase_occlusion: false,
            objects_per_workgroup: 64,
        }
    }
}

/// Concrete culling/draw-generation strategy for one view/pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderWorldGpuCullPlan {
    pub uses_gpu_culling: bool,
    pub single_dispatch: bool,
    pub uses_hiz_occlusion: bool,
    pub two_phase_occlusion: bool,
    pub uses_indirect_count: bool,
    pub uses_multi_draw_indirect: bool,
    pub dispatch_count: u32,
    pub workgroup_count: u32,
    pub objects_per_workgroup: u32,
    pub degraded_reason: Option<String>,
}

impl RenderWorldGpuCullPlan {
    pub fn plan(
        object_count: usize,
        batch_count: usize,
        caps: RenderWorldGpuCullCaps,
        settings: RenderWorldGpuCullSettings,
    ) -> Self {
        let mut degraded = Vec::new();
        let object_count = object_count.min(u32::MAX as usize) as u32;
        let batch_count = batch_count.min(u32::MAX as usize) as u32;
        let objects_per_workgroup = settings.objects_per_workgroup.clamp(32, 256);

        if object_count == 0 {
            return Self {
                uses_gpu_culling: caps.compute,
                single_dispatch: caps.compute && settings.prefer_single_dispatch,
                uses_hiz_occlusion: false,
                two_phase_occlusion: false,
                uses_indirect_count: false,
                uses_multi_draw_indirect: false,
                dispatch_count: 0,
                workgroup_count: 0,
                objects_per_workgroup,
                degraded_reason: None,
            };
        }

        if !caps.compute {
            degraded.push(
                "compute unavailable, render-world culling must fall back to CPU batches"
                    .to_string(),
            );
            return Self {
                uses_gpu_culling: false,
                single_dispatch: false,
                uses_hiz_occlusion: false,
                two_phase_occlusion: false,
                uses_indirect_count: false,
                uses_multi_draw_indirect: false,
                dispatch_count: batch_count.max(1),
                workgroup_count: 0,
                objects_per_workgroup,
                degraded_reason: Some(degraded.join("; ")),
            };
        }

        let single_dispatch = settings.prefer_single_dispatch;
        if !single_dispatch {
            degraded.push("single-dispatch render-world culling disabled by settings".to_string());
        }
        let dispatch_count = if single_dispatch {
            1
        } else {
            batch_count.max(1)
        };
        let workgroup_count = object_count.div_ceil(objects_per_workgroup).max(1);

        let uses_hiz_occlusion = settings.enable_hiz_occlusion && caps.hiz_occlusion;
        if settings.enable_hiz_occlusion && !caps.hiz_occlusion {
            degraded.push("Hi-Z occlusion requested but no depth pyramid is available".to_string());
        }

        let two_phase_occlusion = settings.enable_two_phase_occlusion && uses_hiz_occlusion;
        if settings.enable_two_phase_occlusion && !uses_hiz_occlusion {
            degraded.push("two-phase occlusion requires Hi-Z occlusion".to_string());
        }

        let uses_indirect_count = caps.indirect_count;
        if !uses_indirect_count {
            degraded.push(
                "indirect-count unavailable, using fixed indirect command ranges".to_string(),
            );
        }

        let uses_multi_draw_indirect = caps.multi_draw_indirect || uses_indirect_count;
        if !uses_multi_draw_indirect {
            degraded.push(
                "multi-draw indirect unavailable, backend may submit per-batch indirect draws"
                    .to_string(),
            );
        }

        Self {
            uses_gpu_culling: true,
            single_dispatch,
            uses_hiz_occlusion,
            two_phase_occlusion,
            uses_indirect_count,
            uses_multi_draw_indirect,
            dispatch_count,
            workgroup_count,
            objects_per_workgroup,
            degraded_reason: if degraded.is_empty() {
                None
            } else {
                Some(degraded.join("; "))
            },
        }
    }

    pub const fn is_fast_path(&self) -> bool {
        self.uses_gpu_culling
            && self.single_dispatch
            && self.uses_hiz_occlusion
            && self.uses_indirect_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_path_uses_one_dispatch_hiz_and_indirect_count() {
        let plan = RenderWorldGpuCullPlan::plan(
            100_000,
            128,
            RenderWorldGpuCullCaps {
                compute: true,
                indirect_count: true,
                multi_draw_indirect: true,
                hiz_occlusion: true,
            },
            RenderWorldGpuCullSettings::default(),
        );

        assert!(plan.uses_gpu_culling);
        assert!(plan.single_dispatch);
        assert_eq!(plan.dispatch_count, 1);
        assert_eq!(plan.workgroup_count, 1563);
        assert!(plan.uses_hiz_occlusion);
        assert!(plan.uses_indirect_count);
        assert!(plan.is_fast_path());
        assert_eq!(plan.degraded_reason, None);
    }

    #[test]
    fn no_compute_reports_cpu_batch_fallback() {
        let plan = RenderWorldGpuCullPlan::plan(
            10_000,
            42,
            RenderWorldGpuCullCaps {
                compute: false,
                indirect_count: true,
                multi_draw_indirect: true,
                hiz_occlusion: true,
            },
            RenderWorldGpuCullSettings::default(),
        );

        assert!(!plan.uses_gpu_culling);
        assert_eq!(plan.dispatch_count, 42);
        assert_eq!(plan.workgroup_count, 0);
        assert!(
            plan.degraded_reason
                .as_deref()
                .unwrap()
                .contains("CPU batches")
        );
    }

    #[test]
    fn missing_hiz_and_indirect_count_are_reported_as_degraded() {
        let plan = RenderWorldGpuCullPlan::plan(
            512,
            8,
            RenderWorldGpuCullCaps {
                compute: true,
                indirect_count: false,
                multi_draw_indirect: false,
                hiz_occlusion: false,
            },
            RenderWorldGpuCullSettings {
                enable_two_phase_occlusion: true,
                ..RenderWorldGpuCullSettings::default()
            },
        );

        assert!(plan.uses_gpu_culling);
        assert!(!plan.uses_hiz_occlusion);
        assert!(!plan.two_phase_occlusion);
        assert!(!plan.uses_indirect_count);
        assert!(!plan.is_fast_path());
        let reason = plan.degraded_reason.as_deref().unwrap();
        assert!(reason.contains("Hi-Z"));
        assert!(reason.contains("two-phase"));
        assert!(reason.contains("indirect-count"));
    }
}
