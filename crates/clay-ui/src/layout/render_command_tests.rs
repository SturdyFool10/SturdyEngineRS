// Tests extracted from crates/clay-ui/src/layout/render_command.rs
// Runtime code should stay separate from test code.

use super::*;
use crate::{
    ElementStyle, LayoutCache, LayoutInput, LayoutSizing, Size, UiAntialiasing, UiImageFit,
    UiImageOptions, UiImageSampling, UiLayer, UiShaderUniform, UiShaderUniformValue, UiShape,
};
use sturdy_engine_core::{PipelineHandle, ShaderHandle};

#[test]
fn render_commands_sort_by_layer_before_z_index() {
    let overlay_id = ElementId::new("overlay");
    let base_id = ElementId::new("base");
    let mut root = Element::new(ElementId::new("root"));
    root.layout.width = LayoutSizing::Fixed(100.0);
    root.layout.height = LayoutSizing::Fixed(40.0);

    let mut overlay = Element::new(overlay_id.clone());
    overlay.layout = LayoutInput {
        width: LayoutSizing::Fixed(100.0),
        height: LayoutSizing::Fixed(40.0),
        layer: UiLayer::Overlay,
        z_index: 0,
        ..LayoutInput::default()
    };
    overlay.style = ElementStyle {
        background: UiColor::WHITE,
        ..ElementStyle::default()
    };

    let mut base = Element::new(base_id.clone());
    base.layout = LayoutInput {
        width: LayoutSizing::Fixed(100.0),
        height: LayoutSizing::Fixed(40.0),
        layer: UiLayer::Content,
        z_index: 100,
        ..LayoutInput::default()
    };
    base.style = ElementStyle {
        background: UiColor::WHITE,
        ..ElementStyle::default()
    };

    root.children.push(overlay);
    root.children.push(base);
    let layout =
        LayoutTree::compute(&root, Size::new(100.0, 40.0), &mut LayoutCache::default())
            .unwrap();
    let commands = RenderCommandList::from_element_tree(&root, &layout);
    let rectangles = commands
        .commands
        .iter()
        .filter(|command| command.kind == RenderCommandKind::Rectangle)
        .map(|command| command.id.hash)
        .collect::<Vec<_>>();

    assert_eq!(rectangles, vec![base_id.hash, overlay_id.hash]);
}

#[test]
fn render_commands_carry_resolved_shape() {
    let id = ElementId::new("squircle");
    let mut element = Element::new(id.clone());
    element.layout.width = LayoutSizing::Fixed(100.0);
    element.layout.height = LayoutSizing::Fixed(40.0);
    element.style = ElementStyle {
        background: UiColor::WHITE,
        shape: UiShape::squircle(12.0, 4.0),
        ..ElementStyle::default()
    };
    let layout = LayoutTree::compute(
        &element,
        Size::new(100.0, 40.0),
        &mut LayoutCache::default(),
    )
    .unwrap();
    let commands = RenderCommandList::from_element_tree(&element, &layout);

    let shape = commands
        .commands
        .iter()
        .find_map(|command| match &command.data {
            RenderData::Rectangle(data) => Some(data.shape),
            _ => None,
        })
        .unwrap();

    assert_eq!(shape, UiShape::squircle(12.0, 4.0));
}

#[test]
fn clip_commands_carry_resolved_shape() {
    let id = ElementId::new("clipped-squircle");
    let mut element = Element::new(id);
    element.layout.width = LayoutSizing::Fixed(100.0);
    element.layout.height = LayoutSizing::Fixed(40.0);
    element.layout.clip_x = true;
    element.layout.clip_y = true;
    element.style.shape = UiShape::squircle(14.0, 4.0);
    let layout = LayoutTree::compute(
        &element,
        Size::new(100.0, 40.0),
        &mut LayoutCache::default(),
    )
    .unwrap();
    let commands = RenderCommandList::from_element_tree(&element, &layout);

    let clip_shapes = commands
        .commands
        .iter()
        .filter_map(|command| match &command.data {
            RenderData::Clip(data) => Some(data.shape),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        clip_shapes,
        vec![UiShape::squircle(14.0, 4.0), UiShape::squircle(14.0, 4.0)]
    );
}

#[test]
fn render_commands_carry_element_shader_slot_uniforms() {
    let id = ElementId::new("shader-slot");
    let shader = ShaderRef::custom(ShaderHandle(10), PipelineHandle(20));
    let mut element = Element::new(id);
    element.layout.width = LayoutSizing::Fixed(100.0);
    element.layout.height = LayoutSizing::Fixed(40.0);
    element.style = ElementStyle {
        background: UiColor::WHITE,
        shader_slots: vec![
            UiShaderSlotBinding::new(ShaderSlot::Background, shader).with_uniform(
                UiShaderUniform::new("intensity", UiShaderUniformValue::Float(0.75)),
            ),
        ],
        ..ElementStyle::default()
    };
    let layout = LayoutTree::compute(
        &element,
        Size::new(100.0, 40.0),
        &mut LayoutCache::default(),
    )
    .unwrap();
    let commands = RenderCommandList::from_element_tree(&element, &layout);

    let effect = commands
        .commands
        .iter()
        .find_map(|command| match &command.data {
            RenderData::Rectangle(data) => data.effect.as_ref(),
            _ => None,
        })
        .unwrap();

    assert_eq!(effect.shader, shader);
    assert_eq!(
        effect.uniform("intensity").map(|uniform| &uniform.value),
        Some(&UiShaderUniformValue::Float(0.75))
    );
}

#[test]
fn transparent_background_with_shader_slot_still_emits_rectangle() {
    let id = ElementId::new("transparent-shader-bg");
    let shader = ShaderRef::custom(ShaderHandle(11), PipelineHandle(21));
    let mut element = Element::new(id.clone());
    element.layout.width = LayoutSizing::Fixed(100.0);
    element.layout.height = LayoutSizing::Fixed(40.0);
    element.style = ElementStyle {
        background: UiColor::TRANSPARENT,
        shader_slots: vec![UiShaderSlotBinding::new(ShaderSlot::Background, shader)],
        ..ElementStyle::default()
    };
    let layout = LayoutTree::compute(
        &element,
        Size::new(100.0, 40.0),
        &mut LayoutCache::default(),
    )
    .unwrap();
    let commands = RenderCommandList::from_element_tree(&element, &layout);

    let data = commands
        .commands
        .iter()
        .find_map(|command| match &command.data {
            RenderData::Rectangle(data) => Some(data),
            _ => None,
        })
        .unwrap();

    assert_eq!(data.color, UiColor::TRANSPARENT);
    assert_eq!(
        data.effect.as_ref().map(|effect| effect.shader),
        Some(shader)
    );
}

#[test]
fn transparent_outline_with_shader_slot_still_emits_border() {
    let id = ElementId::new("transparent-shader-border");
    let shader = ShaderRef::custom(ShaderHandle(12), PipelineHandle(22));
    let mut element = Element::new(id.clone());
    element.layout.width = LayoutSizing::Fixed(100.0);
    element.layout.height = LayoutSizing::Fixed(40.0);
    element.style = ElementStyle {
        outline: UiColor::TRANSPARENT,
        outline_width: Edges::all(2.0),
        shader_slots: vec![UiShaderSlotBinding::new(ShaderSlot::Border, shader)],
        ..ElementStyle::default()
    };
    let layout = LayoutTree::compute(
        &element,
        Size::new(100.0, 40.0),
        &mut LayoutCache::default(),
    )
    .unwrap();
    let commands = RenderCommandList::from_element_tree(&element, &layout);

    let data = commands
        .commands
        .iter()
        .find_map(|command| match &command.data {
            RenderData::Border(data) => Some(data),
            _ => None,
        })
        .unwrap();

    assert_eq!(data.color, UiColor::TRANSPARENT);
    assert_eq!(
        data.effect.as_ref().map(|effect| effect.shader),
        Some(shader)
    );
}

#[test]
fn image_commands_carry_sampling_fit_and_edge_aa() {
    let id = ElementId::new("image");
    let mut element = Element::image(id, "icon");
    element.layout.width = LayoutSizing::Fixed(32.0);
    element.layout.height = LayoutSizing::Fixed(32.0);
    if let ElementKind::Image(image) = &mut element.kind {
        image.options = UiImageOptions::default()
            .fit(UiImageFit::Cover)
            .sampling(UiImageSampling::Nearest)
            .edge_antialiasing(UiAntialiasing::supersampled(4));
    }
    let layout =
        LayoutTree::compute(&element, Size::new(32.0, 32.0), &mut LayoutCache::default())
            .unwrap();
    let commands = RenderCommandList::from_element_tree(&element, &layout);

    let data = commands
        .commands
        .iter()
        .find_map(|command| match &command.data {
            RenderData::Image(data) => Some(data),
            _ => None,
        })
        .unwrap();

    assert_eq!(data.image_key, "icon");
    assert_eq!(data.tint, UiColor::WHITE);
    assert_eq!(data.options.fit, UiImageFit::Cover);
    assert_eq!(data.options.sampling, UiImageSampling::Nearest);
    assert_eq!(
        data.options.edge_antialiasing,
        UiAntialiasing::supersampled(4)
    );
}
