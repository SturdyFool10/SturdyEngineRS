// Tests extracted from crates/clay-ui/src/layout/render_graph.rs
// Runtime code should stay separate from test code.

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
