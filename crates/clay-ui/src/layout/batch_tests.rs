// Tests extracted from crates/clay-ui/src/layout/batch.rs
// Runtime code should stay separate from test code.

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
