use crate::colorspaces::color::Color;
use crate::colorspaces::colorspace::ColorSpace;
use serde::{Deserialize, Serialize};

/// CIE XYZ with Observer=2°, Illuminant=D65
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Xyz {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub alpha: f64,
}

impl ColorSpace for Xyz {
    fn to_color(&self) -> Color {
        let x = self.x;
        let y = self.y;
        let z = self.z;
        let r = 3.2406 * x - 1.5372 * y - 0.4986 * z;
        let g = -0.9689 * x + 1.8758 * y + 0.0415 * z;
        let b = 0.0557 * x - 0.2040 * y + 1.0570 * z;
        Color::new(r, g, b, self.alpha)
    }

    fn from_color(c: &Color) -> Self {
        let r = c.r;
        let g = c.g;
        let b = c.b;
        let x = 0.4124 * r + 0.3576 * g + 0.1805 * b;
        let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let z = 0.0193 * r + 0.1192 * g + 0.9505 * b;
        Xyz {
            x,
            y,
            z,
            alpha: c.a,
        }
    }
}
