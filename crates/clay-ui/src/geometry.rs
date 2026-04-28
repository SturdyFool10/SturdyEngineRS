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

    pub fn min(self) -> Vec2 {
        self.origin
    }

    /// Bottom-right exclusive edge of the rectangle.
    ///
    /// A full target rectangle with origin `(0, 0)` and size `(width, height)`
    /// has max edge `(width, height)`. Integer pixel indices inside it run
    /// through `(width - 1, height - 1)`.
    pub fn max_exclusive(self) -> Vec2 {
        self.origin + self.size.to_vec2()
    }

    /// Right exclusive edge.
    pub fn right(self) -> f32 {
        self.max_exclusive().x
    }

    /// Bottom exclusive edge.
    pub fn bottom(self) -> f32 {
        self.max_exclusive().y
    }

    pub fn center(self) -> Vec2 {
        self.origin + self.size.to_vec2() * 0.5
    }

    pub fn contains(self, point: Vec2) -> bool {
        point.x >= self.origin.x
            && point.x < self.right()
            && point.y >= self.origin.y
            && point.y < self.bottom()
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

    pub fn contains_point(self, rect: Rect, point: Vec2) -> bool {
        if !rect.contains(point) {
            return false;
        }

        match self {
            Self::Rect => true,
            Self::RoundedRect { radii } => {
                contains_rounded_rect(rect, point, scale_radii_to_rect(rect, radii), 2.0)
            }
            Self::IndependentCorners {
                top_left,
                top_right,
                bottom_right,
                bottom_left,
            } => contains_independent_corners(
                rect,
                point,
                scale_corner_specs_to_rect(rect, top_left, top_right, bottom_right, bottom_left),
            ),
            Self::Squircle { radius, exponent } => {
                let radius = radius.max(0.0);
                let radii = scale_radii_to_rect(rect, radii_all(radius));
                contains_rounded_rect(rect, point, radii, exponent.max(0.001))
            }
            Self::Capsule => {
                let radius = rect.size.width.min(rect.size.height) * 0.5;
                contains_rounded_rect(rect, point, radii_all(radius), 2.0)
            }
            Self::Circle => {
                let radius = rect.size.width.min(rect.size.height) * 0.5;
                let delta = point - rect.center();
                delta.length_squared() <= radius * radius
            }
            Self::Ellipse => {
                let rx = rect.size.width * 0.5;
                let ry = rect.size.height * 0.5;
                if rx <= 0.0 || ry <= 0.0 {
                    return true;
                }
                let delta = point - rect.center();
                let x = delta.x / rx;
                let y = delta.y / ry;
                x * x + y * y <= 1.0
            }
        }
    }
}

pub fn radii_all(radius: f32) -> Vec4 {
    Vec4::splat(radius)
}

fn contains_rounded_rect(rect: Rect, point: Vec2, radii: Vec4, exponent: f32) -> bool {
    let local = point - rect.origin;
    let width = rect.size.width.max(0.0);
    let height = rect.size.height.max(0.0);
    let exponent = exponent.max(0.001);

    let corners = [
        (radii.x, Vec2::new(radii.x, radii.x), local),
        (
            radii.y,
            Vec2::new(width - radii.y, radii.y),
            Vec2::new(width - local.x, local.y),
        ),
        (
            radii.z,
            Vec2::new(width - radii.z, height - radii.z),
            Vec2::new(width - local.x, height - local.y),
        ),
        (
            radii.w,
            Vec2::new(radii.w, height - radii.w),
            Vec2::new(local.x, height - local.y),
        ),
    ];

    for (radius, center, corner_local) in corners {
        if radius <= 0.0 || corner_local.x >= radius || corner_local.y >= radius {
            continue;
        }

        let delta = (local - center).abs() / radius;
        return delta.x.powf(exponent) + delta.y.powf(exponent) <= 1.0;
    }

    true
}

fn contains_independent_corners(rect: Rect, point: Vec2, corners: [CornerSpec; 4]) -> bool {
    let local = point - rect.origin;
    let width = rect.size.width.max(0.0);
    let height = rect.size.height.max(0.0);
    let corner_distances = [
        Vec2::new(local.x, local.y),
        Vec2::new(width - local.x, local.y),
        Vec2::new(width - local.x, height - local.y),
        Vec2::new(local.x, height - local.y),
    ];

    for (corner, distance) in corners.into_iter().zip(corner_distances) {
        let radius = corner.radius.max(0.0);
        if radius <= 0.0 || distance.x >= radius || distance.y >= radius {
            continue;
        }

        return contains_corner_shape(corner, distance);
    }

    true
}

fn contains_corner_shape(corner: CornerSpec, distance: Vec2) -> bool {
    let radius = corner.radius.max(0.0);
    match corner.shape {
        CornerShape::Round => {
            distance.length_squared() >= radius * radius || {
                let delta = Vec2::splat(radius) - distance;
                delta.length_squared() <= radius * radius
            }
        }
        CornerShape::Bevel | CornerShape::Chamfer => distance.x + distance.y >= radius,
        CornerShape::Notch => false,
        CornerShape::Scoop => distance.length_squared() >= radius * radius,
    }
}

fn scale_radii_to_rect(rect: Rect, radii: Vec4) -> Vec4 {
    let radii = radii.max(Vec4::ZERO);
    let width = rect.size.width.max(0.0);
    let height = rect.size.height.max(0.0);
    let scale = [
        edge_scale(width, radii.x + radii.y),
        edge_scale(height, radii.y + radii.z),
        edge_scale(width, radii.z + radii.w),
        edge_scale(height, radii.w + radii.x),
    ]
    .into_iter()
    .fold(1.0_f32, f32::min);

    radii * scale
}

fn scale_corner_specs_to_rect(
    rect: Rect,
    top_left: CornerSpec,
    top_right: CornerSpec,
    bottom_right: CornerSpec,
    bottom_left: CornerSpec,
) -> [CornerSpec; 4] {
    let mut corners = [top_left, top_right, bottom_right, bottom_left];
    for corner in &mut corners {
        corner.radius = corner.radius.max(0.0);
    }

    let width = rect.size.width.max(0.0);
    let height = rect.size.height.max(0.0);
    let scale = [
        edge_scale(width, corners[0].radius + corners[1].radius),
        edge_scale(height, corners[1].radius + corners[2].radius),
        edge_scale(width, corners[2].radius + corners[3].radius),
        edge_scale(height, corners[3].radius + corners[0].radius),
    ]
    .into_iter()
    .fold(1.0_f32, f32::min);

    for corner in &mut corners {
        corner.radius *= scale;
    }
    corners
}

fn edge_scale(edge: f32, radius_sum: f32) -> f32 {
    if radius_sum <= 0.0 || radius_sum <= edge {
        1.0
    } else {
        edge / radius_sum
    }
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
}
