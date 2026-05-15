use crate::colorspaces::color::Color;
use crate::colorspaces::colorspace::ColorSpace;
use serde::{Deserialize, Serialize};

// NOTE: Numerical stability risks:
// - powf operations can produce NaN for negative bases.
// - No clamping is performed; values may go out of bounds if input is not in [0,1].
// - Epsilon checks added for powf safety.
const EPSILON: f64 = 1e-10;

/// Adobe RGB (1998), D65 white, gamma ≈ 2.19921875
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AdobeRgb {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl ColorSpace for AdobeRgb {
    fn to_color(&self) -> Color {
        // Precompute exponent for inverse gamma (EOTF)
        let inv_gamma = 563.0 / 256.0;
        // Epsilon check for powf safety
        let r_lin = if self.r.abs() < EPSILON {
            0.0
        } else {
            self.r.powf(inv_gamma)
        };
        let g_lin = if self.g.abs() < EPSILON {
            0.0
        } else {
            self.g.powf(inv_gamma)
        };
        let b_lin = if self.b.abs() < EPSILON {
            0.0
        } else {
            self.b.powf(inv_gamma)
        };

        // Adobe RGB → CIEXYZ
        let x = 0.57667_f64 * r_lin + 0.18556_f64 * g_lin + 0.18823_f64 * b_lin;
        let y = 0.29734_f64 * r_lin + 0.62736_f64 * g_lin + 0.07529_f64 * b_lin;
        let z = 0.02703_f64 * r_lin + 0.07069_f64 * g_lin + 0.99134_f64 * b_lin;

        // CIEXYZ → linear sRGB
        let r = 3.2406_f64 * x - 1.5372_f64 * y - 0.4986_f64 * z;
        let g = -0.9689_f64 * x + 1.8758_f64 * y + 0.0415_f64 * z;
        let b = 0.0557_f64 * x - 0.2040_f64 * y + 1.0570_f64 * z;

        // NOTE: No clamping performed; output may be out of bounds if input is not in [0,1].
        Color::new(r, g, b, self.a as f64)
    }

    fn from_color(c: &Color) -> Self {
        // linear sRGB → CIEXYZ
        let x = 0.4124_f64 * c.r + 0.3576_f64 * c.g + 0.1805_f64 * c.b;
        let y = 0.2126_f64 * c.r + 0.7152_f64 * c.g + 0.0722_f64 * c.b;
        let z = 0.0193_f64 * c.r + 0.1192_f64 * c.g + 0.9505_f64 * c.b;

        // CIEXYZ → linear Adobe RGB
        let r_lin = 2.04159_f64 * x - 0.56501_f64 * y - 0.34473_f64 * z;
        let g_lin = -0.96924_f64 * x + 1.87597_f64 * y + 0.04156_f64 * z;
        let b_lin = 0.01344_f64 * x - 0.11836_f64 * y + 1.01517_f64 * z;

        // Precompute exponent for gamma (OETF)
        let gamma = 256.0 / 563.0;
        // Epsilon check for powf safety
        let r = if r_lin.abs() < EPSILON {
            0.0
        } else {
            r_lin.powf(gamma)
        };
        let g = if g_lin.abs() < EPSILON {
            0.0
        } else {
            g_lin.powf(gamma)
        };
        let b = if b_lin.abs() < EPSILON {
            0.0
        } else {
            b_lin.powf(gamma)
        };

        // NOTE: No clamping performed; output may be out of bounds if input is not in [0,1].
        AdobeRgb {
            r: r,
            g: g,
            b: b,
            a: c.a,
        }
    }
}
