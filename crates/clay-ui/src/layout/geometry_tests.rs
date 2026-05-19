// Tests extracted from crates/clay-ui/src/layout/geometry.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn rect_shape_uses_legacy_corner_radius_as_fallback() {
    let shape = UiShape::Rect.with_corner_radius_fallback(radii_all(8.0));

    assert_eq!(
        shape,
        UiShape::RoundedRect {
            radii: radii_all(8.0)
        }
    );
}

#[test]
fn explicit_shape_ignores_legacy_corner_radius_fallback() {
    let shape = UiShape::squircle(12.0, 4.0).with_corner_radius_fallback(radii_all(8.0));

    assert_eq!(shape, UiShape::squircle(12.0, 4.0));
}

#[test]
fn rect_edges_are_origin_plus_size_with_exclusive_max() {
    let rect = Rect::new(10.0, 20.0, 30.0, 40.0);

    assert_eq!(rect.min(), Vec2::new(10.0, 20.0));
    assert_eq!(rect.max_exclusive(), Vec2::new(40.0, 60.0));
    assert_eq!(rect.right(), 40.0);
    assert_eq!(rect.bottom(), 60.0);
    assert_eq!(rect.center(), Vec2::new(25.0, 40.0));
}

#[test]
fn rect_contains_excludes_bottom_right_edges() {
    let rect = Rect::new(0.0, 0.0, 100.0, 40.0);

    assert!(rect.contains(Vec2::new(0.0, 0.0)));
    assert!(rect.contains(Vec2::new(99.0, 39.0)));
    assert!(!rect.contains(Vec2::new(100.0, 39.0)));
    assert!(!rect.contains(Vec2::new(99.0, 40.0)));
    assert!(!rect.contains(Vec2::new(100.0, 40.0)));
}

#[test]
fn rounded_rect_hit_testing_excludes_rounded_corners() {
    let rect = Rect::new(0.0, 0.0, 100.0, 40.0);
    let shape = UiShape::rounded_rect(radii_all(20.0));

    assert!(!shape.contains_point(rect, Vec2::new(1.0, 1.0)));
    assert!(shape.contains_point(rect, Vec2::new(20.0, 20.0)));
    assert!(shape.contains_point(rect, Vec2::new(50.0, 1.0)));
}

#[test]
fn independent_corner_shapes_affect_hit_testing() {
    let rect = Rect::new(0.0, 0.0, 100.0, 40.0);
    let shape = UiShape::independent_corners(
        CornerSpec::with_shape(20.0, CornerShape::Bevel),
        CornerSpec::round(0.0),
        CornerSpec::round(0.0),
        CornerSpec::round(0.0),
    );

    assert!(!shape.contains_point(rect, Vec2::new(4.0, 4.0)));
    assert!(shape.contains_point(rect, Vec2::new(16.0, 8.0)));
}

#[test]
fn circle_and_ellipse_hit_testing_use_actual_coverage() {
    let rect = Rect::new(0.0, 0.0, 100.0, 40.0);

    assert!(!UiShape::Circle.contains_point(rect, Vec2::new(1.0, 1.0)));
    assert!(UiShape::Circle.contains_point(rect, Vec2::new(50.0, 20.0)));
    assert!(!UiShape::Ellipse.contains_point(rect, Vec2::new(1.0, 1.0)));
    assert!(UiShape::Ellipse.contains_point(rect, Vec2::new(50.0, 1.0)));
}

// ── Edge-inclusive / exclusive rectangle contract ─────────────────────────

#[test]
fn rect_contains_includes_top_left_edge() {
    // The left and top edges are inclusive: a point exactly on the origin
    // of the rect must be inside it.
    let rect = Rect::new(10.0, 20.0, 50.0, 30.0);

    assert!(rect.contains(Vec2::new(10.0, 20.0)), "origin is inside");
    assert!(rect.contains(Vec2::new(10.0, 49.0)), "left edge, last row");
    assert!(
        rect.contains(Vec2::new(59.0, 20.0)),
        "last column, top edge"
    );

    // Points just outside the left or top edge are excluded.
    assert!(!rect.contains(Vec2::new(9.9, 20.0)), "left of left edge");
    assert!(!rect.contains(Vec2::new(10.0, 19.9)), "above top edge");
}

#[test]
fn rect_contains_with_non_zero_origin() {
    // Verify that the inclusive-min / exclusive-max contract holds when the
    // rectangle is not anchored at (0, 0).
    let rect = Rect::new(5.0, 8.0, 10.0, 6.0);
    // max_exclusive = (15, 14)

    assert!(
        rect.contains(Vec2::new(5.0, 8.0)),
        "top-left corner included"
    );
    assert!(
        rect.contains(Vec2::new(14.9, 13.9)),
        "just inside bottom-right"
    );
    assert!(!rect.contains(Vec2::new(15.0, 13.0)), "right edge excluded");
    assert!(
        !rect.contains(Vec2::new(10.0, 14.0)),
        "bottom edge excluded"
    );
    assert!(
        !rect.contains(Vec2::new(4.9, 10.0)),
        "left of rect excluded"
    );
    assert!(!rect.contains(Vec2::new(10.0, 7.9)), "above rect excluded");
}

#[test]
fn rect_zero_size_contains_nothing() {
    // A rect with zero width or height has no interior, so contains() must
    // return false for every point, including the origin itself.
    let zero = Rect::new(5.0, 5.0, 0.0, 0.0);
    let zero_w = Rect::new(5.0, 5.0, 0.0, 10.0);
    let zero_h = Rect::new(5.0, 5.0, 10.0, 0.0);

    assert!(!zero.contains(Vec2::new(5.0, 5.0)));
    assert!(!zero_w.contains(Vec2::new(5.0, 5.0)));
    assert!(!zero_h.contains(Vec2::new(5.0, 5.0)));
}

#[test]
fn rect_inset_shrinks_all_edges_symmetrically() {
    let rect = Rect::new(0.0, 0.0, 100.0, 40.0);
    let inset = rect.inset(Edges::all(5.0));

    // Origin moves in by the left/top padding.
    assert_eq!(inset.origin, Vec2::new(5.0, 5.0));
    // Size shrinks by left+right and top+bottom.
    assert_eq!(inset.size.width, 90.0);
    assert_eq!(inset.size.height, 30.0);
    // The exclusive max must equal origin + size.
    assert_eq!(inset.max_exclusive(), Vec2::new(95.0, 35.0));
}

#[test]
fn rect_inset_asymmetric_edges() {
    let rect = Rect::new(10.0, 20.0, 80.0, 60.0);
    let edges = Edges {
        left: 4.0,
        right: 8.0,
        top: 2.0,
        bottom: 6.0,
    };
    let inset = rect.inset(edges);

    assert_eq!(inset.origin.x, 14.0, "origin.x = old_origin.x + left");
    assert_eq!(inset.origin.y, 22.0, "origin.y = old_origin.y + top");
    assert_eq!(inset.size.width, 68.0, "width = 80 - 4 - 8");
    assert_eq!(inset.size.height, 52.0, "height = 60 - 2 - 6");
}

#[test]
fn rect_inset_clamps_to_zero_when_padding_exceeds_size() {
    // Inset by more than the rect's size must not produce negative dimensions.
    let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
    let over = rect.inset(Edges::all(20.0));

    assert_eq!(over.size.width, 0.0, "width clamps to 0");
    assert_eq!(over.size.height, 0.0, "height clamps to 0");
}

#[test]
fn rect_inset_preserves_exclusive_max_contract() {
    // After an inset the exclusive max should still equal origin + size,
    // not the original rect's exclusive max minus padding.
    let rect = Rect::new(0.0, 0.0, 40.0, 20.0);
    let inset = rect.inset(Edges::all(3.0));

    assert_eq!(inset.max_exclusive(), inset.origin + inset.size.to_vec2());
}
