use crate::colorspaces::{
    adobe_rgb::AdobeRgb, color::Color, display_p3::DisplayP3, hsl::Hsl, hsv::Hsv, hwb::Hwb,
    lab::Lab, lch::Lch, luv::Luv, oklab::Oklab, oklch::Oklch, rec2020::Rec2020, srgb::Srgb,
    xyz::Xyz,
};

/// Interpolation within a color space.
///
/// Implemented for all built-in color spaces. Cylindrical spaces (HSL, HSV, HWB,
/// Oklch, Lch) take the shortest path around the hue circle. Linear spaces
/// perform per-component lerp.
///
/// Use via [`Color::blend_in`].
pub trait Interpolate: Sized {
    fn interpolate(&self, other: &Self, t: f64) -> Self;
}

#[inline]
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn lerp_hue(a: f64, b: f64, t: f64) -> f64 {
    let mut diff = b - a;
    if diff > 180.0 {
        diff -= 360.0;
    } else if diff < -180.0 {
        diff += 360.0;
    }
    (a + diff * t).rem_euclid(360.0)
}

// --- Linear spaces ---

impl Interpolate for Color {
    fn interpolate(&self, other: &Self, t: f64) -> Self {
        Self {
            r: lerp(self.r, other.r, t),
            g: lerp(self.g, other.g, t),
            b: lerp(self.b, other.b, t),
            a: lerp(self.a, other.a, t),
        }
    }
}

impl Interpolate for Srgb {
    fn interpolate(&self, other: &Self, t: f64) -> Self {
        Self {
            r: lerp(self.r, other.r, t),
            g: lerp(self.g, other.g, t),
            b: lerp(self.b, other.b, t),
            a: lerp(self.a, other.a, t),
        }
    }
}

impl Interpolate for Oklab {
    fn interpolate(&self, other: &Self, t: f64) -> Self {
        Self {
            l: lerp(self.l, other.l, t),
            a: lerp(self.a, other.a, t),
            b: lerp(self.b, other.b, t),
            alpha: lerp(self.alpha, other.alpha, t),
        }
    }
}

impl Interpolate for Lab {
    fn interpolate(&self, other: &Self, t: f64) -> Self {
        Self {
            l: lerp(self.l, other.l, t),
            a: lerp(self.a, other.a, t),
            b: lerp(self.b, other.b, t),
            alpha: lerp(self.alpha, other.alpha, t),
        }
    }
}

impl Interpolate for Luv {
    fn interpolate(&self, other: &Self, t: f64) -> Self {
        Self {
            l: lerp(self.l, other.l, t),
            u: lerp(self.u, other.u, t),
            v: lerp(self.v, other.v, t),
            alpha: lerp(self.alpha, other.alpha, t),
        }
    }
}

impl Interpolate for Xyz {
    fn interpolate(&self, other: &Self, t: f64) -> Self {
        Self {
            x: lerp(self.x, other.x, t),
            y: lerp(self.y, other.y, t),
            z: lerp(self.z, other.z, t),
            alpha: lerp(self.alpha, other.alpha, t),
        }
    }
}

impl Interpolate for AdobeRgb {
    fn interpolate(&self, other: &Self, t: f64) -> Self {
        Self {
            r: lerp(self.r, other.r, t),
            g: lerp(self.g, other.g, t),
            b: lerp(self.b, other.b, t),
            a: lerp(self.a, other.a, t),
        }
    }
}

impl Interpolate for DisplayP3 {
    fn interpolate(&self, other: &Self, t: f64) -> Self {
        Self {
            r: lerp(self.r, other.r, t),
            g: lerp(self.g, other.g, t),
            b: lerp(self.b, other.b, t),
            a: lerp(self.a, other.a, t),
        }
    }
}

impl Interpolate for Rec2020 {
    fn interpolate(&self, other: &Self, t: f64) -> Self {
        Self {
            r: lerp(self.r, other.r, t),
            g: lerp(self.g, other.g, t),
            b: lerp(self.b, other.b, t),
            a: lerp(self.a, other.a, t),
        }
    }
}

// --- Cylindrical spaces — hue takes the shortest path around the circle ---

impl Interpolate for Hsl {
    fn interpolate(&self, other: &Self, t: f64) -> Self {
        Self {
            h: lerp_hue(self.h, other.h, t),
            s: lerp(self.s, other.s, t),
            l: lerp(self.l, other.l, t),
            a: lerp(self.a, other.a, t),
        }
    }
}

impl Interpolate for Hsv {
    fn interpolate(&self, other: &Self, t: f64) -> Self {
        Self {
            h: lerp_hue(self.h, other.h, t),
            s: lerp(self.s, other.s, t),
            v: lerp(self.v, other.v, t),
            a: lerp(self.a, other.a, t),
        }
    }
}

impl Interpolate for Hwb {
    fn interpolate(&self, other: &Self, t: f64) -> Self {
        Self {
            h: lerp_hue(self.h, other.h, t),
            w: lerp(self.w, other.w, t),
            b: lerp(self.b, other.b, t),
            a: lerp(self.a, other.a, t),
        }
    }
}

impl Interpolate for Oklch {
    fn interpolate(&self, other: &Self, t: f64) -> Self {
        Self {
            l: lerp(self.l, other.l, t),
            c: lerp(self.c, other.c, t),
            h: lerp_hue(self.h, other.h, t),
            alpha: lerp(self.alpha, other.alpha, t),
        }
    }
}

impl Interpolate for Lch {
    fn interpolate(&self, other: &Self, t: f64) -> Self {
        Self {
            l: lerp(self.l, other.l, t),
            c: lerp(self.c, other.c, t),
            h: lerp_hue(self.h, other.h, t),
            a: lerp(self.a, other.a, t),
        }
    }
}
