use crate::{
    Edges, Element, ElementId, ElementKind, Gradient, LayoutTree, Rect, ShaderRef, ShaderSlot,
    TextStyle, UiColor, UiLayer, UiShaderSlotBinding, UiShape,
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RenderCommandKind {
    None,
    Rectangle,
    Border,
    Text,
    Image,
    ScissorStart,
    ScissorEnd,
    Custom,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RectangleRenderData {
    pub color: UiColor,
    pub shader: ShaderRef,
    pub effect: Option<UiShaderSlotBinding>,
    pub gradient: Option<Gradient>,
    pub corner_radius: glam::Vec4,
    pub shape: UiShape,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BorderRenderData {
    pub color: UiColor,
    pub shader: ShaderRef,
    pub effect: Option<UiShaderSlotBinding>,
    pub width: Edges,
    pub corner_radius: glam::Vec4,
    pub shape: UiShape,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextRenderData {
    pub text: String,
    pub style: TextStyle,
    pub pass: TextPass,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TextPass {
    Fill,
    Outline,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageRenderData {
    pub image_key: String,
    pub tint: UiColor,
    pub shader: ShaderRef,
    pub effect: Option<UiShaderSlotBinding>,
    pub corner_radius: glam::Vec4,
    pub shape: UiShape,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClipRenderData {
    pub horizontal: bool,
    pub vertical: bool,
    pub shape: UiShape,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CustomRenderData {
    pub key: String,
    pub color: UiColor,
    pub shader: ShaderRef,
    pub effect: Option<UiShaderSlotBinding>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RenderData {
    None,
    Rectangle(RectangleRenderData),
    Border(BorderRenderData),
    Text(TextRenderData),
    Image(ImageRenderData),
    Clip(ClipRenderData),
    Custom(CustomRenderData),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderCommand {
    pub id: ElementId,
    pub rect: Rect,
    pub layer: UiLayer,
    pub z_index: i16,
    pub kind: RenderCommandKind,
    pub data: RenderData,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderCommandList {
    pub commands: Vec<RenderCommand>,
}

impl RenderCommandList {
    pub fn from_element_tree(root: &Element, layout: &LayoutTree) -> Self {
        let mut list = Self::default();
        append_element_commands(root, layout, &mut list);
        list.sort_for_rendering();
        list
    }

    pub fn sort_for_rendering(&mut self) {
        self.commands
            .sort_by_key(|command| (command.layer, command.z_index, command.id.hash));
    }
}

fn append_element_commands(element: &Element, layout: &LayoutTree, list: &mut RenderCommandList) {
    let Some(node) = layout.by_id(&element.id) else {
        return;
    };
    let rect = node.rect;
    let shape = node.shape;
    let layer = node.layer;
    let z_index = node.z_index;

    if element.layout.clip_x || element.layout.clip_y {
        list.commands.push(RenderCommand {
            id: element.id.clone(),
            rect,
            layer,
            z_index,
            kind: RenderCommandKind::ScissorStart,
            data: RenderData::Clip(ClipRenderData {
                horizontal: element.layout.clip_x,
                vertical: element.layout.clip_y,
                shape,
            }),
        });
    }

    if element.style.background.is_visible() || element.style.background_gradient.is_some() {
        list.commands.push(RenderCommand {
            id: element.id.clone(),
            rect,
            layer,
            z_index,
            kind: RenderCommandKind::Rectangle,
            data: RenderData::Rectangle(RectangleRenderData {
                color: element.style.background,
                shader: element.style.background_shader,
                effect: element.style.shader_slot(ShaderSlot::Background).cloned(),
                gradient: element.style.background_gradient.clone(),
                corner_radius: element.style.corner_radius,
                shape,
            }),
        });
    }

    match &element.kind {
        ElementKind::Container => {}
        ElementKind::Text(text) => list.commands.push(RenderCommand {
            id: element.id.clone(),
            rect,
            layer,
            z_index,
            kind: RenderCommandKind::Text,
            data: RenderData::Text(TextRenderData {
                text: text.text.clone(),
                style: text.style.clone(),
                pass: TextPass::Fill,
            }),
        }),
        ElementKind::Image(image) => list.commands.push(RenderCommand {
            id: element.id.clone(),
            rect,
            layer,
            z_index,
            kind: RenderCommandKind::Image,
            data: RenderData::Image(ImageRenderData {
                image_key: image.image_key.clone(),
                tint: image.tint,
                shader: image.shader,
                effect: element.style.shader_slot(ShaderSlot::Image).cloned(),
                corner_radius: element.style.corner_radius,
                shape,
            }),
        }),
        ElementKind::Custom(key) => list.commands.push(RenderCommand {
            id: element.id.clone(),
            rect,
            layer,
            z_index,
            kind: RenderCommandKind::Custom,
            data: RenderData::Custom(CustomRenderData {
                key: key.clone(),
                color: element.style.background,
                shader: element.style.background_shader,
                effect: element.style.shader_slot(ShaderSlot::Custom).cloned(),
            }),
        }),
    }

    if element.style.outline.is_visible()
        && (element.style.outline_width.left > 0.0
            || element.style.outline_width.right > 0.0
            || element.style.outline_width.top > 0.0
            || element.style.outline_width.bottom > 0.0)
    {
        list.commands.push(RenderCommand {
            id: element.id.clone(),
            rect,
            layer,
            z_index,
            kind: RenderCommandKind::Border,
            data: RenderData::Border(BorderRenderData {
                color: element.style.outline,
                shader: element.style.outline_shader,
                effect: element
                    .style
                    .shader_slot(ShaderSlot::Border)
                    .or_else(|| element.style.shader_slot(ShaderSlot::Outline))
                    .cloned(),
                width: element.style.outline_width,
                corner_radius: element.style.corner_radius,
                shape,
            }),
        });
    }

    for child in &element.children {
        append_element_commands(child, layout, list);
    }

    if let ElementKind::Text(text) = &element.kind
        && text.style.outline.is_some()
    {
        list.commands.push(RenderCommand {
            id: element.id.clone(),
            rect,
            layer,
            z_index,
            kind: RenderCommandKind::Text,
            data: RenderData::Text(TextRenderData {
                text: text.text.clone(),
                style: text.style.clone(),
                pass: TextPass::Outline,
            }),
        });
    }

    if element.layout.clip_x || element.layout.clip_y {
        list.commands.push(RenderCommand {
            id: element.id.clone(),
            rect,
            layer,
            z_index,
            kind: RenderCommandKind::ScissorEnd,
            data: RenderData::Clip(ClipRenderData {
                horizontal: element.layout.clip_x,
                vertical: element.layout.clip_y,
                shape,
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ElementStyle, LayoutCache, LayoutInput, LayoutSizing, Size, UiLayer, UiShaderUniform,
        UiShaderUniformValue, UiShape,
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
}
