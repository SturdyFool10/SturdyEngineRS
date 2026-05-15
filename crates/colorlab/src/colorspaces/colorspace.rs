/// Trait for converting between color spaces.
/// Types implementing this trait can convert to and from the central `Color` type.
use crate::colorspaces::color::Color;

pub trait ColorSpace: Sized {
    /// Convert from this color space to the central `Color` type.
    fn to_color(&self) -> Color;
    /// Convert from the central `Color` type to this color space.
    fn from_color(color: &Color) -> Self;
}
