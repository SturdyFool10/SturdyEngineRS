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
                .gradient
                .as_ref()
                .map(|gradient| gradient.shader)
                .unwrap_or(data.shader);
            (shader.shader, shader.pipeline)
        }
        crate::render_command::RenderData::Border(data) => {
            (data.shader.shader, data.shader.pipeline)
        }
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
        crate::render_command::RenderData::Image(data) => {
            (data.shader.shader, data.shader.pipeline)
        }
        crate::render_command::RenderData::Custom(data) => {
            (data.shader.shader, data.shader.pipeline)
        }
        _ => (None, None),
    }
}
