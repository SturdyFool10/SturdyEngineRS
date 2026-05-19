// Tests extracted from crates/sturdy-engine-testbed/src/bin/coordinate_validation.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn scene_uses_bottom_right_edge_not_last_pixel_as_target_max() {
    let scene = ValidationScene::new(640, 360, None);

    assert_eq!(
        scene.full_target.max_exclusive(),
        glam::Vec2::new(640.0, 360.0)
    );
    assert!(scene.full_target.contains(glam::Vec2::new(639.0, 359.0)));
    assert!(!scene.full_target.contains(glam::Vec2::new(640.0, 360.0)));
    assert_eq!(
        scene.marker_draw_pos(WindowLogicalPx::new(640.0, 360.0)),
        [639.0, 359.0]
    );
}

#[test]
fn scene_uv_samples_cover_corners_edges_and_center() {
    let scene = ValidationScene::new(640, 360, None);
    let samples = scene
        .uv_samples
        .map(|sample| render_target_to_uv(sample, 640, 360).to_vec2());

    assert_eq!(samples[0], glam::Vec2::new(0.0, 0.0));
    assert_eq!(samples[1], glam::Vec2::new(1.0, 0.0));
    assert_eq!(samples[2], glam::Vec2::new(0.0, 1.0));
    assert_eq!(samples[3], glam::Vec2::new(1.0, 1.0));
    assert_eq!(samples[4], glam::Vec2::new(0.5, 0.5));
}

#[test]
fn clipped_ui_child_is_limited_to_clip_rect() {
    let scene = ValidationScene::new(640, 360, None);
    let child = Rect::new(
        scene.clip_rect.origin.x + scene.clip_rect.size.width * 0.65,
        scene.clip_rect.origin.y + scene.clip_rect.size.height * 0.65,
        scene.clip_rect.size.width * 0.5,
        scene.clip_rect.size.height * 0.5,
    );
    let visible = intersect_rects(scene.clip_rect, child);

    assert!(scene.clip_rect.contains(visible.origin));
    assert_eq!(visible.max_exclusive(), scene.clip_rect.max_exclusive());
}

#[test]
fn scene_uses_live_cursor_when_available() {
    let cursor = WindowLogicalPx::new(12.0, 34.0);
    let scene = ValidationScene::new(640, 360, Some(cursor));

    assert_eq!(scene.cursor, cursor);
    assert!(scene.cursor_live);
}
