use glam::Vec2;

/// DPI-scaled window coordinates delivered by the platform layer.
/// Origin is top-left, positive X right, positive Y down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WindowLogicalPx {
    pub x: f32,
    pub y: f32,
}

impl WindowLogicalPx {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn to_vec2(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

/// Raw physical window coordinates before DPI scaling.
/// Origin is top-left, positive X right, positive Y down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WindowPhysicalPx {
    pub x: f32,
    pub y: f32,
}

impl WindowPhysicalPx {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn to_vec2(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

/// UI layout and hit-test pixel coordinates.
/// Origin is top-left, positive X right, positive Y down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UiPx {
    pub x: f32,
    pub y: f32,
}

impl UiPx {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn to_vec2(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

/// Swapchain/surface pixel coordinates.
/// Origin is top-left, positive X right, positive Y down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SurfacePx {
    pub x: f32,
    pub y: f32,
}

impl SurfacePx {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn to_vec2(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

/// Offscreen render target pixel coordinates.
/// Origin is top-left, positive X right, positive Y down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RenderTargetPx {
    pub x: f32,
    pub y: f32,
}

impl RenderTargetPx {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn to_vec2(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

/// Integer texture texel coordinates.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TexelPx {
    pub x: i32,
    pub y: i32,
}

impl TexelPx {
    pub const ZERO: Self = Self { x: 0, y: 0 };

    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// Normalized texture coordinate space (0.0–1.0).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Uv01 {
    pub u: f32,
    pub v: f32,
}

impl Uv01 {
    pub const ZERO: Self = Self { u: 0.0, v: 0.0 };

    pub fn new(u: f32, v: f32) -> Self {
        Self { u, v }
    }

    pub fn to_vec2(self) -> Vec2 {
        Vec2::new(self.u, self.v)
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

    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn to_vec2(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

// ── Conversion helpers ────────────────────────────────────────────────────────

/// Convert physical window pixels to logical pixels using the DPI scale factor.
pub fn physical_to_logical(pos: WindowPhysicalPx, scale_factor: f32) -> WindowLogicalPx {
    let scale = scale_factor.max(f32::EPSILON);
    WindowLogicalPx::new(pos.x / scale, pos.y / scale)
}

/// Convert logical window pixels to physical pixels using the DPI scale factor.
pub fn logical_to_physical(pos: WindowLogicalPx, scale_factor: f32) -> WindowPhysicalPx {
    WindowPhysicalPx::new(pos.x * scale_factor, pos.y * scale_factor)
}

/// Convert logical window pixels to UI layout pixels.
///
/// For screen-space UI this is typically 1:1. World-UI adapters must perform
/// their own ray/surface-local conversion before calling into the UI system.
pub fn window_logical_to_ui(pos: WindowLogicalPx) -> UiPx {
    UiPx::new(pos.x, pos.y)
}

/// Convert surface pixel coordinates to NDC (Vulkan convention: Y-down).
pub fn surface_to_ndc(pos: SurfacePx, surface_width: u32, surface_height: u32) -> Ndc {
    Ndc::new(
        pos.x / surface_width.max(1) as f32 * 2.0 - 1.0,
        pos.y / surface_height.max(1) as f32 * 2.0 - 1.0,
    )
}

impl From<WindowLogicalPx> for UiPx {
    fn from(p: WindowLogicalPx) -> Self {
        window_logical_to_ui(p)
    }
}
