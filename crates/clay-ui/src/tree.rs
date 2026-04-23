use crate::{Element, ElementId, ElementKind, ElementStyle, LayoutInput, TextStyle};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UiTree {
    pub roots: Vec<Element>,
}

impl UiTree {
    pub fn new() -> Self {
        Self { roots: Vec::new() }
    }

    pub fn push_root(&mut self, element: Element) {
        self.roots.push(element);
    }
}

#[derive(Clone, Debug)]
pub struct ElementBuilder {
    element: Element,
}

impl ElementBuilder {
    pub fn container(id: ElementId) -> Self {
        Self {
            element: Element::new(id),
        }
    }

    pub fn text(id: ElementId, text: impl Into<String>, style: TextStyle) -> Self {
        Self {
            element: Element::text(id, text, style),
        }
    }

    pub fn kind(mut self, kind: ElementKind) -> Self {
        self.element.kind = kind;
        self
    }

    pub fn style(mut self, style: ElementStyle) -> Self {
        self.element.style = style;
        self
    }

    pub fn layout(mut self, layout: LayoutInput) -> Self {
        self.element.layout = layout;
        self
    }

    pub fn child(mut self, child: Element) -> Self {
        self.element.children.push(child);
        self
    }

    pub fn build(self) -> Element {
        self.element
    }
}
