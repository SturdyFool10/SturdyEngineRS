// Tests extracted from crates/sturdy-engine-core/src/surface/surface_hdr_caps.rs
// Runtime code should stay separate from test code.

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
        hdr_metadata_supported: false,
    };

    let hdr = SurfaceHdrCaps::from_surface_capabilities(&caps);

    assert!(hdr.hdr10);
    assert!(hdr.sc_rgb);
}
