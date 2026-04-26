use sturdy_engine_core::{
    Access, BufferUse, DrawDesc, ImageHandle, ImageUse, PassDesc, PassWork, PushConstants,
    QueueType, RgState, StageMask, SubresourceRange,
};

use crate::{
    GpuWorkQueue, OffscreenTarget, RenderCommand, RenderData, UiShaderParameterBatch,
    UiShaderResourceRef, UiShaderSlotBinding, UiShaderUniformPackError, UiShaderUniformPacket,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderGraphTarget {
    pub image: ImageHandle,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiShaderParameterBatchPlan {
    pub batch_index: usize,
    pub command_indices: Vec<usize>,
    pub parameters: UiShaderParameterBatch,
}

#[derive(Clone, Debug, Default)]
pub struct UiGraphPassBuilder;

impl UiGraphPassBuilder {
    pub fn build_passes(queue: &GpuWorkQueue, target: RenderGraphTarget) -> Vec<PassDesc> {
        Self::build_passes_with_resource_resolver(queue, target, |_| None)
    }

    pub fn build_passes_with_resource_resolver(
        queue: &GpuWorkQueue,
        target: RenderGraphTarget,
        mut resolve_named_image: impl FnMut(&str) -> Option<ImageHandle>,
    ) -> Vec<PassDesc> {
        queue
            .batches
            .iter()
            .enumerate()
            .filter_map(|(index, batch)| {
                let pipeline = batch.kind.pipeline?;
                let (reads, buffer_reads) =
                    shader_resource_uses(queue, &batch.command_indices, &mut resolve_named_image);
                let push_constants = shader_push_constants(queue, &batch.command_indices);
                Some(PassDesc {
                    name: format!("ui:{}:batch:{index}", queue.tree_name),
                    queue: QueueType::Graphics,
                    shader: batch.kind.shader,
                    pipeline: Some(pipeline),
                    bind_groups: Vec::new(),
                    push_constants,
                    work: PassWork::Draw(DrawDesc {
                        vertex_count: 4,
                        instance_count: batch.command_indices.len() as u32,
                        first_vertex: 0,
                        first_instance: 0,
                        vertex_buffer: None,
                        index_buffer: None,
                    }),
                    reads,
                    writes: vec![ImageUse {
                        image: target.image,
                        access: Access::Write,
                        state: RgState::RenderTarget,
                        subresource: SubresourceRange::WHOLE,
                    }],
                    buffer_reads,
                    buffer_writes: Vec::new(),
                    clear_colors: Vec::new(),
                    clear_depth: None,
                })
            })
            .collect()
    }

    pub fn plan_shader_parameter_batches(
        queue: &GpuWorkQueue,
    ) -> Result<Vec<UiShaderParameterBatchPlan>, UiShaderUniformPackError> {
        queue
            .batches
            .iter()
            .enumerate()
            .filter_map(|(batch_index, batch)| {
                let commands = batch.command_indices.iter().filter_map(|index| {
                    let command = queue.commands.get(*index)?;
                    let effect = command_effect(command)?;
                    (!effect.uniforms.is_empty()).then_some((*index, effect.uniforms.as_slice()))
                });
                let parameters = match UiShaderParameterBatch::pack_commands(commands) {
                    Ok(parameters) if !parameters.records.is_empty() => parameters,
                    Ok(_) => return None,
                    Err(err) => return Some(Err(err)),
                };

                Some(Ok(UiShaderParameterBatchPlan {
                    batch_index,
                    command_indices: batch.command_indices.clone(),
                    parameters,
                }))
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

fn shader_push_constants(queue: &GpuWorkQueue, command_indices: &[usize]) -> Option<PushConstants> {
    let mut packet: Option<UiShaderUniformPacket> = None;

    for index in command_indices {
        let command = queue.commands.get(*index)?;
        let Some(effect) = command_effect(command) else {
            continue;
        };
        if effect.uniforms.is_empty() {
            continue;
        }
        let current = effect.push_constant_packet().ok()?;
        if let Some(existing) = &packet {
            if existing != &current {
                return None;
            }
        } else {
            packet = Some(current);
        }
    }

    packet.map(|packet| PushConstants {
        offset: 0,
        stages: StageMask::FRAGMENT,
        bytes: packet.bytes,
    })
}

fn shader_resource_uses(
    queue: &GpuWorkQueue,
    command_indices: &[usize],
    resolve_named_image: &mut impl FnMut(&str) -> Option<ImageHandle>,
) -> (Vec<ImageUse>, Vec<BufferUse>) {
    let mut reads = Vec::new();
    let mut buffer_reads = Vec::new();

    for index in command_indices {
        let Some(command) = queue.commands.get(*index) else {
            continue;
        };
        let Some(effect) = command_effect(command) else {
            continue;
        };
        for resource in &effect.resources {
            match &resource.resource {
                UiShaderResourceRef::Image(image) => push_unique_image_read(&mut reads, *image),
                UiShaderResourceRef::NamedImage(name) => {
                    if let Some(image) = resolve_named_image(name) {
                        push_unique_image_read(&mut reads, image);
                    }
                }
                UiShaderResourceRef::Buffer(buffer) => {
                    let use_ = BufferUse {
                        buffer: *buffer,
                        access: Access::Read,
                        state: RgState::ShaderRead,
                        offset: 0,
                        size: u64::MAX,
                    };
                    if !buffer_reads.contains(&use_) {
                        buffer_reads.push(use_);
                    }
                }
            }
        }
    }

    (reads, buffer_reads)
}

fn command_effect(command: &RenderCommand) -> Option<&UiShaderSlotBinding> {
    match &command.data {
        RenderData::Rectangle(data) => data.effect.as_ref(),
        RenderData::Border(data) => data.effect.as_ref(),
        RenderData::Image(data) => data.effect.as_ref(),
        RenderData::Custom(data) => data.effect.as_ref(),
        RenderData::None | RenderData::Text(_) | RenderData::Clip(_) => None,
    }
}

fn push_unique_image_read(reads: &mut Vec<ImageUse>, image: ImageHandle) {
    let use_ = ImageUse {
        image,
        access: Access::Read,
        state: RgState::ShaderRead,
        subresource: SubresourceRange::WHOLE,
    };
    if !reads.contains(&use_) {
        reads.push(use_);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ElementId, GpuWorkQueue, OffscreenTarget, RectangleRenderData, RenderCommand,
        RenderCommandKind, ShaderRef, ShaderSlot, UiColor, UiLayer, UiShaderResource,
        UiShaderSlotBinding, UiShaderUniform, UiShaderUniformValue, UiShape,
    };
    use sturdy_engine_core::{BufferHandle, ImageHandle, PipelineHandle, ShaderHandle};

    #[test]
    fn graph_passes_include_shader_slot_image_and_buffer_reads() {
        let scene = ImageHandle(3);
        let resolved = ImageHandle(4);
        let buffer = BufferHandle(5);
        let shader = ShaderRef::custom(ShaderHandle(6), PipelineHandle(7));
        let mut queue = GpuWorkQueue::new("ui", OffscreenTarget::Swapchain);
        queue.commands.push(RenderCommand {
            id: ElementId::new("effect"),
            rect: crate::Rect::new(0.0, 0.0, 100.0, 40.0),
            layer: UiLayer::Content,
            z_index: 0,
            kind: RenderCommandKind::Rectangle,
            data: RenderData::Rectangle(RectangleRenderData {
                color: UiColor::WHITE,
                shader: ShaderRef::SOLID_COLOR,
                effect: Some(
                    UiShaderSlotBinding::new(ShaderSlot::Background, shader)
                        .with_resource(UiShaderResource::image("scene", scene, None))
                        .with_resource(UiShaderResource::named_image("blurred", "scene-blur"))
                        .with_resource(UiShaderResource::buffer("params", buffer)),
                ),
                gradient: None,
                corner_radius: crate::radii_all(0.0),
                shape: UiShape::Rect,
            }),
        });
        queue.rebuild_batches();

        let passes = UiGraphPassBuilder::build_passes_with_resource_resolver(
            &queue,
            RenderGraphTarget {
                image: ImageHandle(9),
            },
            |name| (name == "scene-blur").then_some(resolved),
        );

        assert_eq!(passes.len(), 1);
        assert_eq!(passes[0].reads.len(), 2);
        assert!(passes[0].reads.iter().any(|use_| use_.image == scene));
        assert!(passes[0].reads.iter().any(|use_| use_.image == resolved));
        assert_eq!(passes[0].buffer_reads.len(), 1);
        assert_eq!(passes[0].buffer_reads[0].buffer, buffer);
        assert_eq!(passes[0].buffer_reads[0].state, RgState::ShaderRead);
    }

    #[test]
    fn graph_passes_pack_single_command_shader_uniforms_as_push_constants() {
        let shader = ShaderRef::custom(ShaderHandle(6), PipelineHandle(7));
        let mut queue = GpuWorkQueue::new("ui", OffscreenTarget::Swapchain);
        queue.commands.push(RenderCommand {
            id: ElementId::new("effect"),
            rect: crate::Rect::new(0.0, 0.0, 100.0, 40.0),
            layer: UiLayer::Content,
            z_index: 0,
            kind: RenderCommandKind::Rectangle,
            data: RenderData::Rectangle(RectangleRenderData {
                color: UiColor::WHITE,
                shader: ShaderRef::SOLID_COLOR,
                effect: Some(
                    UiShaderSlotBinding::new(ShaderSlot::Background, shader)
                        .with_uniform(UiShaderUniform::new(
                            "amount",
                            UiShaderUniformValue::Float(0.5),
                        ))
                        .with_uniform(UiShaderUniform::new(
                            "offset",
                            UiShaderUniformValue::Vec2([1.0, 2.0]),
                        )),
                ),
                gradient: None,
                corner_radius: crate::radii_all(0.0),
                shape: UiShape::Rect,
            }),
        });
        queue.rebuild_batches();

        let passes = UiGraphPassBuilder::build_passes(
            &queue,
            RenderGraphTarget {
                image: ImageHandle(9),
            },
        );

        let push_constants = passes[0].push_constants.as_ref().unwrap();
        assert_eq!(push_constants.offset, 0);
        assert_eq!(push_constants.stages, StageMask::FRAGMENT);
        assert_eq!(push_constants.bytes.len(), 12);
    }

    #[test]
    fn graph_parameter_plan_packs_multi_command_uniform_payloads() {
        let shader = ShaderRef::custom(ShaderHandle(6), PipelineHandle(7));
        let mut queue = GpuWorkQueue::new("ui", OffscreenTarget::Swapchain);
        for (index, amount) in [(0, 0.25_f32), (1, 0.75_f32)] {
            queue.commands.push(RenderCommand {
                id: ElementId::new(format!("effect-{index}")),
                rect: crate::Rect::new(0.0, 0.0, 100.0, 40.0),
                layer: UiLayer::Content,
                z_index: 0,
                kind: RenderCommandKind::Rectangle,
                data: RenderData::Rectangle(RectangleRenderData {
                    color: UiColor::WHITE,
                    shader: ShaderRef::SOLID_COLOR,
                    effect: Some(
                        UiShaderSlotBinding::new(ShaderSlot::Background, shader).with_uniform(
                            UiShaderUniform::new("amount", UiShaderUniformValue::Float(amount)),
                        ),
                    ),
                    gradient: None,
                    corner_radius: crate::radii_all(0.0),
                    shape: UiShape::Rect,
                }),
            });
        }
        queue.rebuild_batches();

        let plans = UiGraphPassBuilder::plan_shader_parameter_batches(&queue).unwrap();

        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].batch_index, 0);
        assert_eq!(plans[0].parameters.records.len(), 2);
        assert_eq!(plans[0].parameters.records[0].command_index, 0);
        assert_eq!(plans[0].parameters.records[0].offset, 0);
        assert_eq!(plans[0].parameters.records[1].command_index, 1);
        assert_eq!(plans[0].parameters.records[1].offset, 16);
        assert_eq!(plans[0].parameters.bytes.len(), 32);
    }
}
