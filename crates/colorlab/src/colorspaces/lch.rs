use crate::colorspaces::colorspace::ColorSpace;
use crate::colorspaces::lab::Lab;
use serde::{Deserialize, Serialize};

/// Cylindrical Lab: L, C, H (deg)
///
/// # Numerical Stability
/// - No clamping is performed on input or output values.
/// - If `c` is very close to zero, hue math may be unstable.
/// - Documented for future maintainers: consider clamping or epsilon checks if conversion issues arise.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Lch {
    pub l: f64,
    pub c: f64,
    pub h: f64,
    pub a: f64,
}

impl ColorSpace for Lch {
    fn to_color(&self) -> crate::colorspaces::color::Color {
        // Precompute radians once
        let h_rad = self.h.to_radians();
        let (sin_h, cos_h) = h_rad.sin_cos();
        // Epsilon check for chroma to avoid instability in hue math
        let epsilon = 1e-10;
        let c = if self.c.abs() < epsilon { 0.0 } else { self.c };
        let a = c * cos_h;
        let b = c * sin_h;
        Lab {
            l: self.l,
            a,
            b,
            alpha: self.a,
        }
        .to_color()
    }

    fn from_color(c: &crate::colorspaces::color::Color) -> Self {
        let Lab { l, a, b, alpha } = Lab::from_color(c);
        let c_val = (a * a + b * b).sqrt();
        // Epsilon check for chroma to avoid instability in hue math
        let epsilon = 1e-10;
        let mut h = if c_val.abs() < epsilon {
            0.0
        } else {
            b.atan2(a).to_degrees()
        };
        if h < 0.0 {
            h += 360.0;
        }
        Lch {
            l,
            c: c_val,
            h,
            a: alpha,
        }
    }
}
