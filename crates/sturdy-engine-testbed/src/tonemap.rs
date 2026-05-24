use sturdy_engine::{ToneMappingOp, push_constants};

/// CPU mirror of `shaders/tonemap.slang::TonemapConstants`.
///
/// Keep this struct in lockstep with the shader push-constant layout. Using one
/// shared definition keeps the sample binaries from silently drifting when the
/// tonemap shader gains new parameters.
#[push_constants]
#[derive(Debug)]
pub struct TonemapParams {
    pub tonemap_op: u32,
    pub hdr_output: u32,
    pub exposure: f32,
    pub white_point: f32,
    pub display_gain: f32,
    pub output_gamma: f32,
    pub aces_a: f32,
    pub aces_b: f32,
    pub aces_c: f32,
    pub aces_d: f32,
    pub aces_e: f32,
    pub reinhard_white: f32,
    pub hermite_contrast: f32,
    pub linear_white: f32,
    pub psycho_peak_value: f32,
    pub psycho_highlights: f32,
    pub psycho_shadows: f32,
    pub psycho_contrast_high: f32,
    pub psycho_contrast_low: f32,
    pub psycho_purity: f32,
    pub psycho_bleaching: f32,
    pub psycho_hue_restore: f32,
    pub psycho_adapt_contrast: f32,
    pub psycho_cone_exp: f32,
}

impl Default for TonemapParams {
    fn default() -> Self {
        Self {
            tonemap_op: tone_mapping_id(ToneMappingOp::Aces),
            hdr_output: 0,
            exposure: 1.0,
            white_point: 4.0,
            display_gain: 1.0,
            output_gamma: 2.2,
            aces_a: 2.51,
            aces_b: 0.03,
            aces_c: 2.43,
            aces_d: 0.59,
            aces_e: 0.14,
            reinhard_white: 4.0,
            hermite_contrast: 1.55,
            linear_white: 1.25,
            // 1000 nits / 203 nits (diffuse white reference) = canonical SDR peak.
            psycho_peak_value: 1000.0 / 203.0,
            psycho_highlights: 1.0,
            psycho_shadows: 1.0,
            psycho_contrast_high: 1.0,
            psycho_contrast_low: 1.0,
            psycho_purity: 1.0,
            psycho_bleaching: 0.0,
            psycho_hue_restore: 1.0,
            psycho_adapt_contrast: 1.0,
            psycho_cone_exp: 1.0,
        }
    }
}

pub fn tone_mapping_id(op: ToneMappingOp) -> u32 {
    match op {
        ToneMappingOp::Aces => 0,
        ToneMappingOp::Reinhard => 1,
        ToneMappingOp::Hermite => 2,
        ToneMappingOp::Linear => 3,
        ToneMappingOp::PbrNeutral => 4,
        ToneMappingOp::AgX => 5,
        ToneMappingOp::PsychoV11 => 6,
        ToneMappingOp::PsychoV17 => 7,
    }
}
