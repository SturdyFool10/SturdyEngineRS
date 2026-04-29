use glam::{Vec2, Vec3, Vec4};

/// DPI-scaled window coordinates delivered by the platform layer.
///
/// Origin is top-left, positive X right, positive Y down. For a target with
/// size `(width, height)`, `(width, height)` is the bottom-right pixel edge,
/// not an addressable pixel center.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WindowLogicalPx {
    pub x: f32,
    pub y: f32,
}

impl WindowLogicalPx {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    #[track_caller]
    pub fn new(x: f32, y: f32) -> Self {
        debug_assert_finite2("WindowLogicalPx", x, y);
        Self { x, y }
    }

    pub fn to_vec2(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

/// Raw physical window coordinates before DPI scaling.
///
/// Origin is top-left, positive X right, positive Y down. For a target with
/// size `(width, height)`, `(width, height)` is the bottom-right pixel edge,
/// not an addressable pixel center.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WindowPhysicalPx {
    pub x: f32,
    pub y: f32,
}

impl WindowPhysicalPx {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    #[track_caller]
    pub fn new(x: f32, y: f32) -> Self {
        debug_assert_finite2("WindowPhysicalPx", x, y);
        Self { x, y }
    }

    pub fn to_vec2(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

/// UI layout and hit-test pixel coordinates.
///
/// Origin is top-left, positive X right, positive Y down. For a UI root with
/// size `(width, height)`, `(width, height)` is the bottom-right edge and
/// rectangle max edges are exclusive.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UiPx {
    pub x: f32,
    pub y: f32,
}

impl UiPx {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    #[track_caller]
    pub fn new(x: f32, y: f32) -> Self {
        debug_assert_finite2("UiPx", x, y);
        Self { x, y }
    }

    pub fn to_vec2(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

/// Swapchain/surface pixel coordinates.
///
/// Origin is top-left, positive X right, positive Y down. For a surface with
/// size `(width, height)`, `(width, height)` is the bottom-right edge and
/// integer pixel indices run through `(width - 1, height - 1)`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SurfacePx {
    pub x: f32,
    pub y: f32,
}

impl SurfacePx {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    #[track_caller]
    pub fn new(x: f32, y: f32) -> Self {
        debug_assert_finite2("SurfacePx", x, y);
        Self { x, y }
    }

    pub fn to_vec2(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

/// Offscreen render target pixel coordinates.
///
/// Origin is top-left, positive X right, positive Y down. For a render target
/// with size `(width, height)`, `(width, height)` is the bottom-right edge and
/// integer pixel indices run through `(width - 1, height - 1)`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RenderTargetPx {
    pub x: f32,
    pub y: f32,
}

impl RenderTargetPx {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    #[track_caller]
    pub fn new(x: f32, y: f32) -> Self {
        debug_assert_finite2("RenderTargetPx", x, y);
        Self { x, y }
    }

    pub fn to_vec2(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

/// Integer texture texel coordinates.
///
/// Valid texel indices for a texture of size `(width, height)` run from
/// `(0, 0)` through `(width - 1, height - 1)`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TexelPx {
    pub x: i32,
    pub y: i32,
}

impl TexelPx {
    pub const ZERO: Self = Self { x: 0, y: 0 };

    #[track_caller]
    pub fn new(x: i32, y: i32) -> Self {
        debug_assert!(
            x >= 0 && y >= 0,
            "TexelPx coordinates must be non-negative, got ({x}, {y})"
        );
        Self { x, y }
    }
}

/// Normalized texture coordinate space (0.0-1.0).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Uv01 {
    pub u: f32,
    pub v: f32,
}

impl Uv01 {
    pub const ZERO: Self = Self { u: 0.0, v: 0.0 };

    #[track_caller]
    pub fn new(u: f32, v: f32) -> Self {
        debug_assert_finite2("Uv01", u, v);
        debug_assert!(
            (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v),
            "Uv01 coordinates must be in 0.0..=1.0, got ({u}, {v})"
        );
        Self { u, v }
    }

    pub fn to_vec2(self) -> Vec2 {
        Vec2::new(self.u, self.v)
    }
}

/// Backend-facing homogeneous clip space after projection and before
/// perspective divide.
///
/// Engine code should treat this as an adapter/render-pass boundary type.
/// App, UI, and gameplay code should normally use screen/UI pixel spaces,
/// `WorldSpace`, or `Ndc` instead of depending on backend clip conventions.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClipSpace {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl ClipSpace {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 0.0,
    };

    #[track_caller]
    pub fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        debug_assert_finite4("ClipSpace", x, y, z, w);
        Self { x, y, z, w }
    }

    pub fn to_vec4(self) -> Vec4 {
        Vec4::new(self.x, self.y, self.z, self.w)
    }
}

/// Game/world coordinates.
///
/// The engine does not assign a global up axis to this space. A scene, camera,
/// or game may define Y-up, Z-up, or another convention, but adapters must
/// explicitly convert world coordinates before using screen/UI pixel spaces.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WorldSpace {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl WorldSpace {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    #[track_caller]
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        debug_assert_finite3("WorldSpace", x, y, z);
        Self { x, y, z }
    }

    pub fn to_vec3(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }
}

/// Normalized device coordinate space.
///
/// Engine convention (matches Vulkan): x in [-1, 1] left-to-right,
/// y in [-1, 1] top-to-bottom. Backend adapters own any further remapping.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ndc {
    pub x: f32,
    pub y: f32,
}

impl Ndc {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    #[track_caller]
    pub fn new(x: f32, y: f32) -> Self {
        debug_assert_finite2("Ndc", x, y);
        debug_assert!(
            (-1.0..=1.0).contains(&x) && (-1.0..=1.0).contains(&y),
            "Ndc coordinates must be in -1.0..=1.0, got ({x}, {y})"
        );
        Self { x, y }
    }

    pub fn to_vec2(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

// ── Conversion helpers ────────────────────────────────────────────────────────

/// Convert physical window pixels to logical pixels using the DPI scale factor.
#[track_caller]
pub fn physical_to_logical(pos: WindowPhysicalPx, scale_factor: f32) -> WindowLogicalPx {
    debug_assert_valid_scale_factor(scale_factor);
    let scale = scale_factor.max(f32::EPSILON);
    WindowLogicalPx::new(pos.x / scale, pos.y / scale)
}

/// Convert logical window pixels to physical pixels using the DPI scale factor.
#[track_caller]
pub fn logical_to_physical(pos: WindowLogicalPx, scale_factor: f32) -> WindowPhysicalPx {
    debug_assert_valid_scale_factor(scale_factor);
    WindowPhysicalPx::new(pos.x * scale_factor, pos.y * scale_factor)
}

/// Convert logical window pixels to surface pixels using the DPI scale factor.
///
/// The coordinate orientation remains top-left/Y-down; only the unit changes
/// from logical pixels to physical surface pixels.
#[track_caller]
pub fn window_logical_to_surface(pos: WindowLogicalPx, scale_factor: f32) -> SurfacePx {
    let physical = logical_to_physical(pos, scale_factor);
    SurfacePx::new(physical.x, physical.y)
}

/// Convert logical window pixels to UI layout pixels.
///
/// For screen-space UI this is typically 1:1. World-UI adapters must perform
/// their own ray/surface-local conversion before calling into the UI system.
pub fn window_logical_to_ui(pos: WindowLogicalPx) -> UiPx {
    UiPx::new(pos.x, pos.y)
}

/// Convert UI layout pixels to surface pixels using the UI scale factor.
///
/// Screen-space UI commonly uses the window DPI scale factor here. Texture UI
/// and world-panel UI should pass their target-specific scale.
#[track_caller]
pub fn ui_to_surface(pos: UiPx, scale_factor: f32) -> SurfacePx {
    debug_assert_valid_scale_factor(scale_factor);
    SurfacePx::new(pos.x * scale_factor, pos.y * scale_factor)
}

/// Convert surface pixel coordinates to NDC (Vulkan convention: Y-down).
#[track_caller]
pub fn surface_to_ndc(pos: SurfacePx, surface_width: u32, surface_height: u32) -> Ndc {
    debug_assert!(
        surface_width > 0 && surface_height > 0,
        "surface_to_ndc requires non-zero surface dimensions, got {surface_width}x{surface_height}"
    );
    Ndc::new(
        pos.x / surface_width.max(1) as f32 * 2.0 - 1.0,
        pos.y / surface_height.max(1) as f32 * 2.0 - 1.0,
    )
}

/// Convert render-target pixel coordinates to normalized UV coordinates.
#[track_caller]
pub fn render_target_to_uv(pos: RenderTargetPx, target_width: u32, target_height: u32) -> Uv01 {
    debug_assert!(
        target_width > 0 && target_height > 0,
        "render_target_to_uv requires non-zero target dimensions, got {target_width}x{target_height}"
    );
    Uv01::new(
        pos.x / target_width.max(1) as f32,
        pos.y / target_height.max(1) as f32,
    )
}

#[track_caller]
fn debug_assert_finite2(space: &str, x: f32, y: f32) {
    debug_assert!(
        x.is_finite() && y.is_finite(),
        "{space} coordinates must be finite, got ({x}, {y})"
    );
}

#[track_caller]
fn debug_assert_finite3(space: &str, x: f32, y: f32, z: f32) {
    debug_assert!(
        x.is_finite() && y.is_finite() && z.is_finite(),
        "{space} coordinates must be finite, got ({x}, {y}, {z})"
    );
}

#[track_caller]
fn debug_assert_finite4(space: &str, x: f32, y: f32, z: f32, w: f32) {
    debug_assert!(
        x.is_finite() && y.is_finite() && z.is_finite() && w.is_finite(),
        "{space} coordinates must be finite, got ({x}, {y}, {z}, {w})"
    );
}

#[track_caller]
fn debug_assert_valid_scale_factor(scale_factor: f32) {
    debug_assert!(
        scale_factor.is_finite() && scale_factor > 0.0,
        "DPI scale factor must be finite and positive, got {scale_factor}"
    );
}

impl From<WindowLogicalPx> for UiPx {
    fn from(p: WindowLogicalPx) -> Self {
        window_logical_to_ui(p)
    }
}

#[cfg(test)]
mod tests {
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
}
