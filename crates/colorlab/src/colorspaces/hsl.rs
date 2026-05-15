use crate::colorspaces::color::Color;
use crate::colorspaces::colorspace::ColorSpace;
use serde::{Deserialize, Serialize};

// NOTE: This implementation does not clamp input/output values.
// Documented risks: If input values are outside [0,1] for s, l, or a, or [0,360) for h, output RGB may be out of bounds.
// Division by zero is avoided by logic, but not explicitly guarded. See comments below for details.

const EPSILON: f64 = 1e-10;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Hsl {
    /// Hue in degrees [0, 360)
    pub h: f64,
    /// Saturation [0, 1]
    pub s: f64,
    /// Lightness [0, 1]
    pub l: f64,
    /// Alpha [0, 1]
    pub a: f64,
}

impl ColorSpace for Hsl {
    fn to_color(&self) -> Color {
        // Precompute constants
        let h = self.h / 360.0;
        let s = self.s;
        let l = self.l;

        // Avoid repeated computation of q and p
        // Numerical stability note: If s or l are out of [0,1], q and p may be out of bounds.
        let (q, p) = if l < 0.5 {
            let q = l * (1.0 + s);
            let p = 2.0 * l - q;
            (q, p)
        } else {
            let q = l + s - l * s;
            let p = 2.0 * l - q;
            (q, p)
        };

        // Precompute fractions for hue_to_rgb
        const ONE_SIXTH: f64 = 1.0 / 6.0;
        const ONE_THIRD: f64 = 1.0 / 3.0;
        const ONE_HALF: f64 = 0.5;
        const TWO_THIRDS: f64 = 2.0 / 3.0;

        // Inline hue_to_rgb for minimal ops
        // Numerical stability note: No division by zero, but p and q may be out of bounds if input is not valid.
        fn hue_to_rgb(p: f64, q: f64, mut t: f64) -> f64 {
            if t < 0.0 {
                t += 1.0;
            }
            if t > 1.0 {
                t -= 1.0;
            }
            if t < ONE_SIXTH {
                p + (q - p) * 6.0 * t
            } else if t < ONE_HALF {
                q
            } else if t < TWO_THIRDS {
                p + (q - p) * (TWO_THIRDS - t) * 6.0
            } else {
                p
            }
        }

        let r = hue_to_rgb(p, q, h + ONE_THIRD);
        let g = hue_to_rgb(p, q, h);
        let b = hue_to_rgb(p, q, h - ONE_THIRD);

        // Document: Output RGB may be out of [0,1] if input is not valid.
        Color::new(r, g, b, self.a)
    }

    fn from_color(c: &Color) -> Self {
        let r = c.r;
        let g = c.g;
        let b = c.b;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;

        // Only compute d once
        let d = max - min;

        // Numerical stability note: If d is very small, division by zero may occur.
        // We add an epsilon check to avoid division by zero.
        let (h, s) = if max == min || d.abs() < EPSILON {
            (0.0, 0.0) // achromatic
        } else {
            let s = if l > 0.5 {
                d / (2.0 - max - min)
            } else {
                d / (max + min)
            };
            // Avoid repeated computation for h
            let h = if max == r {
                ((g - b) / d + if g < b { 6.0 } else { 0.0 }) / 6.0
            } else if max == g {
                ((b - r) / d + 2.0) / 6.0
            } else {
                ((r - g) / d + 4.0) / 6.0
            };
            (h * 360.0, s)
        };

        // Document: Output H, S, L may be out of bounds if input RGB is not valid.
        Hsl { h, s, l, a: c.a }
    }
}
