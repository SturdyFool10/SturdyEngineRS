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
    Aces,
    /// Simple Reinhard tone mapping.
    Reinhard,
    /// Hermite spline tone mapping — smooth, perceptually-friendly curve with
    /// no harsh clipping at highlights.
    #[default]
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
    /// PsychoV-11 by clshortfuse/RenoDX — perceptual tonemapper built on the
    /// Stockman-Sharp LMS cone model with MacLeod-Boynton hue geometry.
    /// Supports configurable highlights/shadows, saturation (purity), highlight
    /// bleaching, hue restoration, adaptation contrast, and cone response shaping.
    /// Reference: <https://github.com/clshortfuse/renodx>
    PsychoV11,
    /// PsychoV-17 by clshortfuse/RenoDX — updated perceptual tonemapper with
    /// adaptive-relative weighted LMS, CIE 170-2 gamut boundary for hue signals,
    /// Naka-Rushton with separate adaptive/background anchors, and adaptive gamut
    /// compression against the CIE 1702 human gamut boundary.
    /// Reference: <https://github.com/clshortfuse/renodx>
    PsychoV17,
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
            ToneMappingOp::Hermite
        };

        Ok(Self { mode, tone_mapping })
    }
}

#[cfg(test)]
#[path = "hdr_pipeline_tests.rs"]
mod tests;
