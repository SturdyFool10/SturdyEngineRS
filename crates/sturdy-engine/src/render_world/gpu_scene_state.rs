use std::collections::HashMap;

use crate::Buffer;

use super::{
    GpuObjectId, RenderWorldBatchRange, RenderWorldGpuCullPlan, RenderWorldGpuDrawGenerationPlan,
    RenderWorldGpuMatrixPlan,
};

/// GPU-owned render-world scene and cull-output buffers plus per-mesh ranges.
pub struct RenderWorldGpuSceneState {
    pub buffer: Option<Buffer>,
    pub indirect_buffer: Option<Buffer>,
    pub capacity: usize,
    pub indirect_capacity: usize,
    pub ranges: HashMap<u32, RenderWorldBatchRange>,
    pub object_slots: HashMap<GpuObjectId, u32>,
    pub transform_source_buffer: Option<Buffer>,
    pub current_matrix_buffer: Option<Buffer>,
    pub previous_matrix_buffer: Option<Buffer>,
    pub normal_matrix_buffer: Option<Buffer>,
    pub world_bounds_buffer: Option<Buffer>,
    pub visibility_flags_buffer: Option<Buffer>,
    pub draw_bin_buffer: Option<Buffer>,
    pub draw_indirect_buffer: Option<Buffer>,
    pub draw_count_buffer: Option<Buffer>,
    pub transform_source_capacity: usize,
    pub visibility_flags_capacity: usize,
    pub draw_bin_capacity: usize,
    pub draw_indirect_capacity: usize,
    pub transform_source_slots: HashMap<GpuObjectId, u32>,
    pub matrix_plan: Option<RenderWorldGpuMatrixPlan>,
    pub cull_plan: Option<RenderWorldGpuCullPlan>,
    pub draw_generation_plan: Option<RenderWorldGpuDrawGenerationPlan>,
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
            transform_source_buffer: None,
            current_matrix_buffer: None,
            previous_matrix_buffer: None,
            normal_matrix_buffer: None,
            world_bounds_buffer: None,
            visibility_flags_buffer: None,
            draw_bin_buffer: None,
            draw_indirect_buffer: None,
            draw_count_buffer: None,
            transform_source_capacity: 0,
            visibility_flags_capacity: 0,
            draw_bin_capacity: 0,
            draw_indirect_capacity: 0,
            transform_source_slots: HashMap::new(),
            matrix_plan: None,
            cull_plan: None,
            draw_generation_plan: None,
        }
    }
}
