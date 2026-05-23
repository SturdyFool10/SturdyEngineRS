// Tests extracted from crates/sturdy-engine/src/hdr_pipeline.rs
// Runtime code should stay separate from test code.

use super::*;
use crate::Caps;

fn hdr_caps_both() -> SurfaceHdrCaps {
    SurfaceHdrCaps {
        hdr10: true,
        sc_rgb: true,
    }
}

fn hdr_caps_none() -> SurfaceHdrCaps {
    SurfaceHdrCaps::default()
}

fn caps_fp16() -> Caps {
    use sturdy_engine_core::BackendFeatures;
    Caps {
        features: BackendFeatures {
            image_fp16_render: true,
            ..BackendFeatures::default()
        },
        ..Caps::default()
    }
}

#[test]
fn prefer_hdr_selects_sc_rgb_when_available() {
    let desc =
        HdrPipelineDesc::select(&hdr_caps_both(), &caps_fp16(), HdrPreference::PreferHdr).unwrap();
    assert_eq!(desc.mode, HdrMode::ScRgb);
    assert!(desc.mode.is_hdr());
}

#[test]
fn force_sdr_ignores_display_caps() {
    let desc =
        HdrPipelineDesc::select(&hdr_caps_both(), &caps_fp16(), HdrPreference::ForceSdr).unwrap();
    assert_eq!(desc.mode, HdrMode::Sdr);
    assert!(!desc.mode.is_hdr());
}

#[test]
fn prefer_hdr_falls_back_to_sdr_when_unsupported() {
    let desc =
        HdrPipelineDesc::select(&hdr_caps_none(), &Caps::default(), HdrPreference::PreferHdr)
            .unwrap();
    assert_eq!(desc.mode, HdrMode::Sdr);
}

#[test]
fn require_hdr_errors_when_unsupported() {
    let result = HdrPipelineDesc::select(
        &hdr_caps_none(),
        &Caps::default(),
        HdrPreference::RequireHdr,
    );
    assert!(result.is_err());
}

#[test]
fn hdr_mode_render_format_is_fp16_for_hdr() {
    assert_eq!(HdrMode::ScRgb.render_format(), crate::Format::Rgba16Float);
    assert_eq!(HdrMode::Hdr10.render_format(), crate::Format::Rgba16Float);
    assert_eq!(HdrMode::Sdr.render_format(), crate::Format::Rgba8Unorm);
}

#[test]
fn hdr_pipeline_uses_linear_tone_mapping_in_hdr_mode() {
    let desc =
        HdrPipelineDesc::select(&hdr_caps_both(), &caps_fp16(), HdrPreference::PreferHdr).unwrap();
    assert_eq!(desc.tone_mapping, ToneMappingOp::Linear);
}

#[test]
fn sdr_pipeline_uses_hermite_tone_mapping() {
    let desc =
        HdrPipelineDesc::select(&hdr_caps_none(), &Caps::default(), HdrPreference::PreferHdr)
            .unwrap();
    assert_eq!(desc.tone_mapping, ToneMappingOp::Hermite);
}
