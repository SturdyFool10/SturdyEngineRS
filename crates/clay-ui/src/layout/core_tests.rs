// Tests extracted from crates/clay-ui/src/layout/core.rs
// Runtime code should stay separate from test code.

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
    let mut measurer = |_id: &ElementId, _text: &str, _style: &TextStyle, _width: Option<f32>| {
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
    let mut measurer = |_id: &ElementId, _text: &str, _style: &TextStyle, width: Option<f32>| {
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
    let mut measurer = |_id: &ElementId, _text: &str, _style: &TextStyle, width: Option<f32>| {
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
    let mut measurer = |_id: &ElementId, _text: &str, _style: &TextStyle, _width: Option<f32>| {
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
        LayoutTree::compute(&root, Size::new(300.0, 200.0), &mut LayoutCache::default()).unwrap();

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
        LayoutTree::compute(&root, Size::new(200.0, 100.0), &mut LayoutCache::default()).unwrap();

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
