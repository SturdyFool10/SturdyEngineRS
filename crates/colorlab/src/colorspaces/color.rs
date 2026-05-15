use serde::{Deserialize, Serialize};

use crate::colorspaces::colorspace::ColorSpace;
use crate::colorspaces::interpolate::Interpolate;

/// Linear-light RGBA, f64 components. This is the hub type that all color spaces
/// convert through. Values above 1.0 are valid for HDR.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

// Named linear-light primaries. Only pure primaries and achromatics are provided
// because their sRGB and linear values coincide at 0.0 and 1.0.
impl Color {
    pub const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    pub const WHITE: Self = Self {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };
    pub const TRANSPARENT: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };
    pub const RED: Self = Self {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    pub const LIME: Self = Self {
        r: 0.0,
        g: 1.0,
        b: 0.0,
        a: 1.0,
    };
    pub const BLUE: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 1.0,
        a: 1.0,
    };
    pub const YELLOW: Self = Self {
        r: 1.0,
        g: 1.0,
        b: 0.0,
        a: 1.0,
    };
    pub const CYAN: Self = Self {
        r: 0.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };
    pub const MAGENTA: Self = Self {
        r: 1.0,
        g: 0.0,
        b: 1.0,
        a: 1.0,
    };
}

impl Color {
    /// Construct from linear RGBA components directly.
    #[inline]
    pub fn new(r: f64, g: f64, b: f64, a: f64) -> Self {
        Self { r, g, b, a }
    }

    /// Construct an opaque color from linear RGB.
    #[inline]
    pub fn opaque(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// Construct from 8-bit sRGB components (0–255). Alpha is set to 1.0.
    pub fn from_srgb_u8(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: srgb_to_linear(r as f64 / 255.0),
            g: srgb_to_linear(g as f64 / 255.0),
            b: srgb_to_linear(b as f64 / 255.0),
            a: 1.0,
        }
    }

    /// Construct from 8-bit sRGB + alpha (0–255 each).
    pub fn from_srgba_u8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: srgb_to_linear(r as f64 / 255.0),
            g: srgb_to_linear(g as f64 / 255.0),
            b: srgb_to_linear(b as f64 / 255.0),
            a: a as f64 / 255.0,
        }
    }

    /// Construct from a CSS hex string. Accepts `#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`
    /// (with or without the leading `#`). Hex digits are decoded as sRGB, then
    /// gamma-decoded to linear before storage.
    pub fn from_hex(hex: &str) -> Result<Self, &'static str> {
        let hex = hex.strip_prefix('#').unwrap_or(hex);
        let b = hex.as_bytes();

        fn nibble(byte: u8) -> Result<u8, &'static str> {
            match byte {
                b'0'..=b'9' => Ok(byte - b'0'),
                b'a'..=b'f' => Ok(byte - b'a' + 10),
                b'A'..=b'F' => Ok(byte - b'A' + 10),
                _ => Err("invalid hex digit"),
            }
        }

        fn pair(hi: u8, lo: u8) -> Result<u8, &'static str> {
            Ok(nibble(hi)? * 16 + nibble(lo)?)
        }

        match b.len() {
            3 => Ok(Self::from_srgb_u8(
                nibble(b[0])? * 17,
                nibble(b[1])? * 17,
                nibble(b[2])? * 17,
            )),
            4 => Ok(Self::from_srgba_u8(
                nibble(b[0])? * 17,
                nibble(b[1])? * 17,
                nibble(b[2])? * 17,
                nibble(b[3])? * 17,
            )),
            6 => Ok(Self::from_srgb_u8(
                pair(b[0], b[1])?,
                pair(b[2], b[3])?,
                pair(b[4], b[5])?,
            )),
            8 => Ok(Self::from_srgba_u8(
                pair(b[0], b[1])?,
                pair(b[2], b[3])?,
                pair(b[4], b[5])?,
                pair(b[6], b[7])?,
            )),
            _ => Err("invalid hex color length"),
        }
    }

    /// Convert to 8-bit sRGB + alpha `[r, g, b, a]`. Components are gamma-encoded
    /// and clamped to [0, 255].
    pub fn to_srgb_u8(&self) -> [u8; 4] {
        let enc = |c: f64| (linear_to_srgb(c.clamp(0.0, 1.0)) * 255.0).round() as u8;
        [
            enc(self.r),
            enc(self.g),
            enc(self.b),
            (self.a.clamp(0.0, 1.0) * 255.0).round() as u8,
        ]
    }

    /// Return a CSS lowercase hex string. Outputs `#rrggbb` when alpha is fully
    /// opaque, `#rrggbbaa` otherwise.
    pub fn to_hex_string(&self) -> String {
        let [r, g, b, a] = self.to_srgb_u8();
        if a == 255 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }

    /// Blend `self` toward `other` by `t` (0.0 = self, 1.0 = other), performing
    /// the interpolation in color space `S`.
    ///
    /// ```ignore
    /// // Perceptually smooth red-to-blue, no desaturation in the middle:
    /// let mid = red.blend_in::<Oklch>(&blue, 0.5);
    ///
    /// // Raw linear-light blend:
    /// let mid = red.blend_in::<Color>(&blue, 0.5);
    /// ```
    pub fn blend_in<S>(&self, other: &Color, t: f64) -> Color
    where
        S: ColorSpace + Interpolate,
    {
        S::from_color(self)
            .interpolate(&S::from_color(other), t)
            .to_color()
    }
}

/// `Color` is its own color space (linear light, identity conversion).
/// This lets you write `color.blend_in::<Color>(&other, t)` for a linear blend.
impl ColorSpace for Color {
    #[inline]
    fn to_color(&self) -> Color {
        *self
    }

    #[inline]
    fn from_color(color: &Color) -> Self {
        *color
    }
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f64) -> f64 {
    if c <= 0.0031308 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}
