use crate::{Format, SurfaceColorSpace};

/// HDR output preference for swapchain creation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum SurfaceHdrPreference {
    /// Use standard dynamic range (SRGB non-linear or similar).
    #[default]
    Sdr,
    /// Prefer HDR10 / ST.2084 PQ if the display and backend support it.
    Hdr10,
    /// Prefer scRGB / extended-linear SRGB (suitable for HDR on Windows).
    ScRgb,
}

impl SurfaceHdrPreference {
    /// Returns the preferred (format, color_space) pairs for this HDR mode,
    /// in priority order.  The backend should pick the first pair it supports.
    pub fn preferred_formats(&self) -> &'static [(Format, SurfaceColorSpace)] {
        match self {
            Self::Sdr => &[
                (Format::Bgra8Unorm, SurfaceColorSpace::SrgbNonlinear),
                (Format::Rgba8Unorm, SurfaceColorSpace::SrgbNonlinear),
            ],
            Self::Hdr10 => &[
                (Format::Rgba16Float, SurfaceColorSpace::Hdr10St2084),
                (Format::Rgba32Float, SurfaceColorSpace::Hdr10St2084),
                // Fall back to SDR if HDR10 is unavailable.
                (Format::Bgra8Unorm, SurfaceColorSpace::SrgbNonlinear),
            ],
            Self::ScRgb => &[
                (Format::Rgba16Float, SurfaceColorSpace::ExtendedSrgbLinear),
                // Fall back to SDR.
                (Format::Bgra8Unorm, SurfaceColorSpace::SrgbNonlinear),
            ],
        }
    }
}
