use std::collections::HashMap;

use crate::GpuInstanceData;

use super::RenderWorldBatchRange;

/// CPU-built contents and batch ranges for the render-world GPU scene buffer.
#[derive(Clone, Debug, Default)]
pub struct RenderWorldGpuSceneData {
    pub instances: Vec<GpuInstanceData>,
    pub ranges: HashMap<u32, RenderWorldBatchRange>,
}

impl RenderWorldGpuSceneData {
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    pub fn len(&self) -> usize {
        self.instances.len()
    }

    pub fn range_for_mesh(&self, mesh_id: u32) -> Option<RenderWorldBatchRange> {
        self.ranges.get(&mesh_id).copied()
    }
}
