use crate::{
    Edges, Element, ElementId, ElementKind, Gradient, LayoutTree, Rect, ShaderRef, ShaderSlot,
    TextStyle, UiColor, UiImageOptions, UiLayer, UiShaderSlotBinding, UiShape,
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
    pub options: UiImageOptions,
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

    let background_effect = element.style.shader_slot(ShaderSlot::Background).cloned();
    if element.style.background.is_visible()
        || element.style.background_gradient.is_some()
        || element.style.background_shader != ShaderRef::SOLID_COLOR
        || background_effect.is_some()
    {
        list.commands.push(RenderCommand {
            id: element.id.clone(),
            rect,
            layer,
            z_index,
            kind: RenderCommandKind::Rectangle,
            data: RenderData::Rectangle(RectangleRenderData {
                color: element.style.background,
                shader: element.style.background_shader,
                effect: background_effect,
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
                options: image.options,
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

    let outline_effect = element
        .style
        .shader_slot(ShaderSlot::Border)
        .or_else(|| element.style.shader_slot(ShaderSlot::Outline))
        .cloned();
    let has_outline_width = element.style.outline_width.left > 0.0
        || element.style.outline_width.right > 0.0
        || element.style.outline_width.top > 0.0
        || element.style.outline_width.bottom > 0.0;
    if has_outline_width
        && (element.style.outline.is_visible()
            || element.style.outline_shader != ShaderRef::SOLID_COLOR
            || outline_effect.is_some())
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
                effect: outline_effect,
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
#[path = "render_command_tests.rs"]
mod tests;
