use std::collections::HashMap;

use crate::Buffer;

use super::{GpuObjectId, RenderWorldBatchRange};

/// GPU-owned render-world scene and cull-output buffers plus per-mesh ranges.
pub struct RenderWorldGpuSceneState {
    pub buffer: Option<Buffer>,
    pub indirect_buffer: Option<Buffer>,
    pub capacity: usize,
    pub indirect_capacity: usize,
    pub ranges: HashMap<u32, RenderWorldBatchRange>,
    pub object_slots: HashMap<GpuObjectId, u32>,
}

impl RenderWorldGpuSceneState {
    pub fn new() -> Self {
        Self {
            buffer: None,
            indirect_buffer: None,
            capacity: 0,
            indirect_capacity: 0,
            ranges: HashMap::new(),
            object_slots: HashMap::new(),
        }
    }
}
