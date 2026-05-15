/// Selects how opaque geometry is rendered.
///
/// - `DeferredThenForward` (default): opaque objects via G-Buffer deferred; translucent
///   objects via `OitPass` if enabled. Best quality and scalability.
/// - `ForwardOnly`: all opaque objects rendered with the forward-lit shader in one pass;
///   no G-Buffer, no deferred lighting. Useful as a compatibility fallback on hardware
///   that does not support multiple render targets.
///
/// Set via `DeferredPass::render_path`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum RenderPath {
    #[default]
    DeferredThenForward,
    ForwardOnly,
}

/// Procedural sky configuration.
///
/// The sky is rendered as part of the deferred lighting pass: background pixels
/// (those with no geometry, i.e. depth = 1.0) receive a physically-inspired sky
/// colour derived from the scene's directional light direction. No extra pass needed.
///
/// `SkyConfig::default()` gives a clear midday sky. Set on `DeferredPass` via
/// `deferred.sky = SkyConfig { turbidity: 4.0, ..Default::default() }`.
#[derive(Clone, Debug)]
pub struct SkyConfig {
    /// Atmospheric turbidity [1, 10]. 1 = crystal clear, 10 = heavy haze.
    /// Default 2.5.
    pub turbidity: f32,
    /// Overall sky exposure multiplier. Default 1.0.
    pub exposure: f32,
    /// Sun disc angular radius in degrees. Default 0.53 (physically correct).
    pub sun_size_deg: f32,
    /// Enable sky rendering for background pixels. Default true.
    pub enabled: bool,
}

impl Default for SkyConfig {
    fn default() -> Self {
        Self {
            turbidity: 2.5,
            exposure: 1.0,
            sun_size_deg: 0.53,
            enabled: true,
        }
    }
}
