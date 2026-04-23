use glam::Vec4;

use crate::{Edges, ElementId, Gradient, ShaderRef, Size, TextStyle, UiColor, geometry::radii_all};

#[derive(Clone, Debug, PartialEq)]
pub struct ElementStyle {
    pub background: UiColor,
    pub background_shader: ShaderRef,
    pub background_gradient: Option<Gradient>,
    pub outline: UiColor,
    pub outline_shader: ShaderRef,
    pub outline_width: Edges,
    pub corner_radius: Vec4,
    pub padding: Edges,
    pub transparent_to_input: bool,
}

impl Default for ElementStyle {
    fn default() -> Self {
        Self {
            background: UiColor::TRANSPARENT,
            background_shader: ShaderRef::SOLID_COLOR,
            background_gradient: None,
            outline: UiColor::TRANSPARENT,
            outline_shader: ShaderRef::SOLID_COLOR,
            outline_width: Edges::ZERO,
            corner_radius: radii_all(0.0),
            padding: Edges::ZERO,
            transparent_to_input: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextElement {
    pub text: String,
    pub style: TextStyle,
}

impl TextElement {
    pub fn new(text: impl Into<String>, style: TextStyle) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageElement {
    pub image_key: String,
    pub natural_size: Option<Size>,
    pub tint: UiColor,
    pub shader: ShaderRef,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ElementKind {
    Container,
    Text(TextElement),
    Image(ImageElement),
    Custom(String),
}

impl Default for ElementKind {
    fn default() -> Self {
        Self::Container
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub id: ElementId,
    pub kind: ElementKind,
    pub style: ElementStyle,
    pub layout: crate::LayoutInput,
    pub children: Vec<Element>,
    pub user_data: Option<String>,
}

impl Element {
    pub fn new(id: ElementId) -> Self {
        Self {
            id,
            kind: ElementKind::Container,
            style: ElementStyle::default(),
            layout: crate::LayoutInput::default(),
            children: Vec::new(),
            user_data: None,
        }
    }

    pub fn text(id: ElementId, text: impl Into<String>, style: TextStyle) -> Self {
        let mut element = Self::new(id);
        element.kind = ElementKind::Text(TextElement::new(text, style));
        element
    }

    pub fn image(id: ElementId, image_key: impl Into<String>) -> Self {
        let mut element = Self::new(id);
        element.kind = ElementKind::Image(ImageElement {
            image_key: image_key.into(),
            natural_size: None,
            tint: UiColor::TRANSPARENT,
            shader: ShaderRef::SOLID_COLOR,
        });
        element
    }
}
