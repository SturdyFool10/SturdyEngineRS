use crate::colorspaces::color::Color;
use crate::colorspaces::colorspace::ColorSpace;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Hsv {
    /// Hue in degrees [0,360)
    pub h: f64,
    /// Saturation [0,1]
    pub s: f64,
    /// Value [0,1]
    pub v: f64,
    pub a: f64,
}

// NOTE: This implementation does not clamp input/output values.
// Documented risks: If input values are outside [0,1] for s, v, or a, or [0,360) for h, output RGB may be out of bounds.
impl ColorSpace for Hsv {
    fn to_color(&self) -> Color {
        let h = self.h / 60.0;
        let s = self.s;
        let v = self.v;
        let i = h.floor() as i32;
        let f = h - (i as f64);

        // Precompute 1-s, s*f, s*(1-f)
        let one_minus_s = 1.0 - s;
        let s_times_f = s * f;
        let s_times_one_minus_f = s * (1.0 - f);

        let p = v * one_minus_s;
        let q = v * (1.0 - s_times_f);
        let t = v * (1.0 - s_times_one_minus_f);

        let (r, g, b) = match i.rem_euclid(6) {
            0 => (v, t, p),
            1 => (q, v, p),
            2 => (p, v, t),
            3 => (p, q, v),
            4 => (t, p, v),
            _ => (v, p, q),
        };

        // Document: Output RGB may be out of [0,1] if input is not valid.
        Color::new(r, g, b, self.a)
    }

    fn from_color(c: &Color) -> Self {
        let r = c.r;
        let g = c.g;
        let b = c.b;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let v = max;
        let d = max - min;
        let s = if max != 0.0 { d / max } else { 0.0 };

        // Only compute h if d != 0
        let h = if d == 0.0 {
            0.0
        } else {
            let h = if max == r {
                ((g - b) / d) % 6.0
            } else if max == g {
                ((b - r) / d) + 2.0
            } else {
                ((r - g) / d) + 4.0
            };
            60.0 * h
        };

        Hsv {
            h: if h < 0.0 { h + 360.0 } else { h },
            s,
            v,
            a: c.a,
        }
    }
}
