// Tests extracted from crates/clay-ui/src/layout/floating.rs
// Runtime code should stay separate from test code.

use super::*;
use crate::{Element, LayoutCache};

#[test]
fn floating_layout_places_bottom_start_from_anchor() {
    let anchor = Rect::new(50.0, 20.0, 80.0, 24.0);
    let layout = FloatingLayout::compute(
        anchor,
        Size::new(100.0, 60.0),
        Size::new(400.0, 300.0),
        FloatingOptions::default().offset(4.0),
    );

    assert_eq!(
        layout.placement,
        FloatingPlacement::bottom(FloatingAlign::Start)
    );
    assert_eq!(layout.rect, Rect::new(50.0, 48.0, 100.0, 60.0));
    assert!(!layout.flipped);
    assert!(!layout.clamped);
}

#[test]
fn floating_layout_flips_when_primary_side_overflows() {
    let anchor = Rect::new(50.0, 260.0, 80.0, 24.0);
    let layout = FloatingLayout::compute(
        anchor,
        Size::new(120.0, 80.0),
        Size::new(300.0, 300.0),
        FloatingOptions::default().offset(4.0).viewport_margin(8.0),
    );

    assert_eq!(
        layout.requested_placement,
        FloatingPlacement::bottom(FloatingAlign::Start)
    );
    assert_eq!(
        layout.placement,
        FloatingPlacement::top(FloatingAlign::Start)
    );
    assert_eq!(layout.rect, Rect::new(50.0, 176.0, 120.0, 80.0));
    assert!(layout.flipped);
    assert!(!layout.clamped);
}

#[test]
fn floating_layout_clamps_secondary_axis_inside_viewport() {
    let anchor = Rect::new(250.0, 40.0, 40.0, 20.0);
    let layout = FloatingLayout::compute(
        anchor,
        Size::new(100.0, 60.0),
        Size::new(300.0, 240.0),
        FloatingOptions::default()
            .offset(4.0)
            .viewport_margin(8.0)
            .collision(FloatingCollision::Clamp),
    );

    assert_eq!(layout.rect, Rect::new(192.0, 64.0, 100.0, 60.0));
    assert!(layout.clamped);
    assert!(!layout.flipped);
}

#[test]
fn floating_layout_can_match_anchor_width_and_constrain_size() {
    let anchor = Rect::new(16.0, 20.0, 96.0, 20.0);
    let layout = FloatingLayout::compute(
        anchor,
        Size::new(260.0, 400.0),
        Size::new(160.0, 180.0),
        FloatingOptions::default()
            .match_anchor_width(true)
            .viewport_margin(10.0),
    );

    assert_eq!(layout.rect.size, Size::new(96.0, 160.0));
    assert!(layout.constrained);
}

#[test]
fn anchored_floating_layer_builds_absolute_top_layer_content() {
    let id = ElementId::new("floating-host");
    let mut content = Element::new(ElementId::new("menu"));
    content.layout.z_index = 2;
    let element = anchored_floating_layer(
        id,
        FloatingLayerConfig::new(
            Size::new(300.0, 200.0),
            Rect::new(20.0, 30.0, 80.0, 20.0),
            Size::new(120.0, 90.0),
        )
        .z_index(40),
        content,
    );

    assert_eq!(element.layout.layer, UiLayer::TopLayer);
    assert_eq!(element.layout.z_index, 40);
    assert!(element.style.transparent_to_input);
    assert_eq!(element.children.len(), 1);
    assert_eq!(element.children[0].layout.layer, UiLayer::TopLayer);
    assert_eq!(element.children[0].layout.z_index, 43);
    assert_eq!(
        element.children[0].layout.position,
        LayoutPosition::Absolute {
            offset: glam::Vec2::new(20.0, 54.0)
        }
    );
    assert_eq!(element.children[0].layout.width, LayoutSizing::Fixed(120.0));
    assert_eq!(element.children[0].layout.height, LayoutSizing::Fixed(90.0));
}

#[test]
fn attach_config_resolves_anchor_from_layout_tree() {
    let root_id = ElementId::new("root");
    let anchor_id = ElementId::new("app-owned-anchor");
    let mut root = Element::new(root_id);
    root.style.padding = crate::Edges::all(10.0);
    root.layout.width = LayoutSizing::Fixed(240.0);
    root.layout.height = LayoutSizing::Fixed(120.0);
    let mut anchor = Element::new(anchor_id.clone());
    anchor.layout.width = LayoutSizing::Fixed(80.0);
    anchor.layout.height = LayoutSizing::Fixed(24.0);
    root.children.push(anchor);
    let layout =
        LayoutTree::compute(&root, Size::new(240.0, 120.0), &mut LayoutCache::default())
            .unwrap();
    let config =
        FloatingAttachConfig::new(Size::new(240.0, 120.0), anchor_id, Size::new(120.0, 40.0))
            .z_index(9)
            .transparent_to_input(false);

    let layer_config = config.layer_config(&layout).unwrap();

    assert_eq!(layer_config.anchor, Rect::new(10.0, 10.0, 80.0, 24.0));
    assert_eq!(layer_config.z_index, 9);
    assert!(!layer_config.transparent_to_input);
}

#[test]
fn attach_config_reports_missing_anchor() {
    let anchor_id = ElementId::new("missing-anchor");
    let config = FloatingAttachConfig::new(
        Size::new(240.0, 120.0),
        anchor_id.clone(),
        Size::new(120.0, 40.0),
    );

    let error = config.layout(&LayoutTree::default()).unwrap_err();

    assert_eq!(error, FloatingAttachError::AnchorNotFound(anchor_id));
}

#[test]
fn attached_floating_layer_positions_from_layout_anchor() {
    let root_id = ElementId::new("root");
    let anchor_id = ElementId::new("button");
    let mut root = Element::new(root_id);
    root.style.padding = crate::Edges::all(12.0);
    root.layout.width = LayoutSizing::Fixed(300.0);
    root.layout.height = LayoutSizing::Fixed(180.0);
    let mut anchor = Element::new(anchor_id.clone());
    anchor.layout.width = LayoutSizing::Fixed(96.0);
    anchor.layout.height = LayoutSizing::Fixed(20.0);
    root.children.push(anchor);
    let layout =
        LayoutTree::compute(&root, Size::new(300.0, 180.0), &mut LayoutCache::default())
            .unwrap();
    let content = Element::new(ElementId::new("menu"));
    let config =
        FloatingAttachConfig::new(Size::new(300.0, 180.0), anchor_id, Size::new(160.0, 90.0))
            .options(FloatingOptions::default().offset(6.0))
            .z_index(30);

    let element =
        attached_floating_layer(ElementId::new("floating-host"), &layout, &config, content)
            .unwrap();

    assert_eq!(element.layout.layer, UiLayer::TopLayer);
    assert_eq!(element.layout.z_index, 30);
    assert_eq!(
        element.children[0].layout.position,
        LayoutPosition::Absolute {
            offset: glam::Vec2::new(12.0, 38.0)
        }
    );
    assert_eq!(element.children[0].layout.width, LayoutSizing::Fixed(160.0));
    assert_eq!(element.children[0].layout.height, LayoutSizing::Fixed(90.0));
}
