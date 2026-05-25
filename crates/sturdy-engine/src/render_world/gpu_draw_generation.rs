use std::collections::HashMap;

use bytemuck::Zeroable;

use crate::{DrawIndexedIndirectCommand, MeshId};

use super::RenderWorldPersistentBins;

/// Per-mesh draw parameters needed to turn a persistent bin into an indirect draw command.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RenderWorldGpuMeshDrawInfo {
    pub mesh: MeshId,
    pub index_count: u32,
    pub first_index: u32,
    pub vertex_offset: i32,
}

impl RenderWorldGpuMeshDrawInfo {
    pub const fn indexed(mesh: MeshId, index_count: u32) -> Self {
        Self {
            mesh,
            index_count,
            first_index: 0,
            vertex_offset: 0,
        }
    }
}

/// GPU-readable persistent bin metadata for draw generation.
#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct RenderWorldGpuBinData {
    pub first_object_slot: u32,
    pub object_count: u32,
    pub mesh_id: u32,
    pub pipeline_key: u32,
    pub index_count: u32,
    pub first_index: u32,
    pub vertex_offset: i32,
    pub _pad: u32,
}

impl RenderWorldGpuBinData {
    pub fn from_bins(
        bins: &RenderWorldPersistentBins,
        mesh_draws: impl IntoIterator<Item = RenderWorldGpuMeshDrawInfo>,
    ) -> Result<Vec<Self>, String> {
        let mesh_draws: HashMap<u32, RenderWorldGpuMeshDrawInfo> = mesh_draws
            .into_iter()
            .map(|draw| (draw.mesh.as_u32(), draw))
            .collect();
        let mut gpu_bins = Vec::with_capacity(bins.bins.len());
        for bin in &bins.bins {
            let mesh_id = bin.key.mesh.as_u32();
            let Some(draw) = mesh_draws.get(&mesh_id).copied() else {
                return Err(format!(
                    "missing GPU draw info for render-world bin mesh {}",
                    mesh_id
                ));
            };
            gpu_bins.push(Self {
                first_object_slot: bin.first_object_slot,
                object_count: bin.object_count,
                mesh_id,
                pipeline_key: 0,
                index_count: draw.index_count,
                first_index: draw.first_index,
                vertex_offset: draw.vertex_offset,
                _pad: 0,
            });
        }
        Ok(gpu_bins)
    }
}

/// Plan for render-world GPU draw generation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderWorldGpuDrawGenerationPlan {
    pub bin_count: u32,
    pub object_count: u32,
    pub workgroup_count: u32,
    pub bins_per_workgroup: u32,
    pub uses_indirect_count: bool,
    pub degraded_reason: Option<String>,
}

impl RenderWorldGpuDrawGenerationPlan {
    pub fn plan(
        bins: &RenderWorldPersistentBins,
        use_indirect_count: bool,
        bins_per_workgroup: u32,
    ) -> Self {
        let bin_count = bins.bins.len().min(u32::MAX as usize) as u32;
        let bins_per_workgroup = bins_per_workgroup.clamp(32, 256);
        let workgroup_count = if bin_count == 0 {
            0
        } else {
            bin_count.div_ceil(bins_per_workgroup).max(1)
        };
        Self {
            bin_count,
            object_count: bins.object_count,
            workgroup_count,
            bins_per_workgroup,
            uses_indirect_count: use_indirect_count,
            degraded_reason: (!use_indirect_count).then_some(
                "indirect-count unavailable, generated commands must be submitted as fixed ranges"
                    .to_string(),
            ),
        }
    }
}

/// Upload/allocation stats for render-world GPU draw generation resources.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderWorldGpuDrawGenerationStats {
    pub bin_count: usize,
    pub object_count: usize,
    pub bin_buffer_reallocated: bool,
    pub indirect_buffer_reallocated: bool,
    pub count_buffer_reallocated: bool,
    pub visible_instance_buffer_reallocated: bool,
    pub uploaded_bin_bytes: u64,
    pub indirect_bytes: u64,
    pub visible_instance_bytes: u64,
    pub uses_indirect_count: bool,
    pub degraded_reason: Option<String>,
}

/// Dispatch result for render-world GPU draw generation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderWorldGpuDrawGenerationDispatchStats {
    pub dispatched: bool,
    pub bin_count: u32,
    pub object_count: u32,
    pub workgroup_count: u32,
    pub bins_per_workgroup: u32,
    pub skipped_reason: Option<String>,
}

/// GPU draw output buffers and draw-count policy produced by render-world draw generation.
pub struct RenderWorldGpuDrawOutput<'a> {
    pub indirect_commands: &'a crate::Buffer,
    pub visible_draw_count: &'a crate::Buffer,
    /// Compact per-bin visible object slots written by draw generation.
    ///
    /// Generated indirect commands use `first_instance` as an offset into this table.
    /// A RenderWorld-aware vertex shader can map `SV_InstanceID` through this buffer before
    /// reading GPU-derived matrices or legacy instance data, making sparse visibility safe.
    pub visible_instances: &'a crate::Buffer,
    pub current_matrices: &'a crate::Buffer,
    pub max_draw_count: u32,
    pub use_indirect_count: bool,
}

impl RenderWorldGpuDrawGenerationPlan {
    pub const fn fixed_range_draw_count(&self) -> u32 {
        self.bin_count
    }
}

pub fn zero_indirect_commands(count: usize) -> Vec<DrawIndexedIndirectCommand> {
    vec![DrawIndexedIndirectCommand::zeroed(); count]
}
