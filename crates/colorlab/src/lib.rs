#![allow(dead_code)]

pub mod colorspaces;

pub use colorspaces::adobe_rgb::AdobeRgb;
pub use colorspaces::color::Color;
pub use colorspaces::colorspace::ColorSpace;
pub use colorspaces::display_p3::DisplayP3;
pub use colorspaces::hsl::Hsl;
pub use colorspaces::hsv::Hsv;
pub use colorspaces::hwb::Hwb;
pub use colorspaces::interpolate::Interpolate;
pub use colorspaces::lab::Lab;
pub use colorspaces::lch::Lch;
pub use colorspaces::luv::Luv;
pub use colorspaces::oklab::Oklab;
pub use colorspaces::oklch::Oklch;
pub use colorspaces::rec2020::Rec2020;
pub use colorspaces::srgb::Srgb;
pub use colorspaces::xyz::Xyz;

#[cfg(test)]
mod tests;

