use crate::colorspaces::color::Color;
use crate::colorspaces::colorspace::ColorSpace;
use serde::{Deserialize, Serialize};

// NOTE: Numerical stability risks documented below.
// - Cube root and powf operations may produce NaN/Inf for negative or zero values.
// - No clamping is performed; values may go out of bounds if input is not in [0,1].
// - Epsilon checks are added to avoid division by zero and unstable roots.

/// CIE Lab (D65) — L∈[0,100], a∈[-∞,∞], b∈[-∞,∞]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Lab {
    pub l: f64,
    pub a: f64,
    pub b: f64,
    pub alpha: f64,
}

const XN: f64 = 0.95047;
const YN: f64 = 1.0;
const ZN: f64 = 1.08883;

fn f(t: f64) -> f64 {
    // Precompute constants for minimal ops
    let eps = (6.0f64 / 29.0f64).powi(3);
    let k = (1.0f64 / 3.0f64) * (29.0f64 / 6.0f64).powi(2);
    let c = 4.0f64 / 29.0f64;
    // Epsilon check to avoid unstable cube root
    if t > eps {
        if t.abs() < 1e-10 {
            0.0
        } else {
            t.powf(1.0f64 / 3.0f64)
        }
    } else {
        k * t + c
    }
}

fn f_inv(u: f64) -> f64 {
    // Precompute constants for minimal ops
    let eps = 6.0f64 / 29.0f64;
    let k = 3.0f64 * (6.0f64 / 29.0f64).powi(2);
    let c = 4.0f64 / 29.0f64;
    // Epsilon check to avoid unstable powi
    if u > eps {
        if u.abs() < 1e-10 { 0.0 } else { u.powi(3) }
    } else {
        k * (u - c)
    }
}

impl ColorSpace for Lab {
    fn from_color(c: &Color) -> Self {
        let x = 0.4124 * c.r + 0.3576 * c.g + 0.1805 * c.b;
        let y = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
        let z = 0.0193 * c.r + 0.1192 * c.g + 0.9505 * c.b;

        let fx = f(x / XN);
        let fy = f(y / YN);
        let fz = f(z / ZN);

        let l = 116.0 * fy - 16.0;
        let a = 500.0 * (fx - fy);
        let b = 200.0 * (fy - fz);

        Lab {
            l,
            a,
            b,
            alpha: c.a,
        }
    }

    fn to_color(&self) -> Color {
        // Precompute constants for minimal ops
        const XN: f64 = 0.95047;
        const YN: f64 = 1.0;
        const ZN: f64 = 1.08883;

        let fy = (self.l + 16.0) / 116.0;
        let fx = fy + (self.a / 500.0);
        let fz = fy - (self.b / 200.0);

        let x = XN * f_inv(fx);
        let y = YN * f_inv(fy);
        let z = ZN * f_inv(fz);

        let r = 3.2406 * x - 1.5372 * y - 0.4986 * z;
        let g = -0.9689 * x + 1.8758 * y + 0.0415 * z;
        let b = 0.0557 * x - 0.2040 * y + 1.0570 * z;

        Color::new(r, g, b, self.alpha)
    }
}
