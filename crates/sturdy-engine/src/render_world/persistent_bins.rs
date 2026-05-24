use crate::{GeometryBackend, MeshId};

use super::{MaterialId, RenderObjectState, VisibilityFlags};

/// Coarse material shader class used for persistent render bins.
///
/// Material instances should live in a GPU material table; bins should only vary
/// by shader class/state that changes pipeline or shader execution behavior.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum MaterialShaderClass {
    PbrOpaque,
    PbrMasked,
    PbrTranslucent,
    Unlit,
    Decal,
}

/// Vertex/mesh input layout class used for persistent render bins.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum VertexLayoutClass {
    StaticMesh,
    SkinnedMesh,
    Meshlet,
    Procedural,
}

/// Render-state class that should correspond to stable pipeline state.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum RenderStateClass {
    OpaqueDepthWrite,
    MaskedDepthWrite,
    TranslucentSorted,
    ShadowCaster,
    Decal,
}

/// Stable pipeline class used by render bins before backend-specific pipeline IDs exist.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum PipelineClass {
    GBuffer,
    ForwardLit,
    ShadowDepth,
    VisibilityBuffer,
    Decal,
}

/// Persistent render bin key.
///
/// This intentionally excludes material instance ID. Material parameters should
/// be fetched from GPU tables by object/material ID so many material instances
/// can share a bin when their shader class and render state match.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct RenderWorldBinKey {
    pub pipeline: PipelineClass,
    pub geometry_backend: GeometryBackend,
    pub material_shader: MaterialShaderClass,
    pub vertex_layout: VertexLayoutClass,
    pub render_state: RenderStateClass,
    pub mesh: MeshId,
}

impl RenderWorldBinKey {
    pub const fn new(
        pipeline: PipelineClass,
        geometry_backend: GeometryBackend,
        material_shader: MaterialShaderClass,
        vertex_layout: VertexLayoutClass,
        render_state: RenderStateClass,
        mesh: MeshId,
    ) -> Self {
        Self {
            pipeline,
            geometry_backend,
            material_shader,
            vertex_layout,
            render_state,
            mesh,
        }
    }
}

/// CPU-side persistent bin contents for planning/diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderWorldPersistentBin {
    pub key: RenderWorldBinKey,
    pub object_count: u32,
    pub first_object_slot: u32,
}

/// Collection of persistent bins sorted by stable key.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RenderWorldPersistentBins {
    pub bins: Vec<RenderWorldPersistentBin>,
    pub object_count: u32,
}

impl RenderWorldPersistentBins {
    pub fn from_states(
        states: &[RenderObjectState],
        geometry_backend: GeometryBackend,
        pipeline: PipelineClass,
        material_shader: MaterialShaderClass,
        vertex_layout: VertexLayoutClass,
        render_state: RenderStateClass,
    ) -> Self {
        let mut grouped: Vec<(RenderWorldBinKey, Vec<u32>)> = Vec::new();

        for state in states {
            if !state.visibility.flags.contains(VisibilityFlags::VISIBLE) {
                continue;
            }
            let Some(mesh) = state.mesh else {
                continue;
            };
            let material = state
                .material
                .map(|material| material.material)
                .unwrap_or_else(|| MaterialId::from_raw(mesh.mesh.as_u32()));
            let _material_table_slot = material;

            let key = RenderWorldBinKey::new(
                pipeline,
                geometry_backend,
                material_shader,
                vertex_layout,
                render_state,
                mesh.mesh,
            );
            if let Some((_, objects)) = grouped.iter_mut().find(|(candidate, _)| *candidate == key)
            {
                objects.push(state.object.as_u32());
            } else {
                grouped.push((key, vec![state.object.as_u32()]));
            }
        }

        grouped.sort_by_key(|(key, _)| bin_sort_key(*key));

        let mut bins = Vec::with_capacity(grouped.len());
        let mut object_count = 0_u32;
        for (key, mut objects) in grouped {
            objects.sort_unstable();
            let first_object_slot = object_count;
            object_count = object_count.saturating_add(objects.len().min(u32::MAX as usize) as u32);
            bins.push(RenderWorldPersistentBin {
                key,
                object_count: objects.len().min(u32::MAX as usize) as u32,
                first_object_slot,
            });
        }

        Self { bins, object_count }
    }

    pub fn len(&self) -> usize {
        self.bins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bins.is_empty()
    }
}

fn bin_sort_key(key: RenderWorldBinKey) -> (u8, u8, u8, u8, u8, u32) {
    (
        pipeline_rank(key.pipeline),
        geometry_rank(key.geometry_backend),
        material_shader_rank(key.material_shader),
        vertex_layout_rank(key.vertex_layout),
        render_state_rank(key.render_state),
        key.mesh.as_u32(),
    )
}

const fn pipeline_rank(value: PipelineClass) -> u8 {
    match value {
        PipelineClass::GBuffer => 0,
        PipelineClass::ForwardLit => 1,
        PipelineClass::ShadowDepth => 2,
        PipelineClass::VisibilityBuffer => 3,
        PipelineClass::Decal => 4,
    }
}

const fn geometry_rank(value: GeometryBackend) -> u8 {
    match value {
        GeometryBackend::ClassicVertex => 0,
        GeometryBackend::ComputeIndirect => 1,
        GeometryBackend::MeshShader => 2,
        GeometryBackend::VirtualizedRaster => 3,
        GeometryBackend::RayTracingFallback => 4,
        GeometryBackend::RayTracingSelectedClusters => 5,
    }
}

const fn material_shader_rank(value: MaterialShaderClass) -> u8 {
    match value {
        MaterialShaderClass::PbrOpaque => 0,
        MaterialShaderClass::PbrMasked => 1,
        MaterialShaderClass::PbrTranslucent => 2,
        MaterialShaderClass::Unlit => 3,
        MaterialShaderClass::Decal => 4,
    }
}

const fn vertex_layout_rank(value: VertexLayoutClass) -> u8 {
    match value {
        VertexLayoutClass::StaticMesh => 0,
        VertexLayoutClass::SkinnedMesh => 1,
        VertexLayoutClass::Meshlet => 2,
        VertexLayoutClass::Procedural => 3,
    }
}

const fn render_state_rank(value: RenderStateClass) -> u8 {
    match value {
        RenderStateClass::OpaqueDepthWrite => 0,
        RenderStateClass::MaskedDepthWrite => 1,
        RenderStateClass::TranslucentSorted => 2,
        RenderStateClass::ShadowCaster => 3,
        RenderStateClass::Decal => 4,
    }
}

/// Planning result for persistent render bins.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderWorldPersistentBinPlan {
    pub bin_count: u32,
    pub object_count: u32,
    pub indirect_command_count: u32,
    pub cpu_submission_units: u32,
    pub material_instance_count_affects_bins: bool,
}

impl RenderWorldPersistentBinPlan {
    pub fn plan(bins: &RenderWorldPersistentBins, use_indirect_count: bool) -> Self {
        let bin_count = bins.bins.len().min(u32::MAX as usize) as u32;
        Self {
            bin_count,
            object_count: bins.object_count,
            indirect_command_count: bin_count,
            cpu_submission_units: if use_indirect_count { 1 } else { bin_count },
            material_instance_count_affects_bins: false,
        }
    }

    pub const fn is_gpu_submission_friendly(&self) -> bool {
        self.cpu_submission_units <= 1 && !self.material_instance_count_affects_bins
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use crate::{RenderMaterial, RenderMesh, RenderVisibility, ecs::Transform};

    use super::*;
    use crate::GpuObjectId;

    fn object(raw: u32, mesh: u32, material: u32) -> RenderObjectState {
        let mut state = RenderObjectState::new(GpuObjectId::from_raw(raw));
        state.mesh = Some(RenderMesh::new(MeshId::from_raw(mesh)));
        state.material = Some(RenderMaterial::new(MaterialId::from_raw(material)));
        state.transform = Some(Transform::from_position(Vec3::new(raw as f32, 0.0, 0.0)));
        state
    }

    #[test]
    fn bins_group_by_mesh_and_pipeline_classes_not_material_instance() {
        let bins = RenderWorldPersistentBins::from_states(
            &[object(2, 10, 100), object(1, 10, 101), object(3, 11, 100)],
            GeometryBackend::ComputeIndirect,
            PipelineClass::GBuffer,
            MaterialShaderClass::PbrOpaque,
            VertexLayoutClass::StaticMesh,
            RenderStateClass::OpaqueDepthWrite,
        );

        assert_eq!(bins.len(), 2);
        assert_eq!(bins.object_count, 3);
        assert_eq!(bins.bins[0].object_count, 2);
        assert_eq!(bins.bins[0].key.mesh, MeshId::from_raw(10));
        assert_eq!(bins.bins[1].object_count, 1);
        assert_eq!(bins.bins[1].key.mesh, MeshId::from_raw(11));
    }

    #[test]
    fn hidden_and_meshless_objects_do_not_create_bins() {
        let mut hidden = object(1, 10, 100);
        hidden.visibility = RenderVisibility::hidden();
        let meshless = RenderObjectState::new(GpuObjectId::from_raw(2));

        let bins = RenderWorldPersistentBins::from_states(
            &[hidden, meshless],
            GeometryBackend::ComputeIndirect,
            PipelineClass::GBuffer,
            MaterialShaderClass::PbrOpaque,
            VertexLayoutClass::StaticMesh,
            RenderStateClass::OpaqueDepthWrite,
        );

        assert!(bins.is_empty());
        assert_eq!(bins.object_count, 0);
    }

    #[test]
    fn bin_plan_collapses_cpu_submission_when_indirect_count_is_available() {
        let bins = RenderWorldPersistentBins::from_states(
            &[object(1, 10, 100), object(2, 11, 101)],
            GeometryBackend::ComputeIndirect,
            PipelineClass::GBuffer,
            MaterialShaderClass::PbrOpaque,
            VertexLayoutClass::StaticMesh,
            RenderStateClass::OpaqueDepthWrite,
        );

        let plan = RenderWorldPersistentBinPlan::plan(&bins, true);
        assert_eq!(plan.bin_count, 2);
        assert_eq!(plan.indirect_command_count, 2);
        assert_eq!(plan.cpu_submission_units, 1);
        assert!(plan.is_gpu_submission_friendly());
    }
}
