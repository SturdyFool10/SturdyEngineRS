use sturdy_engine_core::{
    Access, DrawDesc, ImageHandle, ImageUse, PassDesc, PassWork, QueueType, RgState,
    SubresourceRange,
};

use crate::{GpuWorkQueue, OffscreenTarget};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderGraphTarget {
    pub image: ImageHandle,
}

#[derive(Clone, Debug, Default)]
pub struct UiGraphPassBuilder;

impl UiGraphPassBuilder {
    pub fn build_passes(queue: &GpuWorkQueue, target: RenderGraphTarget) -> Vec<PassDesc> {
        queue
            .batches
            .iter()
            .enumerate()
            .filter_map(|(index, batch)| {
                let pipeline = batch.kind.pipeline?;
                Some(PassDesc {
                    name: format!("ui:{}:batch:{index}", queue.tree_name),
                    queue: QueueType::Graphics,
                    shader: batch.kind.shader,
                    pipeline: Some(pipeline),
                    bind_groups: Vec::new(),
                    push_constants: None,
                    work: PassWork::Draw(DrawDesc {
                        vertex_count: 4,
                        instance_count: batch.command_indices.len() as u32,
                        first_vertex: 0,
                        first_instance: 0,
                        vertex_buffer: None,
                        index_buffer: None,
                    }),
                    reads: Vec::new(),
                    writes: vec![ImageUse {
                        image: target.image,
                        access: Access::Write,
                        state: RgState::RenderTarget,
                        subresource: SubresourceRange::WHOLE,
                    }],
                    buffer_reads: Vec::new(),
                    buffer_writes: Vec::new(),
                    clear_colors: Vec::new(),
                    clear_depth: None,
                })
            })
            .collect()
    }

    pub fn target_from_queue(queue: &GpuWorkQueue) -> Option<RenderGraphTarget> {
        match queue.target {
            OffscreenTarget::Image { image, .. } => Some(RenderGraphTarget { image }),
            OffscreenTarget::Swapchain | OffscreenTarget::Named(_) => None,
        }
    }
}
