/// SMPTE ST 2086 / CTA 861.3 HDR mastering display metadata.
///
/// Set on a surface via `Device::set_surface_hdr_metadata` when
/// `BackendFeatures::hdr_output` is true and the swapchain is in an HDR color
/// space.  The display uses this to correctly tone-map the HDR signal.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct HdrMetadata {
    /// Display primary chromaticity coordinates [R, G, B] as (x, y) pairs in
    /// the CIE 1931 xy colour space. Each component is in [0.0, 1.0].
    pub display_primaries: [[f32; 2]; 3],
    /// White point chromaticity as (x, y) in CIE 1931. Each in [0.0, 1.0].
    pub white_point: [f32; 2],
    /// Maximum luminance of the mastering display in nits (cd/m²).
    pub max_luminance: f32,
    /// Minimum luminance of the mastering display in nits (cd/m²).
    pub min_luminance: f32,
    /// Maximum content light level — peak luminance across all frames (nits).
    pub max_content_light_level: f32,
    /// Maximum frame average light level — highest per-frame average (nits).
    pub max_frame_average_light_level: f32,
}

impl HdrMetadata {
    /// Typical values for an HDR10 mastering display (DCI-P3 primaries,
    /// D65 white point, HDR600 luminance range).
    pub fn hdr10_typical() -> Self {
        Self {
            display_primaries: [
                [0.680, 0.320], // R
                [0.265, 0.690], // G
                [0.150, 0.060], // B
            ],
            white_point: [0.3127, 0.3290], // D65
            max_luminance: 1000.0,
            min_luminance: 0.001,
            max_content_light_level: 1000.0,
            max_frame_average_light_level: 400.0,
        }
    }
}
