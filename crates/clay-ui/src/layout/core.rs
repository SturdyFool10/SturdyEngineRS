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
    fn layout_preserves_resolved_shape() {
        let id = ElementId::new("shaped");
        let mut element = Element::new(id.clone());
        element.layout.width = LayoutSizing::Fixed(100.0);
        element.layout.height = LayoutSizing::Fixed(40.0);
        element.style.corner_radius = crate::radii_all(12.0);

        let layout = LayoutTree::compute(
            &element,
            Size::new(100.0, 40.0),
            &mut LayoutCache::default(),
        )
        .unwrap();

        assert_eq!(
            layout.by_id(&id).unwrap().shape,
            UiShape::rounded_rect(crate::radii_all(12.0))
        );
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

    #[test]
    fn positioned_children_do_not_contribute_to_parent_fit_size() {
        let flow_id = ElementId::new("flow");
        let absolute_id = ElementId::new("absolute");
        let mut root = Element::new(ElementId::new("root"));
        root.layout.width = LayoutSizing::Fit {
            min: 0.0,
            max: f32::INFINITY,
        };
        root.layout.height = LayoutSizing::Fit {
            min: 0.0,
            max: f32::INFINITY,
        };

        let mut flow = Element::new(flow_id);
        flow.layout.width = LayoutSizing::Fixed(40.0);
        flow.layout.height = LayoutSizing::Fixed(20.0);

        let mut absolute = Element::new(absolute_id.clone());
        absolute.layout.width = LayoutSizing::Fixed(100.0);
        absolute.layout.height = LayoutSizing::Fixed(40.0);
        absolute.layout.position = LayoutPosition::Absolute {
            offset: Vec2::new(12.0, 16.0),
        };

        root.children.push(flow);
        root.children.push(absolute);

        let layout =
            LayoutTree::compute(&root, Size::new(300.0, 200.0), &mut LayoutCache::default())
                .unwrap();

        assert_eq!(
            layout.by_id(&root.id).unwrap().rect.size,
            Size::new(40.0, 20.0)
        );
        assert_eq!(
            layout.by_id(&absolute_id).unwrap().rect.origin,
            Vec2::new(12.0, 16.0)
        );
    }

    #[test]
    fn child_translation_moves_descendants_with_parent() {
        let child_id = ElementId::new("child");
        let grandchild_id = ElementId::new("grandchild");
        let mut root = Element::new(ElementId::new("root"));
        root.style.padding = Edges::all(10.0);
        root.layout.width = LayoutSizing::Fixed(200.0);
        root.layout.height = LayoutSizing::Fixed(100.0);

        let mut child = Element::new(child_id.clone());
        child.layout.width = LayoutSizing::Fixed(80.0);
        child.layout.height = LayoutSizing::Fixed(40.0);
        child.style.padding = Edges::all(5.0);

        let mut grandchild = Element::new(grandchild_id.clone());
        grandchild.layout.width = LayoutSizing::Fixed(20.0);
        grandchild.layout.height = LayoutSizing::Fixed(10.0);
        child.children.push(grandchild);
        root.children.push(child);

        let layout =
            LayoutTree::compute(&root, Size::new(200.0, 100.0), &mut LayoutCache::default())
                .unwrap();

        assert_eq!(
            layout.by_id(&child_id).unwrap().rect.origin,
            Vec2::new(10.0, 10.0)
        );
        assert_eq!(
            layout.by_id(&grandchild_id).unwrap().rect.origin,
            Vec2::new(15.0, 15.0)
        );
    }

    #[test]
    fn layout_preserves_declared_layer() {
        let mut element = Element::new(ElementId::new("modal"));
        element.layout.layer = UiLayer::TopLayer;
        element.layout.width = LayoutSizing::Fixed(100.0);
        element.layout.height = LayoutSizing::Fixed(40.0);
        let mut cache = LayoutCache::default();

        let layout = LayoutTree::compute(&element, Size::new(320.0, 200.0), &mut cache).unwrap();

        assert_eq!(layout.by_id(&element.id).unwrap().layer, UiLayer::TopLayer);
    }

    #[test]
    fn layout_preserves_input_transparency() {
        let mut element = Element::new(ElementId::new("portal-host"));
        element.style.transparent_to_input = true;
        element.layout.width = LayoutSizing::Fixed(100.0);
        element.layout.height = LayoutSizing::Fixed(40.0);
        let mut cache = LayoutCache::default();

        let layout = LayoutTree::compute(&element, Size::new(320.0, 200.0), &mut cache).unwrap();

        assert!(layout.by_id(&element.id).unwrap().transparent_to_input);
    }

    #[test]
    fn layout_records_actual_parent_relationship() {
        let child_id = ElementId::new("app-owned-child-id");
        let mut root = Element::new(ElementId::new("root"));
        root.layout.width = LayoutSizing::Fixed(100.0);
        root.layout.height = LayoutSizing::Fixed(40.0);
        root.children.push(Element::new(child_id.clone()));
        let mut cache = LayoutCache::default();

        let layout = LayoutTree::compute(&root, Size::new(100.0, 40.0), &mut cache).unwrap();

        assert_eq!(layout.by_id(&root.id).unwrap().parent, 0);
        assert_eq!(layout.by_id(&child_id).unwrap().parent, root.id.hash);
    }
}

#[allow(dead_code)]
fn _padding_size(padding: Edges) -> Size {
    Size::new(padding.horizontal(), padding.vertical())
}
