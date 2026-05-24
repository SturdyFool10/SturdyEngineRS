/// Capabilities needed to expand compact transform sources on the GPU.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RenderWorldGpuMatrixCaps {
    /// Compute shaders can read compact transform sources and write derived buffers.
    pub compute: bool,
    /// Storage buffers are available for source/derived render-world data.
    pub storage_buffers: bool,
    /// Partial buffer updates are available for changed transform source ranges.
    pub partial_buffer_updates: bool,
}

impl Default for RenderWorldGpuMatrixCaps {
    fn default() -> Self {
        Self {
            compute: true,
            storage_buffers: true,
            partial_buffer_updates: true,
        }
    }
}

impl RenderWorldGpuMatrixCaps {
    pub fn from_caps(_caps: &sturdy_engine_core::Caps) -> Self {
        Self::default()
    }
}

/// Runtime policy for GPU matrix and bounds generation.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RenderWorldGpuMatrixSettings {
    /// Prefer GPU expansion over CPU-materialized matrices.
    pub prefer_gpu_generation: bool,
    /// Preserve previous world matrices for motion vectors and temporal passes.
    pub preserve_previous_matrices: bool,
    /// Generate normal matrices for lighting instead of deriving in vertex/fragment shaders.
    pub generate_normal_matrices: bool,
    /// Generate world-space bounds for culling from local bounds + generated matrices.
    pub generate_world_bounds: bool,
    /// Workgroup size for the transform expansion compute shader.
    pub objects_per_workgroup: u32,
}

impl Default for RenderWorldGpuMatrixSettings {
    fn default() -> Self {
        Self {
            prefer_gpu_generation: true,
            preserve_previous_matrices: true,
            generate_normal_matrices: true,
            generate_world_bounds: true,
            objects_per_workgroup: 64,
        }
    }
}

/// Derived buffer sizing and selected path for render-world matrix generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderWorldGpuMatrixPlan {
    pub uses_gpu_generation: bool,
    pub object_count: u32,
    pub workgroup_count: u32,
    pub objects_per_workgroup: u32,
    pub source_bytes: u64,
    pub current_matrix_bytes: u64,
    pub previous_matrix_bytes: u64,
    pub normal_matrix_bytes: u64,
    pub bounds_bytes: u64,
    pub total_derived_bytes: u64,
    pub supports_dirty_source_uploads: bool,
    pub degraded_reason: Option<String>,
}

impl RenderWorldGpuMatrixPlan {
    pub fn plan(
        object_count: usize,
        caps: RenderWorldGpuMatrixCaps,
        settings: RenderWorldGpuMatrixSettings,
    ) -> Self {
        let mut degraded = Vec::new();
        let object_count = object_count.min(u32::MAX as usize) as u32;
        let objects_per_workgroup = settings.objects_per_workgroup.clamp(32, 256);
        let workgroup_count = if object_count == 0 {
            0
        } else {
            object_count.div_ceil(objects_per_workgroup).max(1)
        };

        let mut uses_gpu_generation =
            settings.prefer_gpu_generation && caps.compute && caps.storage_buffers;
        if settings.prefer_gpu_generation && !caps.compute {
            degraded.push("compute unavailable, matrices must be materialized on CPU".to_string());
        }
        if settings.prefer_gpu_generation && !caps.storage_buffers {
            degraded.push(
                "storage buffers unavailable, GPU matrix generation cannot write derived tables"
                    .to_string(),
            );
        }
        if !settings.prefer_gpu_generation {
            uses_gpu_generation = false;
            degraded.push("GPU matrix generation disabled by settings".to_string());
        }
        if !caps.partial_buffer_updates {
            degraded.push(
                "partial buffer updates unavailable, transform source changes require full uploads"
                    .to_string(),
            );
        }

        let source_stride = std::mem::size_of::<super::GpuTransformSourceData>() as u64;
        let mat4_stride = 64_u64;
        let normal_stride = 48_u64;
        let bounds_stride = 16_u64;
        let object_count_u64 = object_count as u64;

        let source_bytes = object_count_u64 * source_stride;
        let current_matrix_bytes = if uses_gpu_generation {
            object_count_u64 * mat4_stride
        } else {
            0
        };
        let previous_matrix_bytes = if uses_gpu_generation && settings.preserve_previous_matrices {
            object_count_u64 * mat4_stride
        } else {
            0
        };
        let normal_matrix_bytes = if uses_gpu_generation && settings.generate_normal_matrices {
            object_count_u64 * normal_stride
        } else {
            0
        };
        let bounds_bytes = if uses_gpu_generation && settings.generate_world_bounds {
            object_count_u64 * bounds_stride
        } else {
            0
        };
        let total_derived_bytes = current_matrix_bytes
            .saturating_add(previous_matrix_bytes)
            .saturating_add(normal_matrix_bytes)
            .saturating_add(bounds_bytes);

        Self {
            uses_gpu_generation,
            object_count,
            workgroup_count,
            objects_per_workgroup,
            source_bytes,
            current_matrix_bytes,
            previous_matrix_bytes,
            normal_matrix_bytes,
            bounds_bytes,
            total_derived_bytes,
            supports_dirty_source_uploads: caps.partial_buffer_updates && caps.storage_buffers,
            degraded_reason: if degraded.is_empty() {
                None
            } else {
                Some(degraded.join("; "))
            },
        }
    }

    pub const fn is_temporal_ready(&self) -> bool {
        self.uses_gpu_generation && self.previous_matrix_bytes > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_path_generates_all_derived_buffers() {
        let plan = RenderWorldGpuMatrixPlan::plan(
            100_000,
            RenderWorldGpuMatrixCaps::default(),
            RenderWorldGpuMatrixSettings::default(),
        );

        assert!(plan.uses_gpu_generation);
        assert_eq!(plan.workgroup_count, 1563);
        assert!(plan.source_bytes > 0);
        assert_eq!(plan.current_matrix_bytes, 100_000 * 64);
        assert_eq!(plan.previous_matrix_bytes, 100_000 * 64);
        assert_eq!(plan.normal_matrix_bytes, 100_000 * 48);
        assert_eq!(plan.bounds_bytes, 100_000 * 16);
        assert!(plan.is_temporal_ready());
        assert_eq!(plan.degraded_reason, None);
    }

    #[test]
    fn no_compute_reports_cpu_matrix_fallback() {
        let plan = RenderWorldGpuMatrixPlan::plan(
            12,
            RenderWorldGpuMatrixCaps {
                compute: false,
                storage_buffers: true,
                partial_buffer_updates: true,
            },
            RenderWorldGpuMatrixSettings::default(),
        );

        assert!(!plan.uses_gpu_generation);
        assert_eq!(plan.current_matrix_bytes, 0);
        assert_eq!(plan.previous_matrix_bytes, 0);
        assert!(plan.degraded_reason.as_deref().unwrap().contains("CPU"));
    }

    #[test]
    fn settings_can_disable_optional_outputs() {
        let plan = RenderWorldGpuMatrixPlan::plan(
            64,
            RenderWorldGpuMatrixCaps::default(),
            RenderWorldGpuMatrixSettings {
                preserve_previous_matrices: false,
                generate_normal_matrices: false,
                generate_world_bounds: false,
                ..RenderWorldGpuMatrixSettings::default()
            },
        );

        assert!(plan.uses_gpu_generation);
        assert_eq!(plan.current_matrix_bytes, 64 * 64);
        assert_eq!(plan.previous_matrix_bytes, 0);
        assert_eq!(plan.normal_matrix_bytes, 0);
        assert_eq!(plan.bounds_bytes, 0);
        assert!(!plan.is_temporal_ready());
    }
}
