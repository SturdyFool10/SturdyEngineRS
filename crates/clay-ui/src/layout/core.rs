use std::collections::HashMap;

use glam::Vec2;

use crate::{
    Axis, Edges, Element, ElementId, ElementKind, Rect, Size, TextStyle, TextWrap, UiShape,
};

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

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum UiLayer {
    Background,
    #[default]
    Content,
    Foreground,
    Overlay,
    TopLayer,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayoutPosition {
    Flow,
    Absolute { offset: Vec2 },
}

impl Default for LayoutPosition {
    fn default() -> Self {
        Self::Flow
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutInput {
    pub width: LayoutSizing,
    pub height: LayoutSizing,
    pub position: LayoutPosition,
    pub direction: LayoutDirection,
    pub gap: f32,
    pub align_x: Align,
    pub align_y: Align,
    pub clip_x: bool,
    pub clip_y: bool,
    pub scroll_offset: Vec2,
    pub layer: UiLayer,
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
            position: LayoutPosition::Flow,
            direction: LayoutDirection::TopToBottom,
            gap: 0.0,
            align_x: Align::Start,
            align_y: Align::Start,
            clip_x: false,
            clip_y: false,
            scroll_offset: Vec2::ZERO,
            layer: UiLayer::Content,
            z_index: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutOutput {
    pub id: ElementId,
    pub parent: u64,
    pub rect: Rect,
    pub content_size: Size,
    pub shape: UiShape,
    pub layer: UiLayer,
    pub z_index: i16,
    pub clip: bool,
    pub transparent_to_input: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LayoutError {
    DuplicateId(u64),
    PercentOutOfRange,
}

#[derive(Default)]
pub struct LayoutCache {
    text: HashMap<u64, Size>,
    text_stats: LayoutTextCacheStats,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LayoutTextCacheStats {
    pub hits: usize,
    pub misses: usize,
}

impl LayoutCache {
    pub fn clear_text(&mut self) {
        self.text.clear();
        self.text_stats = LayoutTextCacheStats::default();
    }

    pub fn text_stats(&self) -> LayoutTextCacheStats {
        self.text_stats
    }

    pub fn reset_text_stats(&mut self) {
        self.text_stats = LayoutTextCacheStats::default();
    }

    fn cached_measured_text_size<F>(
        &mut self,
        id: &ElementId,
        text: &str,
        style: &TextStyle,
        max_width: f32,
        text_measurer: &mut F,
    ) -> Size
    where
        F: FnMut(&ElementId, &str, &TextStyle, Option<f32>) -> Size,
    {
        let width = if style.wrap == TextWrap::Words {
            max_width.is_finite().then_some(max_width.max(1.0))
        } else {
            None
        };
        let key = style.cache_fingerprint(text, width, 1.0) ^ id.hash;
        if let Some(size) = self.text.get(&key) {
            self.text_stats.hits += 1;
            return *size;
        }

        self.text_stats.misses += 1;
        let size = text_measurer(id, text, style, width);
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
        let mut fallback_measurer =
            |_id: &ElementId, text: &str, style: &TextStyle, width: Option<f32>| {
                let estimated_width = text.chars().count() as f32 * style.font_size * 0.55;
                let width =
                    width.map_or(estimated_width, |max_width| max_width.min(estimated_width));
                estimated_text_size(text, style, width)
            };
        Self::compute_with_text_measurer(root, viewport, cache, &mut fallback_measurer)
    }

    pub fn compute_with_text_measurer<F>(
        root: &Element,
        viewport: Size,
        cache: &mut LayoutCache,
        text_measurer: &mut F,
    ) -> Result<Self, LayoutError>
    where
        F: FnMut(&ElementId, &str, &TextStyle, Option<f32>) -> Size,
    {
        let mut tree = Self { nodes: Vec::new() };
        let mut seen = std::collections::HashSet::new();
        layout_element(
            root,
            Rect::new(0.0, 0.0, viewport.width, viewport.height),
            cache,
            text_measurer,
            &mut tree,
            &mut seen,
            0,
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
    text_measurer: &mut impl FnMut(&ElementId, &str, &TextStyle, Option<f32>) -> Size,
    tree: &mut LayoutTree,
    seen: &mut std::collections::HashSet<u64>,
    parent: u64,
) -> Result<Size, LayoutError> {
    if !seen.insert(element.id.hash) {
        return Err(LayoutError::DuplicateId(element.id.hash));
    }

    validate_sizing(element.layout.width)?;
    validate_sizing(element.layout.height)?;

    let available = containing.inset(element.style.padding);
    let child_measurements =
        measure_children(element, available.size, cache, text_measurer, tree, seen)?;
    let content_size = stack_size(
        element.layout.direction,
        element.layout.gap,
        &flow_child_sizes(&child_measurements),
    );
    let padded_content_size = Size::new(
        content_size.width + element.style.padding.horizontal(),
        content_size.height + element.style.padding.vertical(),
    );
    let own_size = Size::new(
        resolve_axis(
            element.layout.width,
            padded_content_size.width,
            containing.size.width,
        ),
        resolve_axis(
            element.layout.height,
            padded_content_size.height,
            containing.size.height,
        ),
    );
    let rect = Rect {
        origin: containing.origin,
        size: own_size,
    };
    tree.nodes.push(LayoutOutput {
        id: element.id.clone(),
        parent,
        rect,
        content_size,
        shape: element.style.resolved_shape(),
        layer: element.layout.layer,
        z_index: element.layout.z_index,
        clip: element.layout.clip_x || element.layout.clip_y,
        transparent_to_input: element.style.transparent_to_input,
    });
    position_children(
        element,
        rect.inset(element.style.padding),
        &child_measurements,
        tree,
    );
    Ok(own_size)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MeasuredChild {
    size: Size,
    position: LayoutPosition,
}

fn measure_children(
    element: &Element,
    available: Size,
    cache: &mut LayoutCache,
    text_measurer: &mut impl FnMut(&ElementId, &str, &TextStyle, Option<f32>) -> Size,
    tree: &mut LayoutTree,
    seen: &mut std::collections::HashSet<u64>,
) -> Result<Vec<MeasuredChild>, LayoutError> {
    if let ElementKind::Text(text) = &element.kind {
        return Ok(vec![MeasuredChild {
            size: cache.cached_measured_text_size(
                &element.id,
                &text.text,
                &text.style,
                available.width,
                text_measurer,
            ),
            position: LayoutPosition::Flow,
        }]);
    }
    if let ElementKind::Image(image) = &element.kind {
        return Ok(vec![MeasuredChild {
            size: image.natural_size.unwrap_or(Size::ZERO),
            position: LayoutPosition::Flow,
        }]);
    }

    let mut measurements = Vec::with_capacity(element.children.len());
    for child in &element.children {
        let child_containing = Rect {
            origin: Vec2::ZERO,
            size: available,
        };
        measurements.push(MeasuredChild {
            size: layout_element(
                child,
                child_containing,
                cache,
                text_measurer,
                tree,
                seen,
                element.id.hash,
            )?,
            position: child.layout.position,
        });
    }
    Ok(measurements)
}

fn flow_child_sizes(children: &[MeasuredChild]) -> Vec<Size> {
    children
        .iter()
        .filter_map(|child| matches!(child.position, LayoutPosition::Flow).then_some(child.size))
        .collect()
}

fn position_children(
    element: &Element,
    content_rect: Rect,
    child_measurements: &[MeasuredChild],
    tree: &mut LayoutTree,
) {
    let mut cursor = content_rect.origin + element.layout.scroll_offset;
    for (child, measurement) in element.children.iter().zip(child_measurements) {
        let target_origin = match measurement.position {
            LayoutPosition::Flow => {
                let cross_offset = align_offset(
                    element.layout.direction,
                    element.layout,
                    measurement.size,
                    content_rect.size,
                );
                cursor + cross_offset
            }
            LayoutPosition::Absolute { offset } => {
                content_rect.origin + element.layout.scroll_offset + offset
            }
        };

        if let Some(current_origin) = tree
            .nodes
            .iter()
            .find(|node| node.id.hash == child.id.hash)
            .map(|node| node.rect.origin)
        {
            translate_subtree(child, target_origin - current_origin, tree);
        }

        if matches!(measurement.position, LayoutPosition::Flow) {
            match axis_for_direction(element.layout.direction) {
                Axis::Horizontal => cursor.x += measurement.size.width + element.layout.gap,
                Axis::Vertical => cursor.y += measurement.size.height + element.layout.gap,
            }
        }
    }
}

fn translate_subtree(element: &Element, delta: Vec2, tree: &mut LayoutTree) {
    if delta == Vec2::ZERO {
        return;
    }

    if let Some(node) = tree
        .nodes
        .iter_mut()
        .find(|node| node.id.hash == element.id.hash)
    {
        node.rect.origin += delta;
    }
    for child in &element.children {
        translate_subtree(child, delta, tree);
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

fn estimated_text_size(_text: &str, style: &TextStyle, width: f32) -> Size {
    Size::new(width, style.line_height.max(style.font_size))
}

fn axis_for_direction(direction: LayoutDirection) -> Axis {
    match direction {
        LayoutDirection::LeftToRight => Axis::Horizontal,
        LayoutDirection::TopToBottom => Axis::Vertical,
    }
}

#[cfg(test)]
#[path = "core_tests.rs"]
mod tests;

#[allow(dead_code)]
fn _padding_size(padding: Edges) -> Size {
    Size::new(padding.horizontal(), padding.vertical())
}
