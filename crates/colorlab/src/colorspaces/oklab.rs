/// Oklab color space and conversion to/from linear RGB (Color).
use crate::colorspaces::color::Color;
use crate::colorspaces::colorspace::ColorSpace;
use serde::{Deserialize, Serialize};

/// Oklab color space (perceptual, 0.0-1.0 for L, usually -0.5..0.5 for a/b, 0.0-1.0 for alpha)
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Oklab {
    pub l: f64,
    pub a: f64,
    pub b: f64,
    pub alpha: f64,
}

const EPSILON: f64 = 1e-10;

/// Clamp a value to a given range.
fn clamp(val: f64, min: f64, max: f64) -> f64 {
    if val < min {
        min
    } else if val > max {
        max
    } else {
        val
    }
}

impl ColorSpace for Oklab {
    fn to_color(&self) -> Color {
        // Oklab to linear RGB
        let l = self.l;
        let a = self.a;
        let b = self.b;

        // 1. Oklab to LMS
        let l_ = l + 0.3963377774 * a + 0.2158037573 * b;
        let m_ = l - 0.1055613458 * a - 0.0638541728 * b;
        let s_ = l - 0.0894841775 * a - 1.2914855480 * b;

        // 2. LMS to nonlinear (cube, clamp for stability)
        let l_cubed = if l_.abs() < EPSILON {
            0.0
        } else {
            l_ * l_ * l_
        };
        let m_cubed = if m_.abs() < EPSILON {
            0.0
        } else {
            m_ * m_ * m_
        };
        let s_cubed = if s_.abs() < EPSILON {
            0.0
        } else {
            s_ * s_ * s_
        };

        // 3. LMS to linear RGB
        let r = 4.0767416621 * l_cubed - 3.3077115913 * m_cubed + 0.2309699292 * s_cubed;
        let g = -1.2684380046 * l_cubed + 2.6097574011 * m_cubed - 0.3413193965 * s_cubed;
        let b = 0.0041960863 * l_cubed - 0.7034186147 * m_cubed + 1.7076147010 * s_cubed;

        Color {
            r: clamp(r, 0.0, 1.0),
            g: clamp(g, 0.0, 1.0),
            b: clamp(b, 0.0, 1.0),
            a: clamp(self.alpha, 0.0, 1.0),
        }
    }

    fn from_color(c: &Color) -> Self {
        // Clamp input for stability
        let r = clamp(c.r, 0.0, 1.0);
        let g = clamp(c.g, 0.0, 1.0);
        let b = clamp(c.b, 0.0, 1.0);

        // 1. Linear RGB to LMS
        let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
        let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
        let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;

        // 2. Nonlinear transform (cube root, clamp for stability)
        let l_ = if l.abs() < EPSILON {
            0.0
        } else {
            l.powf(1.0 / 3.0)
        };
        let m_ = if m.abs() < EPSILON {
            0.0
        } else {
            m.powf(1.0 / 3.0)
        };
        let s_ = if s.abs() < EPSILON {
            0.0
        } else {
            s.powf(1.0 / 3.0)
        };

        // 3. LMS to Oklab
        let l_final = 0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_;
        let a_final = 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_;
        let b_final = 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_;

        Oklab {
            l: clamp(l_final, 0.0, 1.0),
            a: clamp(a_final, -0.5, 0.5),
            b: clamp(b_final, -0.5, 0.5),
            alpha: clamp(c.a, 0.0, 1.0),
        }
    }
}
