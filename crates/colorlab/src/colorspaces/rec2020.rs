use crate::colorspaces::color::Color;
use crate::colorspaces::colorspace::ColorSpace;
use serde::{Deserialize, Serialize};

/// Rec.2020 RGB (D65), gamma ≈ 2.4 for SDR
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rec2020 {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl ColorSpace for Rec2020 {
    fn to_color(&self) -> Color {
        // Clamp input to [0.0, 1.0] for numerical stability before gamma decoding
        let clamp01 = |c: f64| c.max(0.0).min(1.0);
        let inv_gamma = |c: f64| clamp01(c).powf(2.4);

        let r_lin = inv_gamma(self.r);
        let g_lin = inv_gamma(self.g);
        let b_lin = inv_gamma(self.b);

        // Rec.2020 → XYZ
        let x = 0.636958_f64 * r_lin + 0.144617_f64 * g_lin + 0.168881_f64 * b_lin;
        let y = 0.2627_f64 * r_lin + 0.678_f64 * g_lin + 0.0593_f64 * b_lin;
        let z = 0.0_f64 * r_lin + 0.028073_f64 * g_lin + 1.060985_f64 * b_lin;

        // XYZ → linear sRGB
        let r = 3.240969_f64 * x - 1.537383_f64 * y - 0.498611_f64 * z;
        let g = -0.969244_f64 * x + 1.875968_f64 * y + 0.041555_f64 * z;
        let b = 0.05563_f64 * x - 0.203977_f64 * y + 1.056972_f64 * z;

        Color::new(r, g, b, self.a)
    }

    fn from_color(c: &Color) -> Self {
        // linear sRGB → XYZ
        let x = 0.4124 * c.r + 0.3576 * c.g + 0.1805 * c.b;
        let y = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
        let z = 0.0193 * c.r + 0.1192 * c.g + 0.9505 * c.b;

        // XYZ → Rec.2020 linear
        let r_lin = 1.7166634 * x - 0.3556733 * y - 0.2533681 * z;
        let g_lin = -0.6666738 * x + 1.6164557 * y + 0.0157683 * z;
        let b_lin = 0.0176425 * x - 0.0427769 * y + 0.9422433 * z;

        // Clamp before gamma encoding for stability
        let clamp01 = |c: f64| c.max(0.0).min(1.0);
        let gamma_encode = |c: f64| clamp01(c).powf(1.0 / 2.4);

        Rec2020 {
            r: gamma_encode(r_lin),
            g: gamma_encode(g_lin),
            b: gamma_encode(b_lin),
            a: c.a,
        }
    }
}
