/// Backend-neutral renderer workload counters for benchmark reports.
///
/// These counters line up with the roadmap success metrics: visible/submitted
/// triangles, draw/dispatch counts, upload bandwidth, memory pressure, and
/// degraded/fallback state. They are intentionally plain data so runtime,
/// testbed, and future benchmark harness code can serialize them easily.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RendererWorkloadMetrics {
    pub renderable_objects: u32,
    pub visible_objects: u32,
    pub material_count: u32,
    pub resource_table_entries: u32,
    pub persistent_bin_count: u32,
    pub visible_triangles: u64,
    pub submitted_triangles: u64,
    pub draw_count: u32,
    pub dispatch_count: u32,
    pub upload_bytes: u64,
    pub gpu_resident_bytes: u64,
    pub transient_bytes: u64,
    pub gi_probe_rays: u32,
    pub gi_rt_rays: u32,
    pub degraded_event_count: u32,
}

impl RendererWorkloadMetrics {
    pub fn add(&mut self, other: &Self) {
        self.renderable_objects = self
            .renderable_objects
            .saturating_add(other.renderable_objects);
        self.visible_objects = self.visible_objects.saturating_add(other.visible_objects);
        self.material_count = self.material_count.saturating_add(other.material_count);
        self.resource_table_entries = self
            .resource_table_entries
            .saturating_add(other.resource_table_entries);
        self.persistent_bin_count = self
            .persistent_bin_count
            .saturating_add(other.persistent_bin_count);
        self.visible_triangles = self
            .visible_triangles
            .saturating_add(other.visible_triangles);
        self.submitted_triangles = self
            .submitted_triangles
            .saturating_add(other.submitted_triangles);
        self.draw_count = self.draw_count.saturating_add(other.draw_count);
        self.dispatch_count = self.dispatch_count.saturating_add(other.dispatch_count);
        self.upload_bytes = self.upload_bytes.saturating_add(other.upload_bytes);
        self.gpu_resident_bytes = self
            .gpu_resident_bytes
            .saturating_add(other.gpu_resident_bytes);
        self.transient_bytes = self.transient_bytes.saturating_add(other.transient_bytes);
        self.gi_probe_rays = self.gi_probe_rays.saturating_add(other.gi_probe_rays);
        self.gi_rt_rays = self.gi_rt_rays.saturating_add(other.gi_rt_rays);
        self.degraded_event_count = self
            .degraded_event_count
            .saturating_add(other.degraded_event_count);
    }

    pub fn triangle_submission_ratio(&self) -> f32 {
        if self.visible_triangles == 0 {
            return 0.0;
        }
        self.submitted_triangles as f32 / self.visible_triangles as f32
    }

    pub fn upload_mib(&self) -> f32 {
        self.upload_bytes as f32 / (1024.0 * 1024.0)
    }

    pub fn resident_mib(&self) -> f32 {
        self.gpu_resident_bytes as f32 / (1024.0 * 1024.0)
    }
}

/// Targets used by tests/benchmark harnesses to decide whether a renderer path
/// is still on the intended GPU-driven roadmap trajectory.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RendererWorkloadTargets {
    pub max_cpu_submission_units: u32,
    pub max_triangle_submission_ratio: f32,
    pub max_upload_bytes_per_frame: u64,
    pub max_degraded_events: u32,
}

impl Default for RendererWorkloadTargets {
    fn default() -> Self {
        Self {
            max_cpu_submission_units: 1,
            max_triangle_submission_ratio: 1.25,
            max_upload_bytes_per_frame: 64 * 1024 * 1024,
            max_degraded_events: 0,
        }
    }
}

/// Evaluation of a frame/workload against benchmark targets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RendererWorkloadEvaluation {
    pub passes: bool,
    pub failed_reasons: Vec<String>,
}

impl RendererWorkloadEvaluation {
    pub fn evaluate(
        metrics: &RendererWorkloadMetrics,
        cpu_submission_units: u32,
        targets: RendererWorkloadTargets,
    ) -> Self {
        let mut failed_reasons = Vec::new();

        if cpu_submission_units > targets.max_cpu_submission_units {
            failed_reasons.push(format!(
                "CPU submission units {cpu_submission_units} exceed target {}",
                targets.max_cpu_submission_units
            ));
        }

        let triangle_ratio = metrics.triangle_submission_ratio();
        if metrics.visible_triangles > 0 && triangle_ratio > targets.max_triangle_submission_ratio {
            failed_reasons.push(format!(
                "triangle submission ratio {triangle_ratio:.2} exceeds target {:.2}",
                targets.max_triangle_submission_ratio
            ));
        }

        if metrics.upload_bytes > targets.max_upload_bytes_per_frame {
            failed_reasons.push(format!(
                "upload bytes {} exceed target {}",
                metrics.upload_bytes, targets.max_upload_bytes_per_frame
            ));
        }

        if metrics.degraded_event_count > targets.max_degraded_events {
            failed_reasons.push(format!(
                "degraded events {} exceed target {}",
                metrics.degraded_event_count, targets.max_degraded_events
            ));
        }

        Self {
            passes: failed_reasons.is_empty(),
            failed_reasons,
        }
    }
}

/// Builder for accumulating frame metrics from independent renderer systems.
#[derive(Clone, Debug, Default)]
pub struct RendererWorkloadMetricsBuilder {
    metrics: RendererWorkloadMetrics,
}

impl RendererWorkloadMetricsBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_render_world_bins(
        mut self,
        bins: &crate::RenderWorldPersistentBinPlan,
        visible_objects: u32,
    ) -> Self {
        self.metrics.renderable_objects = self.metrics.renderable_objects.max(bins.object_count);
        self.metrics.visible_objects = self.metrics.visible_objects.saturating_add(visible_objects);
        self.metrics.persistent_bin_count = self
            .metrics
            .persistent_bin_count
            .saturating_add(bins.bin_count);
        self.metrics.draw_count = self
            .metrics
            .draw_count
            .saturating_add(bins.indirect_command_count);
        self
    }

    pub fn add_gpu_cull(mut self, cull: &crate::RenderWorldGpuCullPlan) -> Self {
        self.metrics.dispatch_count = self
            .metrics
            .dispatch_count
            .saturating_add(cull.dispatch_count);
        if cull.degraded_reason.is_some() {
            self.metrics.degraded_event_count = self.metrics.degraded_event_count.saturating_add(1);
        }
        self
    }

    pub fn add_gpu_matrix(mut self, matrices: &crate::RenderWorldGpuMatrixPlan) -> Self {
        self.metrics.renderable_objects =
            self.metrics.renderable_objects.max(matrices.object_count);
        self.metrics.dispatch_count = self
            .metrics
            .dispatch_count
            .saturating_add((matrices.workgroup_count > 0) as u32);
        self.metrics.upload_bytes = self
            .metrics
            .upload_bytes
            .saturating_add(matrices.source_bytes);
        self.metrics.gpu_resident_bytes = self
            .metrics
            .gpu_resident_bytes
            .saturating_add(matrices.source_bytes)
            .saturating_add(matrices.total_derived_bytes);
        if matrices.degraded_reason.is_some() {
            self.metrics.degraded_event_count = self.metrics.degraded_event_count.saturating_add(1);
        }
        self
    }

    pub fn add_material_table(mut self, material_table: &crate::MaterialTablePlan) -> Self {
        self.metrics.material_count = self.metrics.material_count.max(material_table.capacity);
        self.metrics.gpu_resident_bytes = self
            .metrics
            .gpu_resident_bytes
            .saturating_add(material_table.total_table_bytes);
        if material_table.degraded_reason.is_some() {
            self.metrics.degraded_event_count = self.metrics.degraded_event_count.saturating_add(1);
        }
        self
    }

    pub fn add_resource_table(mut self, table: &crate::SceneResourceTablePlan) -> Self {
        self.metrics.resource_table_entries = self
            .metrics
            .resource_table_entries
            .saturating_add(table.capacity);
        self.metrics.gpu_resident_bytes = self
            .metrics
            .gpu_resident_bytes
            .saturating_add(table.total_table_bytes);
        if table.degraded_reason.is_some() {
            self.metrics.degraded_event_count = self.metrics.degraded_event_count.saturating_add(1);
        }
        self
    }

    pub fn add_gi(mut self, gi: &crate::RealtimeGiPlan) -> Self {
        self.metrics.gi_probe_rays = self
            .metrics
            .gi_probe_rays
            .saturating_add(gi.probe_rays_per_frame);
        self.metrics.gi_rt_rays = self.metrics.gi_rt_rays.saturating_add(gi.rt_rays_per_frame);
        if gi.degraded_reason.is_some() {
            self.metrics.degraded_event_count = self.metrics.degraded_event_count.saturating_add(1);
        }
        self
    }

    pub fn add_triangles(mut self, visible: u64, submitted: u64) -> Self {
        self.metrics.visible_triangles = self.metrics.visible_triangles.saturating_add(visible);
        self.metrics.submitted_triangles =
            self.metrics.submitted_triangles.saturating_add(submitted);
        self
    }

    pub fn add_upload_bytes(mut self, bytes: u64) -> Self {
        self.metrics.upload_bytes = self.metrics.upload_bytes.saturating_add(bytes);
        self
    }

    pub fn build(self) -> RendererWorkloadMetrics {
        self.metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_accumulate_and_compute_ratios() {
        let mut metrics = RendererWorkloadMetrics {
            visible_triangles: 100,
            submitted_triangles: 125,
            upload_bytes: 2 * 1024 * 1024,
            gpu_resident_bytes: 8 * 1024 * 1024,
            ..RendererWorkloadMetrics::default()
        };
        metrics.add(&RendererWorkloadMetrics {
            visible_triangles: 100,
            submitted_triangles: 100,
            upload_bytes: 1024 * 1024,
            ..RendererWorkloadMetrics::default()
        });

        assert_eq!(metrics.visible_triangles, 200);
        assert_eq!(metrics.submitted_triangles, 225);
        assert_eq!(metrics.triangle_submission_ratio(), 1.125);
        assert_eq!(metrics.upload_mib(), 3.0);
        assert_eq!(metrics.resident_mib(), 8.0);
    }

    #[test]
    fn evaluation_reports_failed_targets() {
        let metrics = RendererWorkloadMetrics {
            visible_triangles: 100,
            submitted_triangles: 300,
            upload_bytes: 128,
            degraded_event_count: 2,
            ..RendererWorkloadMetrics::default()
        };
        let evaluation = RendererWorkloadEvaluation::evaluate(
            &metrics,
            4,
            RendererWorkloadTargets {
                max_cpu_submission_units: 1,
                max_triangle_submission_ratio: 2.0,
                max_upload_bytes_per_frame: 64,
                max_degraded_events: 0,
            },
        );

        assert!(!evaluation.passes);
        assert_eq!(evaluation.failed_reasons.len(), 4);
    }

    #[test]
    fn builder_combines_planning_outputs() {
        let bins = crate::RenderWorldPersistentBinPlan {
            bin_count: 2,
            object_count: 10,
            indirect_command_count: 2,
            cpu_submission_units: 1,
            material_instance_count_affects_bins: false,
        };
        let cull = crate::RenderWorldGpuCullPlan::plan(
            10,
            2,
            crate::RenderWorldGpuCullCaps {
                compute: true,
                indirect_count: true,
                multi_draw_indirect: true,
                hiz_occlusion: true,
            },
            crate::RenderWorldGpuCullSettings::default(),
        );
        let gi = crate::RealtimeGiPlan::plan(
            crate::RealtimeGiCaps {
                compute: true,
                ray_tracing: false,
                bindless: true,
                temporal_history: true,
            },
            crate::RealtimeGiSettings::default(),
        );

        let metrics = RendererWorkloadMetricsBuilder::new()
            .add_render_world_bins(&bins, 8)
            .add_gpu_cull(&cull)
            .add_gi(&gi)
            .add_triangles(1_000, 1_100)
            .build();

        assert_eq!(metrics.renderable_objects, 10);
        assert_eq!(metrics.visible_objects, 8);
        assert_eq!(metrics.persistent_bin_count, 2);
        assert_eq!(metrics.draw_count, 2);
        assert_eq!(metrics.dispatch_count, 1);
        assert_eq!(metrics.gi_probe_rays, 1_000_000);
        assert_eq!(metrics.triangle_submission_ratio(), 1.1);
    }
}
