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
mod tests {
    use super::*;
    use crate::{Format, SurfaceFormatInfo, SurfacePresentMode};

    #[test]
    fn hdr_caps_detect_hdr10_and_scrgb_color_spaces() {
        let caps = SurfaceCapabilities {
            formats: vec![
                SurfaceFormatInfo {
                    format: Format::Rgba16Float,
                    color_space: SurfaceColorSpace::Hdr10St2084,
                },
                SurfaceFormatInfo {
                    format: Format::Rgba16Float,
                    color_space: SurfaceColorSpace::ExtendedSrgbLinear,
                },
            ],
            present_modes: vec![SurfacePresentMode::Fifo],
            min_image_count: 2,
            max_image_count: 0,
            current_width: 0,
            current_height: 0,
        };

        let hdr = SurfaceHdrCaps::from_surface_capabilities(&caps);

        assert!(hdr.hdr10);
        assert!(hdr.sc_rgb);
    }
}
