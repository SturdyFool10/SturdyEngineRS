use crate::{GeometryBackend, GeometryRendererCaps, VirtualMesh};

/// Runtime policy for Nanite-like dense geometry rendering.
///
/// The engine can render the same [`VirtualMesh`] through multiple front-ends.
/// This policy keeps the decision explicit and reportable instead of silently
/// pretending virtualized raster is active when mesh shader hardware, cluster
/// hierarchy data, or residency budget is missing.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DenseGeometrySettings {
    /// Prefer cluster-hierarchy rasterization when both the mesh and hardware support it.
    pub prefer_virtualized_raster: bool,
    /// Allow task/mesh shader rendering when full virtualized raster is unavailable.
    pub allow_mesh_shader_fallback: bool,
    /// Allow compute-culling + indexed-indirect rendering when mesh shaders are unavailable.
    pub allow_compute_indirect_fallback: bool,
    /// Allow classic vertex/index rendering as the final compatibility fallback.
    pub allow_classic_fallback: bool,
    /// Projected geometric error target for cluster LOD decisions.
    pub target_error_pixels: f32,
    /// Maximum resident cluster pages this mesh/pass should budget before streaming pressure.
    pub max_resident_cluster_pages: u32,
}

impl Default for DenseGeometrySettings {
    fn default() -> Self {
        Self {
            prefer_virtualized_raster: true,
            allow_mesh_shader_fallback: true,
            allow_compute_indirect_fallback: true,
            allow_classic_fallback: true,
            target_error_pixels: 1.0,
            max_resident_cluster_pages: 4096,
        }
    }
}

/// Chosen geometry path and residency/quality metadata for a virtual mesh.
#[derive(Clone, Debug, PartialEq)]
pub struct DenseGeometryPlan {
    pub backend: GeometryBackend,
    pub target_error_pixels: f32,
    pub max_resident_cluster_pages: u32,
    pub degraded_reason: Option<String>,
}

impl DenseGeometryPlan {
    pub fn plan(
        mesh: &VirtualMesh,
        caps: GeometryRendererCaps,
        settings: DenseGeometrySettings,
    ) -> Self {
        let mut degraded = Vec::new();
        let target_error_pixels = settings.target_error_pixels.max(0.25);
        let max_resident_cluster_pages = settings.max_resident_cluster_pages.max(1);

        let backend = if settings.prefer_virtualized_raster
            && caps.supports(GeometryBackend::VirtualizedRaster)
            && mesh.has_cluster_hierarchy()
        {
            GeometryBackend::VirtualizedRaster
        } else if settings.allow_mesh_shader_fallback
            && caps.supports(GeometryBackend::MeshShader)
            && mesh.has_meshlets()
        {
            if settings.prefer_virtualized_raster {
                if !caps.supports(GeometryBackend::VirtualizedRaster) {
                    degraded
                        .push("virtualized raster requires mesh/task shader support".to_string());
                } else if !mesh.has_cluster_hierarchy() {
                    degraded.push(
                        "mesh has meshlets but no cluster hierarchy for continuous LOD".to_string(),
                    );
                }
            }
            GeometryBackend::MeshShader
        } else if settings.allow_compute_indirect_fallback
            && caps.supports(GeometryBackend::ComputeIndirect)
        {
            if settings.prefer_virtualized_raster {
                if !mesh.has_meshlets() {
                    degraded.push(
                        "meshlet data unavailable, using indexed indirect fallback".to_string(),
                    );
                } else if !caps.supports(GeometryBackend::MeshShader) {
                    degraded.push(
                        "mesh shaders unavailable, using indexed indirect fallback".to_string(),
                    );
                } else {
                    degraded.push(
                        "cluster hierarchy unavailable, using indexed indirect fallback"
                            .to_string(),
                    );
                }
            }
            GeometryBackend::ComputeIndirect
        } else if settings.allow_classic_fallback {
            if settings.prefer_virtualized_raster {
                degraded.push(
                    "GPU-driven geometry fallbacks unavailable, using classic vertex path"
                        .to_string(),
                );
            }
            GeometryBackend::ClassicVertex
        } else {
            degraded.push("no allowed dense-geometry backend is supported; using classic vertex as safety fallback".to_string());
            GeometryBackend::ClassicVertex
        };

        Self {
            backend,
            target_error_pixels,
            max_resident_cluster_pages,
            degraded_reason: if degraded.is_empty() {
                None
            } else {
                Some(degraded.join("; "))
            },
        }
    }

    pub const fn is_virtualized(&self) -> bool {
        matches!(self.backend, GeometryBackend::VirtualizedRaster)
    }

    pub const fn is_gpu_driven(&self) -> bool {
        matches!(
            self.backend,
            GeometryBackend::ComputeIndirect
                | GeometryBackend::MeshShader
                | GeometryBackend::VirtualizedRaster
        )
    }
}

/// Residency budget for dense mesh data packed into renderer mega-buffers.
///
/// This is intentionally CPU-side planning. The renderer can use the resulting
/// byte and page counts to decide whether a movie-quality mesh should be fully
/// resident, streamed by cluster pages, or demoted to a fallback path.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DenseGeometryResidencySettings {
    /// Bytes in one streamable geometry page. 64 KiB keeps upload granularity
    /// friendly for GPU memory allocators and IO batching.
    pub page_size_bytes: u64,
    /// Soft budget for this mesh/pass within the geometry mega-buffer pool.
    pub mesh_budget_bytes: u64,
    /// Fraction of the budget reserved for newly visible cluster pages.
    pub streaming_headroom_fraction: f32,
}

impl Default for DenseGeometryResidencySettings {
    fn default() -> Self {
        Self {
            page_size_bytes: 64 * 1024,
            mesh_budget_bytes: 256 * 1024 * 1024,
            streaming_headroom_fraction: 0.15,
        }
    }
}

/// CPU estimate of how a [`VirtualMesh`] fits into the dense-geometry residency model.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DenseGeometryResidencyPlan {
    pub vertex_bytes: u64,
    pub index_bytes: u64,
    pub meshlet_bytes: u64,
    pub total_bytes: u64,
    pub page_size_bytes: u64,
    pub required_pages: u32,
    pub resident_page_budget: u32,
    pub streaming_page_headroom: u32,
    pub fully_resident: bool,
}

impl DenseGeometryResidencyPlan {
    pub fn plan(mesh: &VirtualMesh, settings: DenseGeometryResidencySettings) -> Self {
        let page_size_bytes = settings.page_size_bytes.max(4096).next_power_of_two();
        let mesh_budget_bytes = settings.mesh_budget_bytes.max(page_size_bytes);
        let headroom_fraction = settings.streaming_headroom_fraction.clamp(0.0, 0.75);

        let vertex_bytes = (mesh.vertices.len() * std::mem::size_of::<crate::Vertex3d>()) as u64;
        let index_bytes = (mesh.indices.len() * std::mem::size_of::<u32>()) as u64;
        let meshlet_bytes = (mesh.meshlets.len() * std::mem::size_of::<crate::Meshlet>()) as u64
            + (mesh.meshlet_vertices.len() * std::mem::size_of::<u32>()) as u64
            + mesh.meshlet_triangles.len() as u64
            + (mesh.meshlet_groups.len() * std::mem::size_of::<crate::MeshletGroup>()) as u64;
        let total_bytes = vertex_bytes
            .saturating_add(index_bytes)
            .saturating_add(meshlet_bytes);
        let required_pages = pages_for(total_bytes, page_size_bytes);
        let total_budget_pages = pages_for(mesh_budget_bytes, page_size_bytes).max(1);
        let streaming_page_headroom = ((total_budget_pages as f32 * headroom_fraction).round()
            as u32)
            .min(total_budget_pages.saturating_sub(1));
        let resident_page_budget = total_budget_pages
            .saturating_sub(streaming_page_headroom)
            .max(1);
        let fully_resident = required_pages <= resident_page_budget;

        Self {
            vertex_bytes,
            index_bytes,
            meshlet_bytes,
            total_bytes,
            page_size_bytes,
            required_pages,
            resident_page_budget,
            streaming_page_headroom,
            fully_resident,
        }
    }

    pub const fn needs_streaming(&self) -> bool {
        !self.fully_resident
    }
}

fn pages_for(bytes: u64, page_size_bytes: u64) -> u32 {
    if bytes == 0 {
        return 0;
    }

    bytes
        .saturating_add(page_size_bytes.saturating_sub(1))
        .saturating_div(page_size_bytes)
        .min(u32::MAX as u64) as u32
}

/// CPU mirror of the projected-error test that should eventually run in a task
/// or compute shader for cluster LOD selection. This avoids discrete artist LOD
/// levels: a cluster remains detailed while its geometric error projects above
/// the configured pixel threshold.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct VirtualGeometryLodParams {
    pub viewport_height_pixels: f32,
    pub vertical_fov_radians: f32,
    pub target_error_pixels: f32,
}

impl VirtualGeometryLodParams {
    pub fn projected_error_pixels(&self, object_space_error: f32, view_distance: f32) -> f32 {
        if object_space_error <= 0.0 || view_distance <= 0.0 || self.viewport_height_pixels <= 0.0 {
            return 0.0;
        }

        let focal_length_pixels = self.viewport_height_pixels
            / (2.0 * (self.vertical_fov_radians.max(0.001) * 0.5).tan());
        object_space_error * focal_length_pixels / view_distance
    }

    pub fn should_refine(
        &self,
        object_space_error: f32,
        view_distance: f32,
        lod_bias: f32,
    ) -> bool {
        let biased_target = (self.target_error_pixels - lod_bias).max(0.25);
        self.projected_error_pixels(object_space_error, view_distance) > biased_target
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Meshlet, MeshletBounds, MeshletGroup, Vertex3d};

    fn triangle_mesh() -> VirtualMesh {
        VirtualMesh::from_vertex_data(
            "tri",
            vec![
                Vertex3d::default(),
                Vertex3d {
                    position: [1.0, 0.0, 0.0],
                    ..Vertex3d::default()
                },
                Vertex3d {
                    position: [0.0, 1.0, 0.0],
                    ..Vertex3d::default()
                },
            ],
            vec![0, 1, 2],
            0,
        )
    }

    fn meshlet() -> Meshlet {
        Meshlet {
            vertex_offset: 0,
            vertex_count: 3,
            triangle_offset: 0,
            triangle_count: 1,
            bounds: MeshletBounds {
                center: [0.0; 3],
                radius: 1.0,
                cone_apex: [0.0; 3],
                lod_error: 0.0,
                cone_axis: [0, 0, 127],
                cone_cutoff: 0,
            },
        }
    }

    #[test]
    fn plan_selects_virtualized_raster_when_mesh_and_caps_support_it() {
        let mut mesh = triangle_mesh();
        mesh.meshlets.push(meshlet());
        mesh.meshlet_groups.push(MeshletGroup {
            meshlet_offset: 0,
            meshlet_count: 1,
            group_center: [0.0; 3],
            group_radius: 1.0,
            self_lod_error: 0.1,
            parent_lod_error: 1.0,
        });

        let plan = DenseGeometryPlan::plan(
            &mesh,
            GeometryRendererCaps {
                mesh_shading: true,
                task_shading: true,
                compute_indirect: true,
                indirect_draw: true,
                ray_tracing: false,
            },
            DenseGeometrySettings::default(),
        );

        assert_eq!(plan.backend, GeometryBackend::VirtualizedRaster);
        assert!(plan.is_virtualized());
        assert_eq!(plan.degraded_reason, None);
    }

    #[test]
    fn plan_reports_mesh_shader_fallback_when_hierarchy_is_missing() {
        let mut mesh = triangle_mesh();
        mesh.meshlets.push(meshlet());

        let plan = DenseGeometryPlan::plan(
            &mesh,
            GeometryRendererCaps {
                mesh_shading: true,
                task_shading: true,
                compute_indirect: true,
                indirect_draw: true,
                ray_tracing: false,
            },
            DenseGeometrySettings::default(),
        );

        assert_eq!(plan.backend, GeometryBackend::MeshShader);
        assert!(plan.is_gpu_driven());
        assert!(
            plan.degraded_reason
                .as_deref()
                .unwrap()
                .contains("cluster hierarchy")
        );
    }

    #[test]
    fn projected_error_gets_smaller_with_distance() {
        let params = VirtualGeometryLodParams {
            viewport_height_pixels: 1080.0,
            vertical_fov_radians: std::f32::consts::FRAC_PI_3,
            target_error_pixels: 1.0,
        };

        let near = params.projected_error_pixels(0.01, 1.0);
        let far = params.projected_error_pixels(0.01, 100.0);

        assert!(near > far);
        assert!(params.should_refine(0.01, 1.0, 0.0));
        assert!(!params.should_refine(0.01, 100.0, 0.0));
    }

    #[test]
    fn residency_plan_counts_pages_and_detects_fully_resident_meshes() {
        let mesh = triangle_mesh();
        let plan = DenseGeometryResidencyPlan::plan(
            &mesh,
            DenseGeometryResidencySettings {
                page_size_bytes: 4096,
                mesh_budget_bytes: 4096 * 4,
                streaming_headroom_fraction: 0.25,
            },
        );

        assert_eq!(plan.page_size_bytes, 4096);
        assert!(plan.vertex_bytes > 0);
        assert_eq!(plan.index_bytes, 3 * std::mem::size_of::<u32>() as u64);
        assert_eq!(plan.required_pages, 1);
        assert_eq!(plan.resident_page_budget, 3);
        assert_eq!(plan.streaming_page_headroom, 1);
        assert!(plan.fully_resident);
        assert!(!plan.needs_streaming());
    }

    #[test]
    fn residency_plan_marks_over_budget_meshes_for_streaming() {
        let mut mesh = triangle_mesh();
        mesh.vertices = vec![Vertex3d::default(); 512];
        mesh.indices = (0..1536).collect();

        let plan = DenseGeometryResidencyPlan::plan(
            &mesh,
            DenseGeometryResidencySettings {
                page_size_bytes: 4096,
                mesh_budget_bytes: 4096,
                streaming_headroom_fraction: 0.0,
            },
        );

        assert!(plan.required_pages > plan.resident_page_budget);
        assert!(!plan.fully_resident);
        assert!(plan.needs_streaming());
    }
}
