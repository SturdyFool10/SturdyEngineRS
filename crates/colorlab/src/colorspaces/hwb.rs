use crate::colorspaces::color::Color;
use crate::colorspaces::colorspace::ColorSpace;
use crate::colorspaces::hsv::Hsv;
use serde::{Deserialize, Serialize};

// Note: This implementation does not clamp output RGB values after HSV conversion.
// If input values for w, b, or a are out of bounds, results may be unpredictable.
// Division by zero is avoided by logic, but not explicitly guarded.
// Documented for future maintainers.
const EPSILON: f64 = 1e-10;

/// HWB: Hue, Whiteness, Blackness (CSS Level 4)
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Hwb {
    pub h: f64,
    pub w: f64,
    pub b: f64,
    pub a: f64,
}

impl ColorSpace for Hwb {
    fn to_color(&self) -> Color {
        // Clamp w and b for stability, but output RGB is not clamped.
        let w = self.w.clamp(0.0, 1.0);
        let bl = self.b.clamp(0.0, 1.0);
        let sum = w + bl;
        if sum >= 1.0 {
            // Avoid division by zero with epsilon check
            let denom = if sum.abs() < EPSILON { EPSILON } else { sum };
            let gray = w / denom;
            return Color::new(gray, gray, gray, self.a);
        }
        let v = 1.0 - bl;
        let s = if v > EPSILON { 1.0 - (w / v) } else { 0.0 };
        // Directly construct and convert Hsv, minimizing ops
        Hsv {
            h: self.h,
            s,
            v,
            a: self.a,
        }
        .to_color()
    }

    fn from_color(c: &Color) -> Self {
        // No clamping on input; document for maintainers.
        let Hsv { h, a, .. } = Hsv::from_color(c);
        let w = c.r.min(c.g).min(c.b);
        let bl = 1.0 - c.r.max(c.g).max(c.b);
        Hwb { h, w, b: bl, a }
    }
}
