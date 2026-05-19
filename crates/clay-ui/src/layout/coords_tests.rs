// Tests extracted from crates/clay-ui/src/layout/coords.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn clip_space_preserves_homogeneous_components() {
    let clip = ClipSpace::new(1.0, -2.0, 0.5, 4.0);

    assert_eq!(clip.to_vec4(), Vec4::new(1.0, -2.0, 0.5, 4.0));
}

#[test]
fn world_space_preserves_scene_defined_axes() {
    let world = WorldSpace::new(1.0, 2.0, 3.0);

    assert_eq!(world.to_vec3(), Vec3::new(1.0, 2.0, 3.0));
}

#[test]
fn window_logical_to_surface_applies_dpi_scale_without_flipping_axes() {
    let surface = window_logical_to_surface(WindowLogicalPx::new(12.0, 8.0), 1.5);

    assert_eq!(surface, SurfacePx::new(18.0, 12.0));
}

#[test]
fn ui_to_surface_applies_target_scale_without_flipping_axes() {
    let surface = ui_to_surface(UiPx::new(20.0, 30.0), 2.0);

    assert_eq!(surface, SurfacePx::new(40.0, 60.0));
}

#[test]
fn render_target_to_uv_maps_edges_to_unit_square() {
    let uv = render_target_to_uv(RenderTargetPx::new(320.0, 180.0), 640, 360);
    let bottom_right_edge = render_target_to_uv(RenderTargetPx::new(640.0, 360.0), 640, 360);

    assert_eq!(uv, Uv01::new(0.5, 0.5));
    assert_eq!(bottom_right_edge, Uv01::new(1.0, 1.0));
}

#[test]
#[should_panic(expected = "DPI scale factor must be finite and positive")]
fn logical_to_physical_rejects_invalid_scale_in_debug_builds() {
    let _ = logical_to_physical(WindowLogicalPx::ZERO, 0.0);
}

#[test]
#[should_panic(expected = "Uv01 coordinates must be in 0.0..=1.0")]
fn uv01_rejects_out_of_range_values_in_debug_builds() {
    let _ = Uv01::new(1.25, 0.5);
}

#[test]
#[should_panic(expected = "Ndc coordinates must be in -1.0..=1.0")]
fn ndc_rejects_out_of_range_values_in_debug_builds() {
    let _ = Ndc::new(0.0, -1.25);
}

#[test]
#[should_panic(expected = "surface_to_ndc requires non-zero surface dimensions")]
fn surface_to_ndc_rejects_zero_sized_surfaces_in_debug_builds() {
    let _ = surface_to_ndc(SurfacePx::ZERO, 0, 100);
}

#[test]
#[should_panic(expected = "render_target_to_uv requires non-zero target dimensions")]
fn render_target_to_uv_rejects_zero_sized_targets_in_debug_builds() {
    let _ = render_target_to_uv(RenderTargetPx::ZERO, 100, 0);
}

// ── Edge-inclusive / exclusive coordinate contract ────────────────────────

#[test]
fn surface_to_ndc_top_left_corner_maps_to_negative_one() {
    // The inclusive top-left corner of a surface (0, 0) must map to NDC
    // (-1, -1) for both axes (Vulkan/engine Y-down convention).
    let ndc = surface_to_ndc(SurfacePx::ZERO, 640, 360);

    assert_eq!(ndc, Ndc::new(-1.0, -1.0));
}

#[test]
fn surface_to_ndc_exclusive_bottom_right_maps_to_positive_one() {
    // The exclusive bottom-right edge of a surface (width, height) maps to
    // NDC (1, 1).  This edge is not a valid pixel address but it is the
    // canonical boundary that a full-screen quad must cover.
    let ndc = surface_to_ndc(SurfacePx::new(640.0, 360.0), 640, 360);

    assert_eq!(ndc, Ndc::new(1.0, 1.0));
}

#[test]
fn surface_to_ndc_centre_pixel_maps_to_origin() {
    // The centre of a power-of-two surface should land at NDC (0, 0).
    let ndc = surface_to_ndc(SurfacePx::new(320.0, 180.0), 640, 360);

    assert_eq!(ndc, Ndc::new(0.0, 0.0));
}

#[test]
fn window_logical_to_ui_is_identity_for_screen_space() {
    // For screen-space UI the logical-pixel coordinate is passed through
    // unchanged into UI layout space (1:1 mapping, top-left/Y-down).
    let ui = window_logical_to_ui(WindowLogicalPx::new(123.0, 456.0));

    assert_eq!(ui, UiPx::new(123.0, 456.0));
}

#[test]
fn physical_to_logical_inverts_logical_to_physical() {
    let logical = WindowLogicalPx::new(100.0, 50.0);
    let scale = 1.5_f32;
    let physical = logical_to_physical(logical, scale);
    let back = physical_to_logical(physical, scale);

    // Round-trip should be lossless for simple scale factors.
    assert!(
        (back.x - logical.x).abs() < 1e-4,
        "x round-trip: {} vs {}",
        back.x,
        logical.x
    );
    assert!(
        (back.y - logical.y).abs() < 1e-4,
        "y round-trip: {} vs {}",
        back.y,
        logical.y
    );
}
