// Tests extracted from crates/sturdy-engine/src/debug_draw_2d.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn line_emits_core_and_fringe_quads() {
    let mut draw = DebugDraw2d::new();
    draw.line([-0.5, 0.0], [0.5, 0.0]);

    assert_eq!(draw.vertex_count(), 12);
    assert_eq!(draw.index_count(), 18);
}

#[test]
fn polyline_emits_one_segment_per_pair() {
    let mut draw = DebugDraw2d::new();
    draw.polyline(&[[-0.5, -0.5], [0.0, 0.0], [0.5, -0.5]]);

    assert_eq!(draw.vertex_count(), 24);
    assert_eq!(draw.index_count(), 36);
}

#[test]
fn rectangle_and_point_marker_accumulate_geometry() {
    let mut draw = DebugDraw2d::new();
    draw.rectangle([-0.5, -0.5], [1.0, 1.0]);
    draw.cross_marker([0.0, 0.0]);
    draw.point([0.0, 0.0]);

    assert_eq!(draw.vertex_count(), 76);
    assert_eq!(draw.index_count(), 114);
}

#[test]
fn filled_polygon_uses_triangle_fan() {
    let mut draw = DebugDraw2d::new();
    draw.filled_polygon(
        &[[-0.5, -0.5], [0.5, -0.5], [0.0, 0.5]],
        [1.0, 0.0, 0.0, 1.0],
    );

    assert_eq!(draw.vertex_count(), 3);
    assert_eq!(draw.index_count(), 3);
}

#[test]
fn circle_respects_minimum_segment_count() {
    let mut draw = DebugDraw2d::with_style(DebugDrawStyle {
        circle_segments: 2,
        ..DebugDrawStyle::default()
    });
    draw.circle([0.0, 0.0], 0.5);
    draw.filled_circle([0.0, 0.0], 0.25);

    assert_eq!(draw.vertex_count(), 39);
    assert_eq!(draw.index_count(), 57);
}
