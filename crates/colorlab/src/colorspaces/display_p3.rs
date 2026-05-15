use crate::colorspaces::color::Color;
use crate::colorspaces::colorspace::ColorSpace;
use serde::{Deserialize, Serialize};

/// Display P3 (DCI‑P3 primaries + D65 white, sRGB γ)
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DisplayP3 {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

// NOTE: This implementation does not clamp input/output values.
// Documented risks: If input values are outside [0,1] for r, g, b, or a, output RGB may be out of bounds.
// powf operations can produce NaN for negative bases. No clamping is performed; values may go out of bounds if input is not in [0,1].
impl ColorSpace for DisplayP3 {
    fn to_color(&self) -> Color {
        // Precompute constants for gamma decoding
        const GAMMA_THRESHOLD: f64 = 0.04045;
        const GAMMA_DIV: f64 = 12.92;
        const GAMMA_A: f64 = 0.055;
        const GAMMA_B: f64 = 1.055;
        const GAMMA_EXP: f64 = 2.4;

        // Decode sRGB gamma for each channel (no repeated ops)
        let decode = |c: f64| {
            if c <= GAMMA_THRESHOLD {
                c / GAMMA_DIV
            } else {
                ((c + GAMMA_A) / GAMMA_B).powf(GAMMA_EXP)
            }
        };
        let r_lin = decode(self.r);
        let g_lin = decode(self.g);
        let b_lin = decode(self.b);

        // P3→XYZ (minimal ops)
        let x = 0.486569 * r_lin + 0.265673 * g_lin + 0.198187 * b_lin;
        let y = 0.228973 * r_lin + 0.691752 * g_lin + 0.0792749 * b_lin;
        let z = 0.0451143 * g_lin + 1.04379 * b_lin;

        // XYZ→linear sRGB (minimal ops)
        let r = 3.2406 * x - 1.5372 * y - 0.4986 * z;
        let g = -0.9689 * x + 1.8758 * y + 0.0415 * z;
        let b = 0.0557 * x - 0.2040 * y + 1.0570 * z;

        // Document: Output RGB may be out of [0,1] if input is not valid.
        Color::new(r, g, b, self.a)
    }

    fn from_color(c: &Color) -> Self {
        // Precompute constants for gamma encoding
        const GAMMA_ENCODE_THRESHOLD: f64 = 0.0031308;
        const GAMMA_ENCODE_A: f64 = 1.055;
        const GAMMA_ENCODE_B: f64 = 0.055;
        const GAMMA_ENCODE_DIV: f64 = 12.92;
        const GAMMA_ENCODE_EXP: f64 = 1.0 / 2.4;

        // linear sRGB→XYZ
        let x = 0.4124 * c.r + 0.3576 * c.g + 0.1805 * c.b;
        let y = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
        let z = 0.0193 * c.r + 0.1192 * c.g + 0.9505 * c.b;

        // XYZ→P3 linear RGB
        let r_lin = 1.2249 * x - 0.2247 * y - 0.0040 * z;
        let g_lin = -0.0420 * x + 1.0419 * y + 0.0001 * z;
        let b_lin = 0.0000 * x - 0.0776 * y + 0.9398 * z;

        // sRGB gamma encode (minimal ops)
        let encode = |c: f64| {
            if c <= GAMMA_ENCODE_THRESHOLD {
                GAMMA_ENCODE_DIV * c
            } else {
                GAMMA_ENCODE_A * c.powf(GAMMA_ENCODE_EXP) - GAMMA_ENCODE_B
            }
        };
        let r = encode(r_lin);
        let g = encode(g_lin);
        let b = encode(b_lin);

        DisplayP3 { r, g, b, a: c.a }
    }
}
