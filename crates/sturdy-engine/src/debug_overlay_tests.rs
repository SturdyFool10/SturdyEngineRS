// Tests extracted from crates/sturdy-engine/src/debug_overlay.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn screen_space_primitives_emit_geometry() {
    let mut overlay = DebugOverlay::new();
    overlay.line_screen(800, 600, [0.0, 0.0], [800.0, 600.0]);
    overlay.rectangle_screen(800, 600, [100.0, 100.0], [200.0, 50.0]);
    overlay.circle_screen(800, 600, [400.0, 300.0], 20.0);
    overlay.cross_marker_screen(800, 600, [400.0, 300.0], 12.0);

    assert!(!overlay.shapes().is_empty());
    assert_eq!(overlay.text_descs().len(), 0);
}

#[test]
fn text_and_shapes_can_coexist() {
    let mut overlay = DebugOverlay::new();
    overlay.add_screen_text("hello", 18.0, 18.0);
    overlay.filled_rect_screen(1280, 720, [8.0, 8.0], [220.0, 64.0], [0.0, 0.0, 0.0, 0.35]);

    assert!(!overlay.is_empty());
    assert_eq!(overlay.text_descs().len(), 1);
    assert_eq!(overlay.ui_shapes.len(), 1);
}

#[test]
fn transform_and_hit_regions_are_applied_in_screen_space() {
    let mut overlay = DebugOverlay::new();
    overlay
        .set_antialiasing(DebugOverlayAntialiasing::Disabled)
        .set_transform(DebugOverlayTransform {
            translation: [10.0, 20.0],
            scale: [2.0, 2.0],
        })
        .register_hit_region("panel", [10.0, 20.0], [100.0, 40.0]);
    overlay.rectangle_screen(800, 600, [0.0, 0.0], [50.0, 20.0]);

    assert_eq!(
        overlay
            .hit_test_screen([32.0, 38.0])
            .map(|region| region.tag.as_str()),
        Some("panel")
    );
    assert_eq!(
        overlay.config().antialiasing,
        DebugOverlayAntialiasing::Disabled
    );
    assert!(!overlay.shapes().is_empty());
}

#[test]
fn rounded_rectangle_outline_emits_geometry() {
    let mut overlay = DebugOverlay::new();
    overlay.rounded_rectangle_outline_screen(
        1280,
        720,
        [16.0, 16.0],
        [200.0, 80.0],
        10.0,
        3.0,
        [1.0, 1.0, 1.0, 1.0],
    );

    assert_eq!(overlay.ui_shapes.len(), 1);
    assert_eq!(overlay.ui_shapes[0].border_width, 3.0);
}
