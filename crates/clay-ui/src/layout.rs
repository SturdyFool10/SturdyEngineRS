use std::collections::HashMap;

use glam::Vec2;

use crate::{Axis, Edges, Element, ElementId, ElementKind, Rect, Size, TextStyle, TextWrap};

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
) -> Result<Size, LayoutError> {
    if !seen.insert(element.id.hash) {
        return Err(LayoutError::DuplicateId(element.id.hash));
    }

    validate_sizing(element.layout.width)?;
    validate_sizing(element.layout.height)?;

    let available = containing.inset(element.style.padding);
    let child_sizes = measure_children(element, available.size, cache, text_measurer, tree, seen)?;
    let content_size = stack_size(element.layout.direction, element.layout.gap, &child_sizes);
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
    text_measurer: &mut impl FnMut(&ElementId, &str, &TextStyle, Option<f32>) -> Size,
    tree: &mut LayoutTree,
    seen: &mut std::collections::HashSet<u64>,
) -> Result<Vec<Size>, LayoutError> {
    if let ElementKind::Text(text) = &element.kind {
        return Ok(vec![cache.cached_measured_text_size(
            &element.id,
            &text.text,
            &text.style,
            available.width,
            text_measurer,
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
        sizes.push(layout_element(
            child,
            child_containing,
            cache,
            text_measurer,
            tree,
            seen,
        )?);
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
mod tests {
    use super::*;

    #[test]
    fn text_layout_uses_provided_measurer() {
        let id = ElementId::new("measured-text");
        let mut element = Element::text(
            id.clone(),
            "this long text should receive a constrained measurement width",
            TextStyle::default(),
        );
        element.layout.width = LayoutSizing::Fit {
            min: 0.0,
            max: f32::INFINITY,
        };
        element.layout.height = LayoutSizing::Fit {
            min: 0.0,
            max: f32::INFINITY,
        };

        let mut cache = LayoutCache::default();
        let mut calls = 0usize;
        let mut measurer = |_id: &ElementId, text: &str, _style: &TextStyle, width: Option<f32>| {
            calls += 1;
            assert!(text.starts_with("this long text"));
            assert_eq!(width, Some(320.0));
            Size::new(123.0, 45.0)
        };

        let layout = LayoutTree::compute_with_text_measurer(
            &element,
            Size::new(320.0, 200.0),
            &mut cache,
            &mut measurer,
        )
        .unwrap();

        assert_eq!(calls, 1);
        assert_eq!(layout.by_id(&id).unwrap().rect.size, Size::new(123.0, 45.0));
        assert_eq!(
            cache.text_stats(),
            LayoutTextCacheStats { hits: 0, misses: 1 }
        );
    }

    #[test]
    fn text_layout_measurement_is_cached() {
        let element = Element::text(ElementId::new("cached-text"), "hello", TextStyle::default());
        let mut cache = LayoutCache::default();
        let mut calls = 0usize;
        let mut measurer =
            |_id: &ElementId, _text: &str, _style: &TextStyle, _width: Option<f32>| {
                calls += 1;
                Size::new(80.0, 20.0)
            };

        LayoutTree::compute_with_text_measurer(
            &element,
            Size::new(320.0, 200.0),
            &mut cache,
            &mut measurer,
        )
        .unwrap();
        cache.reset_text_stats();
        LayoutTree::compute_with_text_measurer(
            &element,
            Size::new(320.0, 200.0),
            &mut cache,
            &mut measurer,
        )
        .unwrap();

        assert_eq!(calls, 1);
        assert_eq!(
            cache.text_stats(),
            LayoutTextCacheStats { hits: 1, misses: 0 }
        );
    }

    #[test]
    fn wrapped_text_uses_exact_width_for_correct_reservation() {
        let element = Element::text(
            ElementId::new("resize-text"),
            "this long text should be width-dependent when narrow",
            TextStyle::default(),
        );
        let mut cache = LayoutCache::default();
        let mut calls = 0usize;
        let mut measured_widths = Vec::new();
        let mut measurer =
            |_id: &ElementId, _text: &str, _style: &TextStyle, width: Option<f32>| {
                calls += 1;
                measured_widths.push(width);
                Size::new(80.0, 20.0)
            };

        LayoutTree::compute_with_text_measurer(
            &element,
            Size::new(335.0, 200.0),
            &mut cache,
            &mut measurer,
        )
        .unwrap();
        LayoutTree::compute_with_text_measurer(
            &element,
            Size::new(330.0, 200.0),
            &mut cache,
            &mut measurer,
        )
        .unwrap();

        assert_eq!(calls, 2);
        assert_eq!(measured_widths, vec![Some(335.0), Some(330.0)]);
    }

    #[test]
    fn nowrap_labels_ignore_width_for_resize_cache_stability() {
        let mut style = TextStyle::default();
        style.wrap = TextWrap::None;
        let element = Element::text(ElementId::new("wide-label"), "hello", style);
        let mut cache = LayoutCache::default();
        let mut calls = 0usize;
        let mut measured_widths = Vec::new();
        let mut measurer =
            |_id: &ElementId, _text: &str, _style: &TextStyle, width: Option<f32>| {
                calls += 1;
                measured_widths.push(width);
                Size::new(80.0, 20.0)
            };

        LayoutTree::compute_with_text_measurer(
            &element,
            Size::new(640.0, 200.0),
            &mut cache,
            &mut measurer,
        )
        .unwrap();
        LayoutTree::compute_with_text_measurer(
            &element,
            Size::new(420.0, 200.0),
            &mut cache,
            &mut measurer,
        )
        .unwrap();

        assert_eq!(calls, 1);
        assert_eq!(measured_widths, vec![None]);
    }

    #[test]
    fn fit_container_reserves_child_text_plus_padding() {
        let child_id = ElementId::new("badge-text");
        let mut badge = Element::new(ElementId::new("badge"));
        badge.style.padding = Edges::symmetric(12.0, 8.0);
        badge.layout.width = LayoutSizing::Fit {
            min: 0.0,
            max: f32::INFINITY,
        };
        badge.layout.height = LayoutSizing::Fit {
            min: 0.0,
            max: f32::INFINITY,
        };
        badge.children.push(Element::text(
            child_id.clone(),
            "Input Ready",
            TextStyle::default(),
        ));
        let mut cache = LayoutCache::default();
        let mut measurer =
            |_id: &ElementId, _text: &str, _style: &TextStyle, _width: Option<f32>| {
                Size::new(78.0, 18.0)
            };

        let layout = LayoutTree::compute_with_text_measurer(
            &badge,
            Size::new(320.0, 200.0),
            &mut cache,
            &mut measurer,
        )
        .unwrap();

        assert_eq!(
            layout.by_id(&badge.id).unwrap().rect.size,
            Size::new(102.0, 34.0)
        );
        assert_eq!(
            layout.by_id(&child_id).unwrap().rect.origin,
            Vec2::new(12.0, 8.0)
        );
    }
}

#[allow(dead_code)]
fn _padding_size(padding: Edges) -> Size {
    Size::new(padding.horizontal(), padding.vertical())
}
