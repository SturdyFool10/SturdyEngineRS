use crate::colorspaces::color::Color;
use crate::colorspaces::colorspace::ColorSpace;
use serde::{Deserialize, Serialize};

/// CIE L*u*v* (D65)
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Luv {
    pub l: f64,
    pub u: f64,
    pub v: f64,
    pub alpha: f64,
}

const XN_LUV: f64 = 0.95047;
const YN_LUV: f64 = 1.0;
const ZN_LUV: f64 = 1.08883;

const EPSILON: f64 = 1e-10;

fn u_prime(x: f64, y: f64, z: f64) -> f64 {
    let denom = x + 15.0 * y + 3.0 * z;
    if denom.abs() < EPSILON {
        0.0
    } else {
        4.0 * x / denom
    }
}
fn v_prime(x: f64, y: f64, z: f64) -> f64 {
    let denom = x + 15.0 * y + 3.0 * z;
    if denom.abs() < EPSILON {
        0.0
    } else {
        9.0 * y / denom
    }
}

impl ColorSpace for Luv {
    fn to_color(&self) -> Color {
        // Reference white u', v'
        let up_ref = u_prime(XN_LUV, YN_LUV, ZN_LUV);
        let vp_ref = v_prime(XN_LUV, YN_LUV, ZN_LUV);

        let l = self.l;
        let u = self.u;
        let v = self.v;

        let yr = if l > 8.0 {
            ((l + 16.0) / 116.0).powi(3)
        } else {
            l / 903.3
        };

        let up = if l.abs() < EPSILON {
            up_ref
        } else {
            u / (13.0 * l) + up_ref
        };
        let vp = if l.abs() < EPSILON {
            vp_ref
        } else {
            v / (13.0 * l) + vp_ref
        };

        let vp_denom = (4.0 * vp).abs().max(EPSILON);

        let x = yr * 9.0 * up / vp_denom;
        let y = yr;
        let z = yr * (12.0 - 3.0 * up - 20.0 * vp) / vp_denom;

        let r = 3.2406 * x - 1.5372 * y - 0.4986 * z;
        let g = -0.9689 * x + 1.8758 * y + 0.0415 * z;
        let b = 0.0557 * x - 0.2040 * y + 1.0570 * z;

        Color::new(r, g, b, self.alpha)
    }

    fn from_color(c: &Color) -> Self {
        let x = 0.4124 * c.r + 0.3576 * c.g + 0.1805 * c.b;
        let y = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
        let z = 0.0193 * c.r + 0.1192 * c.g + 0.9505 * c.b;

        let yr = y / YN_LUV;
        let l = if yr > 0.008856 {
            116.0 * yr.powf(1.0 / 3.0) - 16.0
        } else {
            903.3 * yr
        };
        let ur_p = u_prime(x, y, z);
        let vr_p = v_prime(x, y, z);
        let ur_n = u_prime(XN_LUV, YN_LUV, ZN_LUV);
        let vr_n = v_prime(XN_LUV, YN_LUV, ZN_LUV);

        let u = 13.0 * l * (ur_p - ur_n);
        let v = 13.0 * l * (vr_p - vr_n);

        Luv {
            l,
            u,
            v,
            alpha: c.a,
        }
    }
}
