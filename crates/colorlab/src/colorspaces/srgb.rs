use serde::{Deserialize, Serialize};

/// sRGB color space (non-linear, 0.0-1.0)
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]

pub struct Srgb {
    pub r: f64,

    pub g: f64,

    pub b: f64,

    pub a: f64,
}

use crate::colorspaces::color::Color;
use crate::colorspaces::colorspace::ColorSpace;

impl ColorSpace for Srgb {
    fn to_color(&self) -> Color {
        // Convert sRGB to linear RGB
        fn srgb_to_linear(c: f64) -> f64 {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }

        Color {
            r: srgb_to_linear(self.r),
            g: srgb_to_linear(self.g),
            b: srgb_to_linear(self.b),
            a: self.a,
        }
    }

    fn from_color(color: &Color) -> Self {
        // Convert linear RGB to sRGB
        fn linear_to_srgb(c: f64) -> f64 {
            if c <= 0.0031308 {
                12.92 * c
            } else {
                1.055 * c.powf(1.0 / 2.4) - 0.055
            }
        }

        Srgb {
            r: linear_to_srgb(color.r),
            g: linear_to_srgb(color.g),
            b: linear_to_srgb(color.b),
            a: color.a,
        }
    }
}
