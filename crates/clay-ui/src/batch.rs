use sturdy_engine_core::{ImageHandle, PipelineHandle, ShaderHandle};

use crate::{RenderCommand, RenderCommandKind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OffscreenTarget {
    Swapchain,
    Image {
        image: ImageHandle,
        width: u32,
        height: u32,
    },
    Named(String),
}

impl OffscreenTarget {
    pub fn surface_extent(&self, fallback_width: u32, fallback_height: u32) -> (u32, u32) {
        match self {
            Self::Swapchain | Self::Named(_) => (fallback_width.max(1), fallback_height.max(1)),
            Self::Image { width, height, .. } => ((*width).max(1), (*height).max(1)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GpuBatchKind {
    pub command: RenderCommandKind,
    pub shader: Option<ShaderHandle>,
    pub pipeline: Option<PipelineHandle>,
    pub clipped: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GpuBatch {
    pub kind: GpuBatchKind,
    pub command_indices: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GpuWorkQueue {
    pub tree_name: String,
    pub target: OffscreenTarget,
    pub commands: Vec<RenderCommand>,
    pub batches: Vec<GpuBatch>,
}

impl GpuWorkQueue {
    pub fn new(tree_name: impl Into<String>, target: OffscreenTarget) -> Self {
        Self {
            tree_name: tree_name.into(),
            target,
            commands: Vec::new(),
            batches: Vec::new(),
        }
    }

    pub fn rebuild_batches(&mut self) {
        self.batches.clear();
        let mut clip_depth = 0usize;
        for (index, command) in self.commands.iter().enumerate() {
            match command.kind {
                RenderCommandKind::ScissorStart => {
                    clip_depth += 1;
                    continue;
                }
                RenderCommandKind::ScissorEnd => {
                    clip_depth = clip_depth.saturating_sub(1);
                    continue;
                }
                _ => {}
            }
            let (shader, pipeline) = shader_for_command(command);
            let kind = GpuBatchKind {
                command: command.kind,
                shader,
                pipeline,
                clipped: clip_depth > 0,
            };
            if let Some(last) = self.batches.last_mut()
                && last.kind == kind
            {
                last.command_indices.push(index);
                continue;
            }
            self.batches.push(GpuBatch {
                kind,
                command_indices: vec![index],
            });
        }
    }
}

fn shader_for_command(command: &RenderCommand) -> (Option<ShaderHandle>, Option<PipelineHandle>) {
    match &command.data {
        crate::render_command::RenderData::Rectangle(data) => {
            let shader = data
                .effect
                .as_ref()
                .map(|effect| effect.shader)
                .or_else(|| data.gradient.as_ref().map(|gradient| gradient.shader))
                .unwrap_or(data.shader);
            (shader.shader, shader.pipeline)
        }
        crate::render_command::RenderData::Border(data) => data
            .effect
            .as_ref()
            .map(|effect| (effect.shader.shader, effect.shader.pipeline))
            .unwrap_or((data.shader.shader, data.shader.pipeline)),
        crate::render_command::RenderData::Text(data) => match data.pass {
            crate::render_command::TextPass::Fill => (
                data.style.fill_shader.shader,
                data.style.fill_shader.pipeline,
            ),
            crate::render_command::TextPass::Outline => data
                .style
                .outline
                .as_ref()
                .map(|outline| (outline.shader.shader, outline.shader.pipeline))
                .unwrap_or((None, None)),
        },
        crate::render_command::RenderData::Image(data) => data
            .effect
            .as_ref()
            .map(|effect| (effect.shader.shader, effect.shader.pipeline))
            .unwrap_or((data.shader.shader, data.shader.pipeline)),
        crate::render_command::RenderData::Custom(data) => data
            .effect
            .as_ref()
            .map(|effect| (effect.shader.shader, effect.shader.pipeline))
            .unwrap_or((data.shader.shader, data.shader.pipeline)),
        _ => (None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ElementId, RectangleRenderData, RenderCommand, RenderData, ShaderRef, ShaderSlot, UiColor,
        UiLayer, UiShaderSlotBinding, UiShape,
    };

    #[test]
    fn batches_use_element_shader_slot_pipeline() {
        let shader = ShaderHandle(11);
        let pipeline = PipelineHandle(22);
        let mut queue = GpuWorkQueue::new("ui", OffscreenTarget::Swapchain);
        queue.commands.push(RenderCommand {
            id: ElementId::new("custom-background"),
            rect: crate::Rect::new(0.0, 0.0, 100.0, 40.0),
            layer: UiLayer::Content,
            z_index: 0,
            kind: RenderCommandKind::Rectangle,
            data: RenderData::Rectangle(RectangleRenderData {
                color: UiColor::WHITE,
                shader: ShaderRef::SOLID_COLOR,
                effect: Some(UiShaderSlotBinding::new(
                    ShaderSlot::Background,
                    ShaderRef::custom(shader, pipeline),
                )),
                gradient: None,
                corner_radius: crate::radii_all(0.0),
                shape: UiShape::Rect,
            }),
        });

        queue.rebuild_batches();

        assert_eq!(queue.batches.len(), 1);
        assert_eq!(queue.batches[0].kind.shader, Some(shader));
        assert_eq!(queue.batches[0].kind.pipeline, Some(pipeline));
    }
}
