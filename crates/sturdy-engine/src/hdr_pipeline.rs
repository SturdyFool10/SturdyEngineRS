use crate::{Caps, Format, SurfaceHdrCaps};

/// The HDR output mode selected for a surface.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum HdrMode {
    /// HDR10 PQ (Rec.2020 color space, ST.2084 transfer function).
    Hdr10,
    /// scRGB linear (extended sRGB, suitable for HDR on Windows).
    ScRgb,
    /// Standard dynamic range SDR fallback.
    Sdr,
}

impl HdrMode {
    /// The render-target format that should be used for this mode.
    pub fn render_format(self) -> Format {
        match self {
            Self::Hdr10 | Self::ScRgb => Format::Rgba16Float,
            Self::Sdr => Format::Rgba8Unorm,
        }
    }

    /// Returns `true` when this mode uses a wide-gamut / HDR pipeline.
    pub fn is_hdr(self) -> bool {
        matches!(self, Self::Hdr10 | Self::ScRgb)
    }
}

/// How the engine should prefer HDR output.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum HdrPreference {
    /// Use HDR if the display supports it; otherwise fall back to SDR.
    #[default]
    PreferHdr,
    /// Always use SDR regardless of display capabilities.
    ForceSdr,
    /// Fail if HDR is not available.
    RequireHdr,
}

/// The tone-mapping algorithm applied when converting the HDR render buffer to
/// the swapchain image.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum ToneMappingOp {
    /// ACES filmic tone mapping (industry standard for HDR → SDR).
    #[default]
    Aces,
    /// Simple Reinhard tone mapping.
    Reinhard,
    /// Hermite spline tone mapping — smooth, perceptually-friendly curve with
    /// no harsh clipping at highlights.
    Hermite,
    /// Pass-through: no tone mapping applied (for HDR displays).
    Linear,
    /// Khronos PBR Neutral — identity below 0.76 nit, smooth highlight
    /// compression above, subtle desaturation at peak. Designed to preserve
    /// PBR material colours without shifting hue in the midtones.
    /// Reference: <https://github.com/KhronosGroup/ToneMapping>
    PbrNeutral,
    /// AgX by Troy Sobotka (Blender default since 3.x). Transforms to a
    /// log-normalised AgX working space, applies a per-channel sigmoid, then
    /// maps back. Keeps saturated colours from clipping and avoids the
    /// over-contrasty shoulder of ACES.
    AgX,
}

/// Describes the HDR rendering pipeline configuration for a surface.
#[derive(Clone, Debug)]
pub struct HdrPipelineDesc {
    pub mode: HdrMode,
    pub tone_mapping: ToneMappingOp,
}

impl HdrPipelineDesc {
    /// Select the best HDR configuration for the surface and device.
    pub fn select(
        hdr_caps: &SurfaceHdrCaps,
        device_caps: &Caps,
        preference: HdrPreference,
    ) -> crate::Result<Self> {
        let mode = match preference {
            HdrPreference::ForceSdr => HdrMode::Sdr,
            HdrPreference::PreferHdr | HdrPreference::RequireHdr => {
                let hdr_possible = (hdr_caps.sc_rgb || hdr_caps.hdr10)
                    && (device_caps.features.image_fp16_render
                        || device_caps.features.image_fp32_render);

                if hdr_possible {
                    if hdr_caps.sc_rgb {
                        HdrMode::ScRgb
                    } else {
                        HdrMode::Hdr10
                    }
                } else if preference == HdrPreference::RequireHdr {
                    return Err(crate::Error::Unsupported(
                        "HDR output is not available on this display or device",
                    ));
                } else {
                    HdrMode::Sdr
                }
            }
        };

        let tone_mapping = if mode.is_hdr() {
            ToneMappingOp::Linear
        } else {
            ToneMappingOp::Aces
        };

        Ok(Self { mode, tone_mapping })
    }
}

#[cfg(test)]
mod tests {
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
            HdrPipelineDesc::select(&hdr_caps_both(), &caps_fp16(), HdrPreference::PreferHdr)
                .unwrap();
        assert_eq!(desc.mode, HdrMode::ScRgb);
        assert!(desc.mode.is_hdr());
    }

    #[test]
    fn force_sdr_ignores_display_caps() {
        let desc = HdrPipelineDesc::select(&hdr_caps_both(), &caps_fp16(), HdrPreference::ForceSdr)
            .unwrap();
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
            HdrPipelineDesc::select(&hdr_caps_both(), &caps_fp16(), HdrPreference::PreferHdr)
                .unwrap();
        assert_eq!(desc.tone_mapping, ToneMappingOp::Linear);
    }

    #[test]
    fn sdr_pipeline_uses_aces_tone_mapping() {
        let desc =
            HdrPipelineDesc::select(&hdr_caps_none(), &Caps::default(), HdrPreference::PreferHdr)
                .unwrap();
        assert_eq!(desc.tone_mapping, ToneMappingOp::Aces);
    }
}
