// Tests extracted from crates/sturdy-engine/src/ui_renderer.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn pixel_rect_to_ndc_uses_viewport_dimensions() {
    let (pos, size) = pixel_rect_to_ndc(100.0, 50.0, 300.0, 250.0, 800, 600);

    assert_eq!(pos, [-0.75, 1.0 - 250.0 / 600.0 * 2.0]);
    assert_eq!(size, [0.5, 200.0 / 600.0 * 2.0]);
}

#[test]
fn quad_bounds_handles_unordered_points() {
    let (min, max) = quad_bounds(&[[4.0, 1.0], [2.0, 9.0], [7.0, 3.0], [5.0, -1.0]]);

    assert_eq!(min, [2.0, -1.0]);
    assert_eq!(max, [7.0, 9.0]);
}

#[test]
fn clip_rect_intersects_enabled_axes_only() {
    let clip = ClipRect::viewport(100, 100);
    let horizontal_only =
        clip.intersect_axes(clay_ui::Rect::new(25.0, 30.0, 50.0, 10.0), true, false);

    assert_eq!(
        horizontal_only,
        ClipRect {
            min_x: 25.0,
            min_y: 0.0,
            max_x: 75.0,
            max_y: 100.0,
        }
    );
}

#[test]
fn clipped_uv_preserves_sample_region_after_scissor() {
    let original = clay_ui::Rect::new(100.0, 50.0, 200.0, 100.0);
    let clipped = clay_ui::Rect::new(150.0, 75.0, 100.0, 50.0);

    assert_eq!(clipped_uv(original, clipped), [0.25, 0.25, 0.75, 0.75]);
}
