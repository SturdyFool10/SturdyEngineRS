use std::collections::HashMap;

use glam::Vec2;

use crate::{Axis, Edges, Element, ElementId, ElementKind, Rect, Size};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayoutDirection {
    LeftToRight,
    TopToBottom,
}

impl Default for LayoutDirection {
    fn default() -> Self {
        Self::TopToBottom
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Align {
    Start,
    Center,
    End,
}

impl Default for Align {
    fn default() -> Self {
        Self::Start
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayoutSizing {
    Fit { min: f32, max: f32 },
    Grow { min: f32, max: f32 },
    Percent(f32),
    Fixed(f32),
}

impl Default for LayoutSizing {
    fn default() -> Self {
        Self::Fit {
            min: 0.0,
            max: f32::INFINITY,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutInput {
    pub width: LayoutSizing,
    pub height: LayoutSizing,
    pub direction: LayoutDirection,
    pub gap: f32,
    pub align_x: Align,
    pub align_y: Align,
    pub clip_x: bool,
    pub clip_y: bool,
    pub scroll_offset: Vec2,
    pub z_index: i16,
}

impl Default for LayoutInput {
    fn default() -> Self {
        Self {
            width: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            direction: LayoutDirection::TopToBottom,
            gap: 0.0,
            align_x: Align::Start,
            align_y: Align::Start,
            clip_x: false,
            clip_y: false,
            scroll_offset: Vec2::ZERO,
            z_index: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutOutput {
    pub id: ElementId,
    pub rect: Rect,
    pub content_size: Size,
    pub z_index: i16,
    pub clip: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LayoutError {
    DuplicateId(u64),
    PercentOutOfRange,
}

#[derive(Default)]
pub struct LayoutCache {
    text: HashMap<(u64, u32, u32), Size>,
}

impl LayoutCache {
    pub fn clear_text(&mut self) {
        self.text.clear();
    }

    fn cached_text_size(&mut self, id: u64, text: &str, font_size: f32, max_width: f32) -> Size {
        let key = (id, text.len() as u32, font_size.to_bits());
        if let Some(size) = self.text.get(&key) {
            return *size;
        }
        let width = if max_width.is_finite() {
            max_width.min(text.chars().count() as f32 * font_size * 0.55)
        } else {
            text.chars().count() as f32 * font_size * 0.55
        };
        let line_height = font_size * 1.35;
        let size = Size::new(width, line_height);
        self.text.insert(key, size);
        size
    }
}

#[derive(Default)]
pub struct LayoutTree {
    pub nodes: Vec<LayoutOutput>,
}

impl LayoutTree {
    pub fn compute(
        root: &Element,
        viewport: Size,
        cache: &mut LayoutCache,
    ) -> Result<Self, LayoutError> {
        let mut tree = Self { nodes: Vec::new() };
        let mut seen = std::collections::HashSet::new();
        layout_element(
            root,
            Rect::new(0.0, 0.0, viewport.width, viewport.height),
            cache,
            &mut tree,
            &mut seen,
        )?;
        Ok(tree)
    }

    pub fn by_id(&self, id: &ElementId) -> Option<&LayoutOutput> {
        self.nodes.iter().find(|node| node.id.hash == id.hash)
    }
}

fn layout_element(
    element: &Element,
    containing: Rect,
    cache: &mut LayoutCache,
    tree: &mut LayoutTree,
    seen: &mut std::collections::HashSet<u64>,
) -> Result<Size, LayoutError> {
    if !seen.insert(element.id.hash) {
        return Err(LayoutError::DuplicateId(element.id.hash));
    }

    validate_sizing(element.layout.width)?;
    validate_sizing(element.layout.height)?;

    let available = containing.inset(element.style.padding);
    let child_sizes = measure_children(element, available.size, cache, tree, seen)?;
    let content_size = stack_size(element.layout.direction, element.layout.gap, &child_sizes);
    let own_size = Size::new(
        resolve_axis(
            element.layout.width,
            content_size.width,
            containing.size.width,
        ),
        resolve_axis(
            element.layout.height,
            content_size.height,
            containing.size.height,
        ),
    );
    let rect = Rect {
        origin: containing.origin,
        size: own_size,
    };
    tree.nodes.push(LayoutOutput {
        id: element.id.clone(),
        rect,
        content_size,
        z_index: element.layout.z_index,
        clip: element.layout.clip_x || element.layout.clip_y,
    });
    position_children(
        element,
        rect.inset(element.style.padding),
        &child_sizes,
        tree,
    );
    Ok(own_size)
}

fn measure_children(
    element: &Element,
    available: Size,
    cache: &mut LayoutCache,
    tree: &mut LayoutTree,
    seen: &mut std::collections::HashSet<u64>,
) -> Result<Vec<Size>, LayoutError> {
    if let ElementKind::Text(text) = &element.kind {
        return Ok(vec![cache.cached_text_size(
            element.id.hash,
            &text.text,
            text.style.font_size,
            available.width,
        )]);
    }
    if let ElementKind::Image(image) = &element.kind {
        return Ok(vec![image.natural_size.unwrap_or(Size::ZERO)]);
    }

    let mut sizes = Vec::with_capacity(element.children.len());
    for child in &element.children {
        let child_containing = Rect {
            origin: Vec2::ZERO,
            size: available,
        };
        sizes.push(layout_element(child, child_containing, cache, tree, seen)?);
    }
    Ok(sizes)
}

fn position_children(
    element: &Element,
    content_rect: Rect,
    child_sizes: &[Size],
    tree: &mut LayoutTree,
) {
    let mut cursor = content_rect.origin + element.layout.scroll_offset;
    for (child, size) in element.children.iter().zip(child_sizes) {
        if let Some(node) = tree
            .nodes
            .iter_mut()
            .find(|node| node.id.hash == child.id.hash)
        {
            let cross_offset = align_offset(
                element.layout.direction,
                element.layout,
                *size,
                content_rect.size,
            );
            node.rect.origin = cursor + cross_offset;
        }
        match axis_for_direction(element.layout.direction) {
            Axis::Horizontal => cursor.x += size.width + element.layout.gap,
            Axis::Vertical => cursor.y += size.height + element.layout.gap,
        }
    }
}

fn align_offset(
    direction: LayoutDirection,
    input: LayoutInput,
    size: Size,
    available: Size,
) -> Vec2 {
    match direction {
        LayoutDirection::LeftToRight => Vec2::new(
            0.0,
            align_value(input.align_y, size.height, available.height),
        ),
        LayoutDirection::TopToBottom => {
            Vec2::new(align_value(input.align_x, size.width, available.width), 0.0)
        }
    }
}

fn align_value(align: Align, child: f32, parent: f32) -> f32 {
    match align {
        Align::Start => 0.0,
        Align::Center => ((parent - child) * 0.5).max(0.0),
        Align::End => (parent - child).max(0.0),
    }
}

fn stack_size(direction: LayoutDirection, gap: f32, sizes: &[Size]) -> Size {
    if sizes.is_empty() {
        return Size::ZERO;
    }
    let gap_total = gap * sizes.len().saturating_sub(1) as f32;
    match direction {
        LayoutDirection::LeftToRight => Size::new(
            sizes.iter().map(|size| size.width).sum::<f32>() + gap_total,
            sizes.iter().map(|size| size.height).fold(0.0, f32::max),
        ),
        LayoutDirection::TopToBottom => Size::new(
            sizes.iter().map(|size| size.width).fold(0.0, f32::max),
            sizes.iter().map(|size| size.height).sum::<f32>() + gap_total,
        ),
    }
}

fn resolve_axis(sizing: LayoutSizing, content: f32, parent: f32) -> f32 {
    match sizing {
        LayoutSizing::Fit { min, max } => content.clamp(min, max),
        LayoutSizing::Grow { min, max } => parent.clamp(min, max),
        LayoutSizing::Percent(percent) => parent * percent.clamp(0.0, 1.0),
        LayoutSizing::Fixed(size) => size,
    }
}

fn validate_sizing(sizing: LayoutSizing) -> Result<(), LayoutError> {
    if let LayoutSizing::Percent(percent) = sizing
        && !(0.0..=1.0).contains(&percent)
    {
        return Err(LayoutError::PercentOutOfRange);
    }
    Ok(())
}

fn axis_for_direction(direction: LayoutDirection) -> Axis {
    match direction {
        LayoutDirection::LeftToRight => Axis::Horizontal,
        LayoutDirection::TopToBottom => Axis::Vertical,
    }
}

#[allow(dead_code)]
fn _padding_size(padding: Edges) -> Size {
    Size::new(padding.horizontal(), padding.vertical())
}
