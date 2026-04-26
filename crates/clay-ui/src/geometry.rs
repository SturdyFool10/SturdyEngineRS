use glam::{Vec2, Vec4};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const ZERO: Self = Self {
        width: 0.0,
        height: 0.0,
    };

    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    pub fn to_vec2(self) -> Vec2 {
        Vec2::new(self.width, self.height)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub origin: Vec2,
    pub size: Size,
}

impl Rect {
    pub const ZERO: Self = Self {
        origin: Vec2::ZERO,
        size: Size::ZERO,
    };

    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            origin: Vec2::new(x, y),
            size: Size::new(width, height),
        }
    }

    pub fn right(self) -> f32 {
        self.origin.x + self.size.width
    }

    pub fn bottom(self) -> f32 {
        self.origin.y + self.size.height
    }

    pub fn contains(self, point: Vec2) -> bool {
        point.x >= self.origin.x
            && point.x <= self.right()
            && point.y >= self.origin.y
            && point.y <= self.bottom()
    }

    pub fn inset(self, edges: Edges) -> Self {
        let width = (self.size.width - edges.left - edges.right).max(0.0);
        let height = (self.size.height - edges.top - edges.bottom).max(0.0);
        Self::new(
            self.origin.x + edges.left,
            self.origin.y + edges.top,
            width,
            height,
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Edges {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

impl Edges {
    pub const ZERO: Self = Self {
        left: 0.0,
        right: 0.0,
        top: 0.0,
        bottom: 0.0,
    };

    pub const fn all(value: f32) -> Self {
        Self {
            left: value,
            right: value,
            top: value,
            bottom: value,
        }
    }

    pub const fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            left: horizontal,
            right: horizontal,
            top: vertical,
            bottom: vertical,
        }
    }

    pub fn horizontal(self) -> f32 {
        self.left + self.right
    }

    pub fn vertical(self) -> f32 {
        self.top + self.bottom
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CornerShape {
    Round,
    Bevel,
    Chamfer,
    Notch,
    Scoop,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CornerSpec {
    pub radius: f32,
    pub shape: CornerShape,
    pub smoothing: f32,
}

impl CornerSpec {
    pub const fn round(radius: f32) -> Self {
        Self {
            radius,
            shape: CornerShape::Round,
            smoothing: 1.0,
        }
    }

    pub const fn with_shape(radius: f32, shape: CornerShape) -> Self {
        Self {
            radius,
            shape,
            smoothing: 1.0,
        }
    }
}

impl Default for CornerSpec {
    fn default() -> Self {
        Self::round(0.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UiShape {
    Rect,
    RoundedRect {
        radii: Vec4,
    },
    IndependentCorners {
        top_left: CornerSpec,
        top_right: CornerSpec,
        bottom_right: CornerSpec,
        bottom_left: CornerSpec,
    },
    Squircle {
        radius: f32,
        exponent: f32,
    },
    Capsule,
    Circle,
    Ellipse,
}

impl Default for UiShape {
    fn default() -> Self {
        Self::Rect
    }
}

impl UiShape {
    pub const fn rounded_rect(radii: Vec4) -> Self {
        Self::RoundedRect { radii }
    }

    pub const fn squircle(radius: f32, exponent: f32) -> Self {
        Self::Squircle { radius, exponent }
    }

    pub const fn independent_corners(
        top_left: CornerSpec,
        top_right: CornerSpec,
        bottom_right: CornerSpec,
        bottom_left: CornerSpec,
    ) -> Self {
        Self::IndependentCorners {
            top_left,
            top_right,
            bottom_right,
            bottom_left,
        }
    }

    pub fn with_corner_radius_fallback(self, radii: Vec4) -> Self {
        if matches!(self, Self::Rect) && radii.max_element() > 0.0 {
            Self::RoundedRect { radii }
        } else {
            self
        }
    }
}

pub fn radii_all(radius: f32) -> Vec4 {
    Vec4::splat(radius)
}

#[cfg(test)]
mod tests {
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
}
