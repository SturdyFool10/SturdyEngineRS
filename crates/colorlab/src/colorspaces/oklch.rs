use crate::colorspaces::colorspace::ColorSpace;
use crate::colorspaces::oklab::Oklab;
use serde::{Deserialize, Serialize};

/// Oklch color space (cylindrical representation of Oklab)
///
/// # Fields
/// - l: lightness (0.0-1.0)
/// - c: chroma (0.0+, typically 0.0-0.4)
/// - h: hue angle in degrees (0.0-360.0)
/// - alpha: opacity (0.0-1.0)
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Oklch {
    pub l: f64,
    pub c: f64,
    pub h: f64,
    pub alpha: f64,
}

impl Default for Oklch {
    fn default() -> Self {
        Self {
            l: 0.0,
            c: 0.0,
            h: 0.0,
            alpha: 1.0,
        }
    }
}

impl ColorSpace for Oklch {
    fn to_color(&self) -> crate::colorspaces::color::Color {
        // Precompute radians once
        let h_rad = self.h.to_radians();
        let (sin_h, cos_h) = h_rad.sin_cos();

        // Epsilon check for chroma to avoid instability in hue math
        let epsilon = 1e-10;
        let c = if self.c.abs() < epsilon { 0.0 } else { self.c };

        let a = c * cos_h;
        let b = c * sin_h;

        Oklab {
            l: self.l,
            a,
            b,
            alpha: self.alpha,
        }
        .to_color()
    }

    fn from_color(c: &crate::colorspaces::color::Color) -> Self {
        let Oklab { l, a, b, alpha } = Oklab::from_color(c);

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

        Oklch {
            l,
            c: c_val,
            h,
            alpha,
        }
    }
}
