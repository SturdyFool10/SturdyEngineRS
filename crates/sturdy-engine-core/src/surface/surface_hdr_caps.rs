use crate::{SurfaceCapabilities, SurfaceColorSpace};

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct SurfaceHdrCaps {
    pub hdr10: bool,
    pub sc_rgb: bool,
}

impl SurfaceHdrCaps {
    pub fn from_surface_capabilities(capabilities: &SurfaceCapabilities) -> Self {
        let mut hdr = Self::default();
        for format in &capabilities.formats {
            match format.color_space {
                SurfaceColorSpace::Hdr10St2084 | SurfaceColorSpace::Hdr10Hlg => {
                    hdr.hdr10 = true;
                }
                SurfaceColorSpace::ExtendedSrgbLinear => {
                    hdr.sc_rgb = true;
                }
                SurfaceColorSpace::SrgbNonlinear
                | SurfaceColorSpace::DisplayP3Nonlinear
                | SurfaceColorSpace::Unknown => {}
            }
        }
        hdr
    }
}

#[cfg(test)]
#[path = "surface_hdr_caps_tests.rs"]
mod tests;
