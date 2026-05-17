// Post-processing stack — config types and shader passes (Track Post).
//
// Config types follow Law 1: every struct's `Default::default()` is
// production-quality. Effects are disabled by default; set `enabled: true`
// to activate them.
//
// ## Execution order (roadmap Track Post)
//
// `GTAO → GI → Fog → Motion Blur → Bloom → DoF → Upscale → Tone Map →
//  **Grain → Vignette → CA** → Output`
//
// The passes in this file run after tone mapping. Each takes an input
// `GraphImage`, applies the effect, and returns a new `GraphImage` registered
// in the frame under a stable name. Disabled passes return the input unchanged
// (no GPU work, O(1) clone).
//
// ## Typical usage
//
// ```ignore
// // In init:
// let pp = PostProcessPasses::new(&engine)?;
// let pp_config = PostProcessConfig {
//     grain:     GrainConfig     { enabled: true,  intensity: 0.04, ..Default::default() },
//     vignette:  VignetteConfig  { enabled: true,  intensity: 0.35, ..Default::default() },
//     chromatic_aberration: CaConfig { enabled: false, ..Default::default() },
//     ..Default::default()
// };
//
// // In render (after tone mapping):
// let tonemapped: GraphImage = /* your tonemapped output */;
// let final_image = pp.execute(&tonemapped, frame, &pp_config, frame_time_secs)?;
// ```

use crate::push_constants;
use crate::{Engine, GraphImage, RenderFrame, Result, ShaderProgram, StageMask};

// ── Config types ──────────────────────────────────────────────────────────────

/// Reserved auto-exposure (eye adaptation) configuration.
///
/// The public fields match the planned luminance-histogram implementation, but
/// the runtime pass is not executable until the engine has a reduction/history
/// resource for scene luminance. `PostProcessPasses::execute` returns
/// `Error::Unsupported` when this is enabled.
///
#[derive(Clone, Debug)]
pub struct AutoExposureConfig {
    pub enabled: bool,
    /// Minimum allowed exposure value (EV100). Default: −2.0 (dim indoor).
    pub min_ev: f32,
    /// Maximum allowed exposure value (EV100). Default: 8.0 (bright outdoor).
    pub max_ev: f32,
    /// Histogram percentile used to derive the metered exposure. Default: 0.5
    /// (median). Lower values expose for highlights; higher for shadows.
    pub target_percentile: f32,
    /// How quickly exposure adapts toward the target, in EV100/second. Default: 1.5.
    pub adaptation_speed: f32,
}

impl Default for AutoExposureConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_ev: -2.0,
            max_ev: 8.0,
            target_percentile: 0.5,
            adaptation_speed: 1.5,
        }
    }
}

/// Film grain configuration.
///
/// Adds animated per-pixel noise after tone mapping. Disabled by default.
/// Applied at display resolution so the grain does not interact with TAA.
#[derive(Clone, Debug)]
pub struct GrainConfig {
    pub enabled: bool,
    /// Noise magnitude [0, 1]. Default: 0.04.
    pub intensity: f32,
    /// Grain texel size in display pixels. 1.0 = single-pixel noise,
    /// larger values produce coarser grain. Default: 1.5.
    pub size: f32,
    /// `true` = RGB-independent grain (more organic look).
    /// `false` = monochrome grain. Default: false.
    pub colored: bool,
}

impl Default for GrainConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            intensity: 0.04,
            size: 1.5,
            colored: false,
        }
    }
}

/// Screen-edge vignette configuration.
///
/// Darkens pixels toward the corners of the display, simulating the
/// light fall-off of a physical lens. Disabled by default.
#[derive(Clone, Debug)]
pub struct VignetteConfig {
    pub enabled: bool,
    /// Darkening strength at the corners [0, 1]. Default: 0.35.
    pub intensity: f32,
    /// Normalised distance from the centre at which darkening begins [0, 1].
    /// 0.0 = darkening starts at the centre; 1.0 = only at the very corner.
    /// Default: 0.55.
    pub radius: f32,
    /// Width of the smooth transition beyond `radius` [0, 1]. Default: 0.35.
    pub feather: f32,
}

impl Default for VignetteConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            intensity: 0.35,
            radius: 0.55,
            feather: 0.35,
        }
    }
}

/// Chromatic aberration configuration.
///
/// Separates RGB channels radially toward screen edges, simulating the
/// wavelength-dependent refraction of a physical camera lens. Disabled by
/// default (strength: 0.003 — a subtle, perceptually plausible amount).
#[derive(Clone, Debug)]
pub struct CaConfig {
    pub enabled: bool,
    /// Channel separation magnitude at the screen edge [0, ~0.015].
    /// 0.003 is a realistic value for a quality prime lens. Default: 0.003.
    pub strength: f32,
    /// Exponent controlling the edge-weight curve. 2.0 = quadratic (effect is
    /// negligible near centre, stronger at edges). Default: 2.0.
    pub radial_falloff: f32,
}

impl Default for CaConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strength: 0.003,
            radial_falloff: 2.0,
        }
    }
}

/// Lens dirt and flare configuration.
///
/// Applies a dirt/grime texture mask over the bloom result to simulate
/// lens imperfections. Disabled by default.
#[derive(Clone, Debug)]
pub struct LensConfig {
    /// Multiplier for the dirt overlay. Default: 0.0 (no dirt).
    pub dirt_strength: f32,
    /// Enable a simple lens flare simulation. Default: false.
    pub flare_enabled: bool,
    /// Flare intensity multiplier used when `flare_enabled` is true. Default: 0.25.
    pub flare_strength: f32,
}

impl LensConfig {
    pub fn enabled(&self) -> bool {
        self.dirt_strength > 0.0 || self.flare_enabled
    }
}

impl Default for LensConfig {
    fn default() -> Self {
        Self {
            dirt_strength: 0.0,
            flare_enabled: false,
            flare_strength: 0.25,
        }
    }
}

/// Unified post-processing configuration.
///
/// Holds the configuration for all post-effects that run after tone mapping.
/// `Default::default()` disables all optional effects and is production-safe.
///
/// # Execution order (roadmap Track Post)
/// `Bloom → DoF → Upscale → Tone Map → **Grain → Vignette → CA** → Output`
///
/// ```ignore
/// let config = PostProcessConfig {
///     grain:    GrainConfig    { enabled: true, intensity: 0.04, ..Default::default() },
///     vignette: VignetteConfig { enabled: true, ..Default::default() },
///     ..Default::default()
/// };
/// ```
#[derive(Clone, Debug, Default)]
pub struct PostProcessConfig {
    /// Eye-adaptation auto-exposure (reserved; enabling returns `Unsupported`).
    pub auto_exposure: AutoExposureConfig,
    /// Animated film grain added after tone mapping.
    pub grain: GrainConfig,
    /// Screen-edge vignette darkening.
    pub vignette: VignetteConfig,
    /// RGB channel separation toward screen edges.
    pub chromatic_aberration: CaConfig,
    /// Lens dirt / flare overlay.
    pub lens: LensConfig,
}

// ── Push-constant layouts ─────────────────────────────────────────────────────

#[push_constants]
#[derive(Debug, Default)]
struct VignetteConstants {
    intensity: f32,
    radius: f32,
    feather: f32,
    _pad: f32,
}

#[push_constants]
#[derive(Debug, Default)]
struct GrainConstants {
    intensity: f32,
    size: f32,
    time: f32,
    colored: u32,
}

#[push_constants]
#[derive(Debug, Default)]
struct CaConstants {
    strength: f32,
    radial_falloff: f32,
    _pad: [f32; 2],
}

#[push_constants]
#[derive(Debug, Default)]
struct LensConstants {
    dirt_strength: f32,
    flare_enabled: u32,
    flare_strength: f32,
    _pad: f32,
}

// ── Individual passes ─────────────────────────────────────────────────────────

/// Vignette post-processing pass.
///
/// Darkens the corners of the screen using a smooth radial ramp. Backed by
/// `vignette.slang` (embedded at compile time). Create once, reuse every frame.
pub struct VignettePass {
    program: ShaderProgram,
}

impl VignettePass {
    /// Create the pass, compiling the built-in vignette shader.
    pub fn new(engine: &Engine) -> Result<Self> {
        let program = ShaderProgram::from_inline_fragment(
            engine,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/shaders/vignette.slang"
            )),
        )?;
        Ok(Self { program })
    }

    /// Apply vignette to `input` and return the result.
    ///
    /// If `config.enabled` is `false` the input is returned unchanged with
    /// no GPU work performed.
    ///
    /// The output image is registered in `frame` as `"vignette_output"`.
    pub fn execute(
        &self,
        input: &GraphImage,
        frame: &RenderFrame,
        config: &VignetteConfig,
    ) -> Result<GraphImage> {
        if !config.enabled {
            return Ok(input.clone());
        }
        input.register_as("vignette_input");
        let output = frame.image_sized_to("vignette_output", input.desc().format, input)?;
        let constants = VignetteConstants {
            intensity: config.intensity,
            radius: config.radius,
            feather: config.feather,
            _pad: 0.0,
        };
        output.execute_shader_with_push_constants(
            &self.program,
            StageMask::FRAGMENT,
            bytemuck::bytes_of(&constants),
        )?;
        Ok(output)
    }
}

/// Film grain post-processing pass.
///
/// Adds animated per-pixel noise after tone mapping. Backed by
/// `film_grain.slang` (embedded at compile time).
pub struct GrainPass {
    program: ShaderProgram,
}

impl GrainPass {
    /// Create the pass, compiling the built-in film grain shader.
    pub fn new(engine: &Engine) -> Result<Self> {
        let program = ShaderProgram::from_inline_fragment(
            engine,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/shaders/film_grain.slang"
            )),
        )?;
        Ok(Self { program })
    }

    /// Apply film grain to `input` and return the result.
    ///
    /// `time_secs` is a monotonically-increasing frame time (e.g.
    /// `frame_clock.elapsed().as_secs_f32()`). It drives the grain animation
    /// so each frame shows a different noise pattern.
    ///
    /// The output image is registered in `frame` as `"grain_output"`.
    pub fn execute(
        &self,
        input: &GraphImage,
        frame: &RenderFrame,
        config: &GrainConfig,
        time_secs: f32,
    ) -> Result<GraphImage> {
        if !config.enabled {
            return Ok(input.clone());
        }
        input.register_as("grain_input");
        let output = frame.image_sized_to("grain_output", input.desc().format, input)?;
        let constants = GrainConstants {
            intensity: config.intensity,
            size: config.size,
            time: time_secs,
            colored: config.colored as u32,
        };
        output.execute_shader_with_push_constants(
            &self.program,
            StageMask::FRAGMENT,
            bytemuck::bytes_of(&constants),
        )?;
        Ok(output)
    }
}

/// Chromatic aberration post-processing pass.
///
/// Separates RGB channels radially toward screen edges. Backed by
/// `chromatic_aberration.slang` (embedded at compile time).
pub struct CaPass {
    program: ShaderProgram,
}

/// Lens dirt and flare post-processing pass.
///
/// Adds a procedural dirt mask and simple display-space flare highlights after
/// tone mapping. Backed by `lens_dirt_flare.slang`.
pub struct LensPass {
    program: ShaderProgram,
}

impl LensPass {
    pub fn new(engine: &Engine) -> Result<Self> {
        let program = ShaderProgram::from_inline_fragment(
            engine,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/shaders/lens_dirt_flare.slang"
            )),
        )?;
        Ok(Self { program })
    }

    pub fn execute(
        &self,
        input: &GraphImage,
        frame: &RenderFrame,
        config: &LensConfig,
    ) -> Result<GraphImage> {
        if !config.enabled() {
            return Ok(input.clone());
        }
        input.register_as("lens_input");
        let output = frame.image_sized_to("lens_output", input.desc().format, input)?;
        let constants = LensConstants {
            dirt_strength: config.dirt_strength.max(0.0),
            flare_enabled: config.flare_enabled as u32,
            flare_strength: config.flare_strength.max(0.0),
            _pad: 0.0,
        };
        output.execute_shader_with_push_constants(
            &self.program,
            StageMask::FRAGMENT,
            bytemuck::bytes_of(&constants),
        )?;
        Ok(output)
    }
}

impl CaPass {
    /// Create the pass, compiling the built-in CA shader.
    pub fn new(engine: &Engine) -> Result<Self> {
        let program = ShaderProgram::from_inline_fragment(
            engine,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/shaders/chromatic_aberration.slang"
            )),
        )?;
        Ok(Self { program })
    }

    /// Apply chromatic aberration to `input` and return the result.
    ///
    /// The output image is registered in `frame` as `"ca_output"`.
    pub fn execute(
        &self,
        input: &GraphImage,
        frame: &RenderFrame,
        config: &CaConfig,
    ) -> Result<GraphImage> {
        if !config.enabled {
            return Ok(input.clone());
        }
        input.register_as("ca_input");
        let output = frame.image_sized_to("ca_output", input.desc().format, input)?;
        let constants = CaConstants {
            strength: config.strength,
            radial_falloff: config.radial_falloff,
            _pad: [0.0; 2],
        };
        output.execute_shader_with_push_constants(
            &self.program,
            StageMask::FRAGMENT,
            bytemuck::bytes_of(&constants),
        )?;
        Ok(output)
    }
}

// ── PostProcessPasses — convenience bundle ────────────────────────────────────

/// All post-processing passes bundled together.
///
/// Create once at init time and store alongside the render state. Reuse every
/// frame by calling `execute`.
///
/// ```ignore
/// let pp = PostProcessPasses::new(&engine)?;
/// // each frame, after tone mapping:
/// let final_image = pp.execute(&tonemapped, frame, &config, frame_time)?;
/// ```
pub struct PostProcessPasses {
    pub grain: GrainPass,
    pub vignette: VignettePass,
    pub ca: CaPass,
    pub lens: LensPass,
}

impl PostProcessPasses {
    /// Create all passes, compiling their built-in shaders.
    pub fn new(engine: &Engine) -> Result<Self> {
        Ok(Self {
            grain: GrainPass::new(engine)?,
            vignette: VignettePass::new(engine)?,
            ca: CaPass::new(engine)?,
            lens: LensPass::new(engine)?,
        })
    }

    /// Apply the full post-process stack in roadmap order:
    /// **Lens → Grain → Vignette → CA**
    ///
    /// Each disabled pass is a no-op (no GPU work, O(1) clone).
    /// Returns the final output image ready for presentation.
    pub fn execute(
        &self,
        input: &GraphImage,
        frame: &RenderFrame,
        config: &PostProcessConfig,
        time_secs: f32,
    ) -> Result<GraphImage> {
        if config.auto_exposure.enabled {
            return Err(crate::Error::Unsupported(
                "auto-exposure requires luminance histogram/reduction support that is not implemented yet",
            ));
        }
        let after_lens = self.lens.execute(input, frame, &config.lens)?;
        let after_grain = self
            .grain
            .execute(&after_lens, frame, &config.grain, time_secs)?;
        let after_vignette = self
            .vignette
            .execute(&after_grain, frame, &config.vignette)?;
        let after_ca = self
            .ca
            .execute(&after_vignette, frame, &config.chromatic_aberration)?;
        Ok(after_ca)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lens_config_defaults_to_disabled() {
        let config = LensConfig::default();

        assert!(!config.enabled());
        assert_eq!(config.dirt_strength, 0.0);
        assert!(!config.flare_enabled);
        assert_eq!(config.flare_strength, 0.25);
    }

    #[test]
    fn lens_config_enables_for_dirt_or_flare() {
        assert!(
            LensConfig {
                dirt_strength: 0.1,
                ..Default::default()
            }
            .enabled()
        );
        assert!(
            LensConfig {
                flare_enabled: true,
                ..Default::default()
            }
            .enabled()
        );
    }

    #[test]
    fn auto_exposure_documents_reserved_runtime_behavior() {
        let config = AutoExposureConfig {
            enabled: true,
            ..Default::default()
        };

        assert!(config.enabled);
    }
}
