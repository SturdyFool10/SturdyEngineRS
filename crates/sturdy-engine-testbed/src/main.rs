use std::{path::PathBuf, time::Instant};

mod cornell_rt;
mod overlay;
mod path_tracer_camera;
mod path_tracer_subscene;
mod shadow_showcase;
mod tonemap;

use cornell_rt::CornellRtScene;
use path_tracer_camera::{PathTracerCameraGpu, PathTracerCameraInput, PathTracerCameraRig};
use path_tracer_subscene::PathTracerSubscene;
use shadow_showcase::ShadowShowcase;
use sturdy_engine::{
    AntiAliasingConfig, AntiAliasingDial, AntiAliasingPass, AppRuntime, AppRuntimeFrame,
    AutoExposureConfig, AutoExposurePass, BloomConfig, BloomPass, CpuProceduralTexture2d,
    DebugOverlay, DebugOverlayRenderer, DebugViewPicker, Engine, Error, Extent3d, Format,
    GraphImageHistory, GpuProceduralTexture, HdrPipelineDesc, HdrPreference, ImageDesc,
    ImageDimension, ImageUsage, KeyInput, KeyInputState, KeyModifier, KeyToken, MotionVectorLayer,
    MotionVectorSpace, ProceduralTextureRecipe, ProceduralTextureUpdatePolicy,
    Result as EngineResult, RuntimeApp, RuntimeController, RuntimeMotionVectorDesc,
    RuntimePostProcessDesc, RuntimeSettingDescriptor, RuntimeSettingId, RuntimeSettingKey,
    RuntimeSettingOption, SamplerPreset, ShaderProgram, ShaderWatcher, ShellFrame, StageMask,
    SurfaceColorSpace, ToneMappingOp, WindowConfig, init_tracing_with_default_filter,
    push_constants, run_with_runtime, set_log_level,
};

const PT_QUALITY_SETTING: &str = "testbed.pt_quality";
const PT_RESET_SETTING: &str = "testbed.pt_reset_history";
const PT_MAX_ACCUMULATION_FRAMES: u32 = 65536;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PtQuality {
    Preview,
    Balanced,
    Stable,
}

impl PtQuality {
    fn label(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Balanced => "balanced",
            Self::Stable => "stable",
        }
    }

    fn display_label(self) -> &'static str {
        match self {
            Self::Preview => "Preview",
            Self::Balanced => "Balanced",
            Self::Stable => "Stable",
        }
    }

    fn max_frames(self) -> u32 {
        match self {
            Self::Preview => 32,
            Self::Balanced => 512,
            Self::Stable => PT_MAX_ACCUMULATION_FRAMES,
        }
    }

    fn from_setting(value: &str) -> Option<Self> {
        match value {
            "preview" | "Preview" => Some(Self::Preview),
            "balanced" | "Balanced" => Some(Self::Balanced),
            "stable" | "Stable" => Some(Self::Stable),
            _ => None,
        }
    }
}
use tonemap::{TonemapParams, tone_mapping_id};

#[push_constants]
struct FrameConstants {
    time: f32,
    aspect: f32,
    resolution: [f32; 2],
    scene: u32,
    frame_index: u32,
}

#[push_constants]
struct LutParams {
    phase: f32,
}

#[push_constants]
struct CornellDenoiseParams {
    frame_index: u32,
    max_frames: u32,
    output_mode: u32,
    _pad: u32,
}

#[push_constants]
struct PtTemporalConstants {
    frame_index: u32,
    has_history: u32,
    max_frames: u32,
    mode: u32,
}

#[derive(Clone, Copy, Debug)]
struct TonemapSettings {
    exposure: f32,
    white_point: f32,
    display_gain: f32,
    output_gamma: f32,
    aces_a: f32,
    aces_b: f32,
    aces_c: f32,
    aces_d: f32,
    aces_e: f32,
    reinhard_white: f32,
    hermite_contrast: f32,
    linear_white: f32,
    // PsychoV-11 parameters
    psycho_peak_value: f32,
    psycho_highlights: f32,
    psycho_shadows: f32,
    psycho_contrast_high: f32,
    psycho_contrast_low: f32,
    psycho_purity: f32,
    psycho_bleaching: f32,
    psycho_hue_restore: f32,
    psycho_adapt_contrast: f32,
    psycho_cone_exp: f32,
}

impl Default for TonemapSettings {
    fn default() -> Self {
        let params = TonemapParams::default();
        Self {
            exposure: params.exposure,
            white_point: params.white_point,
            display_gain: params.display_gain,
            output_gamma: params.output_gamma,
            aces_a: params.aces_a,
            aces_b: params.aces_b,
            aces_c: params.aces_c,
            aces_d: params.aces_d,
            aces_e: params.aces_e,
            reinhard_white: params.reinhard_white,
            hermite_contrast: params.hermite_contrast,
            linear_white: params.linear_white,
            psycho_peak_value: params.psycho_peak_value,
            psycho_highlights: params.psycho_highlights,
            psycho_shadows: params.psycho_shadows,
            psycho_contrast_high: params.psycho_contrast_high,
            psycho_contrast_low: params.psycho_contrast_low,
            psycho_purity: params.psycho_purity,
            psycho_bleaching: params.psycho_bleaching,
            psycho_hue_restore: params.psycho_hue_restore,
            psycho_adapt_contrast: params.psycho_adapt_contrast,
            psycho_cone_exp: params.psycho_cone_exp,
        }
    }
}

impl TonemapSettings {
    fn params(
        self,
        tone_mapping: ToneMappingOp,
        hdr_output: bool,
        selected_dial: TonemapDial,
    ) -> TonemapParams {
        let mut settings = self;
        settings.sync_operator_white_point(tone_mapping, selected_dial);
        TonemapParams {
            tonemap_op: tone_mapping_id(tone_mapping),
            hdr_output: hdr_output as u32,
            exposure: settings.exposure,
            white_point: settings.white_point,
            display_gain: settings.display_gain,
            output_gamma: settings.output_gamma,
            aces_a: settings.aces_a,
            aces_b: settings.aces_b,
            aces_c: settings.aces_c,
            aces_d: settings.aces_d,
            aces_e: settings.aces_e,
            reinhard_white: settings.reinhard_white,
            hermite_contrast: settings.hermite_contrast,
            linear_white: settings.linear_white,
            psycho_peak_value: settings.psycho_peak_value,
            psycho_highlights: settings.psycho_highlights,
            psycho_shadows: settings.psycho_shadows,
            psycho_contrast_high: settings.psycho_contrast_high,
            psycho_contrast_low: settings.psycho_contrast_low,
            psycho_purity: settings.psycho_purity,
            psycho_bleaching: settings.psycho_bleaching,
            psycho_hue_restore: settings.psycho_hue_restore,
            psycho_adapt_contrast: settings.psycho_adapt_contrast,
            psycho_cone_exp: settings.psycho_cone_exp,
        }
    }

    fn get(self, dial: TonemapDial) -> f32 {
        match dial {
            TonemapDial::Exposure => self.exposure,
            TonemapDial::WhitePoint => self.white_point,
            TonemapDial::DisplayGain => self.display_gain,
            TonemapDial::OutputGamma => self.output_gamma,
            TonemapDial::AcesA => self.aces_a,
            TonemapDial::AcesB => self.aces_b,
            TonemapDial::AcesC => self.aces_c,
            TonemapDial::AcesD => self.aces_d,
            TonemapDial::AcesE => self.aces_e,
            TonemapDial::ReinhardWhite => self.reinhard_white,
            TonemapDial::HermiteContrast => self.hermite_contrast,
            TonemapDial::LinearWhite => self.linear_white,
            TonemapDial::PsychoPeakValue => self.psycho_peak_value,
            TonemapDial::PsychoHighlights => self.psycho_highlights,
            TonemapDial::PsychoShadows => self.psycho_shadows,
            TonemapDial::PsychoContrastHigh => self.psycho_contrast_high,
            TonemapDial::PsychoContrastLow => self.psycho_contrast_low,
            TonemapDial::PsychoPurity => self.psycho_purity,
            TonemapDial::PsychoBleaching => self.psycho_bleaching,
            TonemapDial::PsychoHueRestore => self.psycho_hue_restore,
            TonemapDial::PsychoAdaptContrast => self.psycho_adapt_contrast,
            TonemapDial::PsychoConeExp => self.psycho_cone_exp,
        }
    }

    fn reset_for(&mut self, tone_mapping: ToneMappingOp) {
        let defaults = Self::default();
        self.exposure = defaults.exposure;
        self.white_point = defaults.white_point;
        self.display_gain = defaults.display_gain;
        self.output_gamma = defaults.output_gamma;
        match tone_mapping {
            ToneMappingOp::Aces => {
                self.aces_a = defaults.aces_a;
                self.aces_b = defaults.aces_b;
                self.aces_c = defaults.aces_c;
                self.aces_d = defaults.aces_d;
                self.aces_e = defaults.aces_e;
            }
            ToneMappingOp::Reinhard => self.reinhard_white = defaults.reinhard_white,
            ToneMappingOp::Hermite => self.hermite_contrast = defaults.hermite_contrast,
            ToneMappingOp::Linear => self.linear_white = defaults.linear_white,
            ToneMappingOp::PbrNeutral | ToneMappingOp::AgX => {}
            ToneMappingOp::PsychoV11 | ToneMappingOp::PsychoV17 => {
                self.psycho_peak_value = defaults.psycho_peak_value;
                self.psycho_highlights = defaults.psycho_highlights;
                self.psycho_shadows = defaults.psycho_shadows;
                self.psycho_contrast_high = defaults.psycho_contrast_high;
                self.psycho_contrast_low = defaults.psycho_contrast_low;
                self.psycho_purity = defaults.psycho_purity;
                self.psycho_bleaching = defaults.psycho_bleaching;
                self.psycho_hue_restore = defaults.psycho_hue_restore;
                self.psycho_adapt_contrast = defaults.psycho_adapt_contrast;
                self.psycho_cone_exp = defaults.psycho_cone_exp;
            }
        }
    }

    fn set(&mut self, dial: TonemapDial, value: f32) {
        match dial {
            TonemapDial::Exposure => self.exposure = value,
            TonemapDial::WhitePoint => self.white_point = value,
            TonemapDial::DisplayGain => self.display_gain = value,
            TonemapDial::OutputGamma => self.output_gamma = value,
            TonemapDial::AcesA => self.aces_a = value,
            TonemapDial::AcesB => self.aces_b = value,
            TonemapDial::AcesC => self.aces_c = value,
            TonemapDial::AcesD => self.aces_d = value,
            TonemapDial::AcesE => self.aces_e = value,
            TonemapDial::ReinhardWhite => self.reinhard_white = value,
            TonemapDial::HermiteContrast => self.hermite_contrast = value,
            TonemapDial::LinearWhite => self.linear_white = value,
            TonemapDial::PsychoPeakValue => self.psycho_peak_value = value,
            TonemapDial::PsychoHighlights => self.psycho_highlights = value,
            TonemapDial::PsychoShadows => self.psycho_shadows = value,
            TonemapDial::PsychoContrastHigh => self.psycho_contrast_high = value,
            TonemapDial::PsychoContrastLow => self.psycho_contrast_low = value,
            TonemapDial::PsychoPurity => self.psycho_purity = value,
            TonemapDial::PsychoBleaching => self.psycho_bleaching = value,
            TonemapDial::PsychoHueRestore => self.psycho_hue_restore = value,
            TonemapDial::PsychoAdaptContrast => self.psycho_adapt_contrast = value,
            TonemapDial::PsychoConeExp => self.psycho_cone_exp = value,
        }
    }

    fn sync_operator_white_point(&mut self, tone_mapping: ToneMappingOp, changed: TonemapDial) {
        if changed != TonemapDial::WhitePoint {
            return;
        }
        match tone_mapping {
            ToneMappingOp::Reinhard => self.reinhard_white = self.white_point,
            ToneMappingOp::Linear => self.linear_white = self.white_point,
            ToneMappingOp::Aces
            | ToneMappingOp::Hermite
            | ToneMappingOp::PbrNeutral
            | ToneMappingOp::AgX
            | ToneMappingOp::PsychoV11
            | ToneMappingOp::PsychoV17 => {}
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TonemapDial {
    Exposure,
    WhitePoint,
    DisplayGain,
    OutputGamma,
    AcesA,
    AcesB,
    AcesC,
    AcesD,
    AcesE,
    ReinhardWhite,
    HermiteContrast,
    LinearWhite,
    PsychoPeakValue,
    PsychoHighlights,
    PsychoShadows,
    PsychoContrastHigh,
    PsychoContrastLow,
    PsychoPurity,
    PsychoBleaching,
    PsychoHueRestore,
    PsychoAdaptContrast,
    PsychoConeExp,
}

impl TonemapDial {
    fn next(self) -> Self {
        match self {
            Self::Exposure => Self::WhitePoint,
            Self::WhitePoint => Self::DisplayGain,
            Self::DisplayGain => Self::OutputGamma,
            Self::OutputGamma => Self::AcesA,
            Self::AcesA => Self::AcesB,
            Self::AcesB => Self::AcesC,
            Self::AcesC => Self::AcesD,
            Self::AcesD => Self::AcesE,
            Self::AcesE => Self::ReinhardWhite,
            Self::ReinhardWhite => Self::HermiteContrast,
            Self::HermiteContrast => Self::LinearWhite,
            Self::LinearWhite => Self::PsychoPeakValue,
            Self::PsychoPeakValue => Self::PsychoHighlights,
            Self::PsychoHighlights => Self::PsychoShadows,
            Self::PsychoShadows => Self::PsychoContrastHigh,
            Self::PsychoContrastHigh => Self::PsychoContrastLow,
            Self::PsychoContrastLow => Self::PsychoPurity,
            Self::PsychoPurity => Self::PsychoBleaching,
            Self::PsychoBleaching => Self::PsychoHueRestore,
            Self::PsychoHueRestore => Self::PsychoAdaptContrast,
            Self::PsychoAdaptContrast => Self::PsychoConeExp,
            Self::PsychoConeExp => Self::Exposure,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Exposure => "exposure",
            Self::WhitePoint => "white point",
            Self::DisplayGain => "display gain",
            Self::OutputGamma => "SDR gamma",
            Self::AcesA => "ACES a",
            Self::AcesB => "ACES b",
            Self::AcesC => "ACES c",
            Self::AcesD => "ACES d",
            Self::AcesE => "ACES e",
            Self::ReinhardWhite => "Reinhard white",
            Self::HermiteContrast => "Hermite contrast",
            Self::LinearWhite => "Linear white",
            Self::PsychoPeakValue => "Psycho peak nits/203",
            Self::PsychoHighlights => "Psycho highlights",
            Self::PsychoShadows => "Psycho shadows",
            Self::PsychoContrastHigh => "Psycho contrast hi",
            Self::PsychoContrastLow => "Psycho contrast lo",
            Self::PsychoPurity => "Psycho purity",
            Self::PsychoBleaching => "Psycho bleaching",
            Self::PsychoHueRestore => "Psycho hue restore",
            Self::PsychoAdaptContrast => "Psycho adapt contrast",
            Self::PsychoConeExp => "Psycho cone exp",
        }
    }

    fn step(self) -> f32 {
        match self {
            Self::AcesB | Self::AcesE => 0.01,
            Self::OutputGamma | Self::HermiteContrast => 0.05,
            Self::AcesA | Self::AcesC | Self::AcesD => 0.1,
            Self::Exposure | Self::DisplayGain => 0.1,
            Self::WhitePoint | Self::ReinhardWhite | Self::LinearWhite => 0.25,
            Self::PsychoPeakValue => 0.5,
            Self::PsychoBleaching | Self::PsychoHueRestore => 0.05,
            Self::PsychoHighlights
            | Self::PsychoShadows
            | Self::PsychoContrastHigh
            | Self::PsychoContrastLow
            | Self::PsychoPurity
            | Self::PsychoAdaptContrast
            | Self::PsychoConeExp => 0.05,
        }
    }

    fn min(self) -> f32 {
        match self {
            Self::AcesB | Self::AcesE => 0.0,
            Self::OutputGamma | Self::HermiteContrast => 0.2,
            Self::Exposure
            | Self::WhitePoint
            | Self::DisplayGain
            | Self::ReinhardWhite
            | Self::LinearWhite => 0.05,
            Self::AcesA | Self::AcesC | Self::AcesD => 0.01,
            Self::PsychoPeakValue => 0.5,
            Self::PsychoBleaching => 0.0,
            Self::PsychoHueRestore => 0.0,
            Self::PsychoHighlights
            | Self::PsychoShadows
            | Self::PsychoContrastHigh
            | Self::PsychoContrastLow
            | Self::PsychoPurity
            | Self::PsychoAdaptContrast
            | Self::PsychoConeExp => 0.1,
        }
    }

    fn max(self) -> f32 {
        match self {
            Self::OutputGamma => 4.0,
            Self::HermiteContrast => 3.0,
            Self::AcesB | Self::AcesE => 1.0,
            Self::AcesA | Self::AcesC | Self::AcesD => 8.0,
            Self::Exposure
            | Self::WhitePoint
            | Self::DisplayGain
            | Self::ReinhardWhite
            | Self::LinearWhite => 16.0,
            Self::PsychoPeakValue => 50.0,
            Self::PsychoBleaching | Self::PsychoHueRestore => 1.0,
            Self::PsychoHighlights
            | Self::PsychoShadows
            | Self::PsychoContrastHigh
            | Self::PsychoContrastLow
            | Self::PsychoPurity
            | Self::PsychoAdaptContrast
            | Self::PsychoConeExp => 3.0,
        }
    }
}

struct Testbed {
    engine: Engine,
    scene_program: ShaderProgram,
    motion_program: ShaderProgram,
    tonemap_program: ShaderProgram,
    pt_temporal_program: ShaderProgram,
    pt_clear_program: ShaderProgram,
    cornell_denoise_program: ShaderProgram,
    bloom_pass: BloomPass,
    aa_pass: AntiAliasingPass,
    auto_exposure_pass: AutoExposurePass,
    /// General auto-exposure config for rasterized and outdoor scenes.
    auto_exposure_config: AutoExposureConfig,
    /// Cornell/path-traced config: high metering_floor discards miss-ray background.
    auto_exposure_config_pt: AutoExposureConfig,
    /// EV100 from the previous frame's auto-exposure compute — fed into
    /// `tonemap_constants.exposure` on the next frame as a linear scale.
    auto_exposure_adapted_ev: Option<f32>,
    bloom_config: BloomConfig,
    bloom_enabled: bool,
    bloom_only: bool,
    show_motion_vectors: bool,
    hdr_output: bool,
    tone_mapping: ToneMappingOp,
    tonemap_settings: TonemapSettings,
    selected_tonemap_dial: TonemapDial,
    aa: AntiAliasingConfig,
    color_lut: GpuProceduralTexture,
    procedural_mask: CpuProceduralTexture2d,
    debug_overlay: DebugOverlayRenderer,
    debug_view_picker: DebugViewPicker,
    runtime_controller: Option<RuntimeController>,
    texture_resolution: TextureResolutionTier,
    pt_quality: PtQuality,
    selected_scene: ShowcaseScene,
    shadow_scene: ShadowShowcase,
    cornell_rt_scene: Option<CornellRtScene>,
    path_tracer_subscene: PathTracerSubscene,
    path_tracer_camera: PathTracerCameraRig,
    previous_path_tracer_camera: PathTracerCameraGpu,
    path_tracer_camera_input: PathTracerCameraInput,
    pt_history: GraphImageHistory,
    pt_max_frames: u32,
    clear_path_tracer_after_first_trace: bool,
    show_graph_inspector: bool,
    pending_debug_image_export: bool,
    debug_image_export_index: u64,
    frame_index: u64,
    started_at: Instant,
    last_frame_elapsed: f32,
    shader_watcher: ShaderWatcher,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TextureResolutionTier {
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShowcaseScene {
    Overview,
    ProceduralSky,
    ProceduralMaterials,
    RealtimeShadows,
    TemporalAndPost,
    ProceduralTextures,
    Bloom,
    CornellPathTracing,
    DebugGraph,
}

impl ShowcaseScene {
    const ALL: [Self; 9] = [
        Self::Overview,
        Self::ProceduralSky,
        Self::ProceduralMaterials,
        Self::RealtimeShadows,
        Self::TemporalAndPost,
        Self::ProceduralTextures,
        Self::Bloom,
        Self::CornellPathTracing,
        Self::DebugGraph,
    ];

    fn id(self) -> u32 {
        match self {
            Self::Overview => 0,
            Self::ProceduralSky => 1,
            Self::ProceduralMaterials => 2,
            Self::RealtimeShadows => 3,
            Self::TemporalAndPost => 4,
            Self::ProceduralTextures => 5,
            Self::Bloom => 6,
            Self::CornellPathTracing => 7,
            Self::DebugGraph => 8,
        }
    }

    fn from_number_key(key: &str) -> Option<Self> {
        match key {
            "1" => Some(Self::Overview),
            "2" => Some(Self::ProceduralSky),
            "3" => Some(Self::ProceduralMaterials),
            "4" => Some(Self::RealtimeShadows),
            "5" => Some(Self::TemporalAndPost),
            "6" => Some(Self::ProceduralTextures),
            "7" => Some(Self::Bloom),
            "8" => Some(Self::CornellPathTracing),
            "9" => Some(Self::DebugGraph),
            _ => None,
        }
    }

    fn number(self) -> u32 {
        self.id() + 1
    }

    fn label(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::ProceduralSky => "procedural sky",
            Self::ProceduralMaterials => "procedural materials",
            Self::RealtimeShadows => "realtime shadows",
            Self::TemporalAndPost => "temporal/post",
            Self::ProceduralTextures => "procedural textures",
            Self::Bloom => "bloom",
            Self::CornellPathTracing => "cornell path tracing",
            Self::DebugGraph => "debug graph",
        }
    }

    fn short_label(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::ProceduralSky => "sky",
            Self::ProceduralMaterials => "materials",
            Self::RealtimeShadows => "shadows",
            Self::TemporalAndPost => "temporal",
            Self::ProceduralTextures => "textures",
            Self::Bloom => "bloom",
            Self::CornellPathTracing => "cornell",
            Self::DebugGraph => "debug",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Self::Overview => "mosaic of every showcase scene",
            Self::ProceduralSky => "shader-driven sky lighting",
            Self::ProceduralMaterials => "procedural PBR-style material spheres",
            Self::RealtimeShadows => "deferred PBR with dynamic cascaded shadows",
            Self::TemporalAndPost => "motion-vector and post-processing stress scene",
            Self::ProceduralTextures => "CPU/GPU procedural texture sampling",
            Self::Bloom => "HDR emitters for bloom evaluation",
            Self::CornellPathTracing => "progressive path tracing subscenes",
            Self::DebugGraph => "debug image and render-graph inspection",
        }
    }

    fn picker_line() -> String {
        let choices = Self::ALL
            .iter()
            .map(|scene| format!("{} {}", scene.number(), scene.short_label()))
            .collect::<Vec<_>>()
            .join("  ");
        format!("scenes: {choices}")
    }
}

impl TextureResolutionTier {
    fn label(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    fn size(self) -> u32 {
        match self {
            Self::Low => 256,
            Self::Medium => 512,
            Self::High => 1024,
        }
    }

    fn from_setting(value: &str) -> Option<Self> {
        match value {
            "low" | "Low" => Some(Self::Low),
            "medium" | "Medium" => Some(Self::Medium),
            "high" | "High" => Some(Self::High),
            _ => None,
        }
    }
}

impl RuntimeApp for Testbed {
    type Error = sturdy_engine::Error;

    fn init(runtime: &mut AppRuntime) -> EngineResult<Self> {
        ensure_testbed_tracing_filter();
        let engine = runtime.engine();
        let surface_info = runtime.surface().info();
        let hdr_caps = runtime.surface().hdr_caps()?;
        let hdr_desc =
            HdrPipelineDesc::select(&hdr_caps, &engine.caps(), HdrPreference::PreferHdr)?;
        let hdr_output = surface_is_hdr(surface_info.color_space);

        tracing::info!(
            adapter = ?engine.adapter_name(),
            backend = ?engine.backend_kind(),
            surface_format = ?surface_info.format,
            color_space = ?surface_info.color_space,
            width = surface_info.size.width,
            height = surface_info.size.height,
            "testbed renderer initialized"
        );
        tracing::info!(
            hdr_mode = ?hdr_desc.mode,
            tone_mapping = ?hdr_desc.tone_mapping,
            "HDR pipeline selected"
        );

        // GPU-driven color LUT: the generator shader receives a phase parameter
        // each frame and writes the gradient directly on the GPU — no CPU upload.
        let lut_program = engine.load_shader(shader_path("color_lut_gen.slang"))?;
        let color_lut = GpuProceduralTexture::new(
            engine,
            "color_lut",
            256,
            1,
            Format::Rgba8Unorm,
            lut_program,
        )?;
        let procedural_mask = CpuProceduralTexture2d::from_recipe_rgba8(
            engine,
            "procedural_mask",
            512,
            512,
            ProceduralTextureUpdatePolicy::Once,
            ProceduralTextureRecipe::RadialMask {
                inner_radius: 0.18,
                outer_radius: 1.0,
                color: [255, 255, 255, 255],
            },
        )?;

        let scene_program = engine.load_shader(shader_path("shader_graph_fragment.slang"))?;
        let motion_program = engine.load_shader(shader_path("motion_vectors.slang"))?;
        let tonemap_program = engine.load_shader(shader_path("tonemap.slang"))?;
        let pt_temporal_program =
            engine.load_shader(shader_path("temporal_accumulate.slang"))?;
        let pt_clear_program =
            engine.load_shader(shader_path("temporal_clear_history.slang"))?;
        let cornell_denoise_program = engine.load_shader(shader_path("cornell_denoise.slang"))?;

        let mut shader_watcher = ShaderWatcher::new();
        for program in [
            &scene_program,
            &motion_program,
            &tonemap_program,
            &pt_temporal_program,
            &pt_clear_program,
            &cornell_denoise_program,
        ] {
            if let Some(path) = program.source_path() {
                shader_watcher.watch(path);
            }
        }

        let shadow_scene = ShadowShowcase::new(engine)?;
        let cornell_rt_scene = CornellRtScene::new(engine, shader_path("cornell_rt.slang"))?;
        if cornell_rt_scene.is_some() {
            tracing::info!("hardware path tracing enabled for Cornell/outdoor subscenes");
        } else {
            tracing::warn!("hardware path tracing unavailable; using shader fallback");
        }
        let engine_clone = engine.clone();
        let mut testbed = Self {
            engine: engine_clone,
            scene_program,
            motion_program,
            tonemap_program,
            pt_temporal_program,
            pt_clear_program,
            cornell_denoise_program,
            bloom_pass: BloomPass::new(engine)?,
            aa_pass: AntiAliasingPass::new(engine)?,
            auto_exposure_pass: AutoExposurePass::new(engine)?,
            auto_exposure_config: AutoExposureConfig {
                enabled: true,
                min_ev: -1.0,
                max_ev: 6.0,
                // Bias toward highlights so direct-sun scenes (bright ground plane)
                // don't blow out: lower percentile pushes target_log_luma up →
                // higher adapted EV → lower exposure multiplier → darker output.
                target_percentile: 0.40,
                // Exclude near-black sky/miss-ray pixels from the mean so they
                // don't drag target_log_luma down and over-brighten the scene.
                metering_floor: 0.01,
                ..AutoExposureConfig::default()
            },
            // Path-traced / Cornell config: high metering_floor so background miss-ray
            // pixels (luma < 0.02) never enter the histogram mean.  Only lit geometry
            // contributes.
            // min_ev=1.5 matches REF_EV so the scene is never brightened — the path
            // tracer converges to the correct exposure on its own and only needs
            // auto-exposure to prevent it from being too bright, not to lift it.
            auto_exposure_config_pt: AutoExposureConfig {
                enabled: true,
                metering_floor: 0.02,
                min_ev: 1.5,
                max_ev: 4.0,
                ..AutoExposureConfig::default()
            },
            auto_exposure_adapted_ev: None,
            bloom_config: BloomConfig::default(),
            bloom_enabled: true,
            bloom_only: false,
            show_motion_vectors: false,
            hdr_output,
            tone_mapping: ToneMappingOp::Hermite,
            tonemap_settings: TonemapSettings::default(),
            selected_tonemap_dial: TonemapDial::Exposure,
            aa: AntiAliasingConfig::default(),
            color_lut,
            procedural_mask,
            debug_overlay: DebugOverlayRenderer::new(engine)?,
            debug_view_picker: DebugViewPicker::new(engine)?,
            runtime_controller: None,
            texture_resolution: TextureResolutionTier::Medium,
            pt_quality: PtQuality::Stable,
            selected_scene: ShowcaseScene::CornellPathTracing,
            shadow_scene,
            cornell_rt_scene,
            path_tracer_subscene: PathTracerSubscene::DEFAULT,
            path_tracer_camera: PathTracerCameraRig::outdoor_default(),
            previous_path_tracer_camera: PathTracerCameraRig::outdoor_default().gpu_data(),
            path_tracer_camera_input: PathTracerCameraInput::default(),
            pt_history: GraphImageHistory::new(),
            pt_max_frames: PT_MAX_ACCUMULATION_FRAMES,
            clear_path_tracer_after_first_trace: true,
            show_graph_inspector: false,
            pending_debug_image_export: false,
            debug_image_export_index: 0,
            frame_index: 0,
            started_at: Instant::now(),
            last_frame_elapsed: 0.0,
            shader_watcher,
        };
        let controller = runtime.controller().clone();
        testbed.register_runtime_settings(&controller)?;
        testbed.seed_runtime_settings(&controller)?;
        testbed.runtime_controller = Some(controller);
        Ok(testbed)
    }

    fn update(&mut self, appframe: &mut AppRuntimeFrame<'_>) -> EngineResult<()> {
        appframe.set_wait_for_gpu_before_present(
            self.selected_scene == ShowcaseScene::CornellPathTracing,
        );
        let shell_frame = appframe.shell_frame();
        let surface_image = appframe.surface_image();
        let runtime_controller = shell_frame.runtime_controller();

        // Poll for shader file changes and hot-reload any that have changed.
        let changed_paths = self.shader_watcher.poll_changed();
        for path in &changed_paths {
            let result = if path == self.scene_program.source_path().unwrap_or(path.as_path()) {
                self.scene_program.reload()
            } else if path == self.motion_program.source_path().unwrap_or(path.as_path()) {
                self.motion_program.reload()
            } else if path == self.tonemap_program.source_path().unwrap_or(path.as_path()) {
                self.tonemap_program.reload()
            } else if path
                == self
                    .pt_temporal_program
                    .source_path()
                    .unwrap_or(path.as_path())
            {
                self.pt_temporal_program.reload()
            } else if path
                == self
                    .pt_clear_program
                    .source_path()
                    .unwrap_or(path.as_path())
            {
                self.pt_clear_program.reload()
            } else if path
                == self
                    .cornell_denoise_program
                    .source_path()
                    .unwrap_or(path.as_path())
            {
                self.cornell_denoise_program.reload()
            } else {
                Ok(false)
            };
            match result {
                Ok(true) => {
                    runtime_controller.clear_shader_compile_error(path);
                    self.reset_path_tracer_accumulation();
                    tracing::info!("hot reload: reloaded {}", path.display());
                }
                Err(e) => {
                    runtime_controller.report_shader_compile_error(path, format!("{}", e));
                    tracing::error!("hot reload: compile error in {}: {}", path.display(), e);
                }
                _ => {}
            }
        }

        let elapsed = self.started_at.elapsed().as_secs_f32();
        let delta_seconds = (elapsed - self.last_frame_elapsed).clamp(0.0, 0.1);
        self.last_frame_elapsed = elapsed;
        let frame_index = self.frame_index.min(u32::MAX as u64) as u32;
        self.frame_index = self.frame_index.saturating_add(1);
        let ext = surface_image.desc().extent;
        let aspect = ext.width as f32 / ext.height.max(1) as f32;

        if self.advance_path_tracer_camera(delta_seconds) {
            self.reset_path_tracer_accumulation();
        }

        // Register swapchain first — required so hdr_color_image can read the extent.
        let swapchain = shell_frame.inner().swapchain_image(surface_image)?;
        let frame = shell_frame.inner();

        // GPU procedural LUT: always generated so debug views and shader scenes can use it.
        let color_lut = self.color_lut.generate_with_constants(
            frame,
            &LutParams {
                phase: elapsed * 0.4,
            },
        )?;
        let procedural_mask = self.procedural_mask.prepare(frame)?;
        shell_frame.register_debug_image("gpu_color_lut", &color_lut);
        shell_frame.register_debug_image("cpu_procedural_mask", &procedural_mask);

        // Scene rendering: rasterized 3D deferred pass for RealtimeShadows,
        // fullscreen shader for all other scenes.
        let (scene_color, motion_vectors_opt) = if self.selected_scene
            == ShowcaseScene::RealtimeShadows
        {
            // Use scene-4-specific graph image names. The fullscreen showcase scenes use
            // "scene_color" as an MSAA-resolved image with COPY_DST usage, while this
            // deferred path renders directly into a single-sample target with COPY_SRC usage.
            // Reusing the same cache key name can evict an in-flight image during scene
            // switches on low-latency present paths.
            let scene_target = shell_frame.default_hdr_scene_target("shadow_scene_color", 1)?;
            let scene_color = shell_frame
                .resolve_default_hdr_scene_target(&scene_target, "shadow_scene_color_resolved")?;
            self.shadow_scene.advance(elapsed);
            self.shadow_scene
                .draw(frame, &scene_color, &self.engine, aspect)?;
            shell_frame.register_debug_image("hdr_shadow_scene_color", &scene_color);
            (scene_color, None)
        } else {
            let mut path_tracer_motion_vectors = None;
            let scene_color = if self.selected_scene == ShowcaseScene::CornellPathTracing {
                if let Some(cornell_rt_scene) = &self.cornell_rt_scene {
                    // Acquire ping-pong history first to get the current frame index,
                    // which the Cornell RT path tracer uses as its per-frame sample seed.
                    let mut pt_history_desc = CornellRtScene::output_desc(ext.width, ext.height);
                    pt_history_desc.usage |= ImageUsage::RENDER_TARGET | ImageUsage::SAMPLED;
                    let pt_history = frame.history_images(
                        &mut self.pt_history,
                        "cornell_accumulation",
                        pt_history_desc,
                    )?;
                    let cornell_sample_frame =
                        pt_history.frame_index.min(u32::MAX as u64) as u32;
                    let current_camera = self.path_tracer_camera.gpu_data();
                    let cornell_frame = cornell_rt_scene.draw(
                        frame,
                        &self.engine,
                        self.path_tracer_subscene,
                        ext.width,
                        ext.height,
                        aspect,
                        cornell_sample_frame,
                        current_camera,
                        self.previous_path_tracer_camera,
                    )?;
                    self.previous_path_tracer_camera = current_camera;
                    shell_frame.register_debug_image("hdr_cornell_rt_sample", &cornell_frame.color);
                    shell_frame.register_debug_image("hdr_cornell_guides", &cornell_frame.guide);
                    shell_frame.register_debug_image(
                        "hdr_cornell_material_guides",
                        &cornell_frame.material_guide,
                    );
                    shell_frame.register_debug_image(
                        "cornell_camera_local_motion_vectors",
                        &cornell_frame.motion_vectors,
                    );
                    shell_frame.register_debug_image("cornell_normals", &cornell_frame.normals);
                    shell_frame.register_debug_image("cornell_depth", &cornell_frame.depth);
                    let cornell_guide = cornell_frame.guide;
                    let cornell_material_guide = cornell_frame.material_guide;
                    path_tracer_motion_vectors = Some(cornell_frame.motion_vectors);

                    // Temporal accumulation using the same pt_history acquired above.
                    if !pt_history.has_history {
                        pt_history.write.execute_shader_auto(&self.pt_clear_program)?;
                    }
                    cornell_frame.color.register_as("current_signal");
                    pt_history.read.register_as("history_signal");
                    frame.set_sampler("current_sampler", SamplerPreset::Linear);
                    frame.set_sampler("history_sampler", SamplerPreset::Linear);
                    pt_history.write.execute_shader_with_push_constants(
                        &self.pt_temporal_program,
                        StageMask::FRAGMENT,
                        bytemuck::bytes_of(&PtTemporalConstants {
                            frame_index: cornell_sample_frame,
                            has_history: pt_history.has_history as u32,
                            max_frames: self.pt_max_frames,
                            mode: 0,
                        }),
                    )?;
                    let accumulated = pt_history.write;
                    shell_frame.register_debug_image("hdr_cornell_accumulated", &accumulated);

                    let mut denoise_desc = accumulated.desc();
                    denoise_desc.usage |=
                        ImageUsage::SAMPLED | ImageUsage::RENDER_TARGET | ImageUsage::COPY_SRC;
                    denoise_desc.debug_name = Some("cornell_rt_denoised");
                    let denoised = frame.image("cornell_denoised", denoise_desc)?;
                    accumulated.register_as("accumulated_radiance");
                    cornell_guide.register_as("current_guides");
                    cornell_material_guide.register_as("current_material_guides");
                    denoised.execute_shader_with_constants_auto(
                        &self.cornell_denoise_program,
                        &CornellDenoiseParams {
                            frame_index: cornell_sample_frame,
                            max_frames: self.pt_max_frames,
                            output_mode: 0,
                            _pad: 0,
                        },
                    )?;
                    shell_frame.register_debug_image("hdr_cornell_denoised", &denoised);

                    let mut delta_desc = accumulated.desc();
                    delta_desc.usage |=
                        ImageUsage::SAMPLED | ImageUsage::RENDER_TARGET | ImageUsage::COPY_SRC;
                    delta_desc.debug_name = Some("cornell_rt_denoise_delta");
                    let denoise_delta = frame.image("cornell_denoise_delta", delta_desc)?;
                    denoise_delta.execute_shader_with_constants_auto(
                        &self.cornell_denoise_program,
                        &CornellDenoiseParams {
                            frame_index: cornell_sample_frame,
                            max_frames: self.pt_max_frames,
                            output_mode: 1,
                            _pad: 0,
                        },
                    )?;
                    shell_frame.register_debug_image("hdr_cornell_denoise_delta", &denoise_delta);

                    denoised.register_as("scene_color");
                    let final_color = denoised;

                    if self.clear_path_tracer_after_first_trace {
                        self.pt_history.reset();
                        self.clear_path_tracer_after_first_trace = false;
                        tracing::debug!(
                            subscene = self.path_tracer_subscene.label(),
                            "path tracer accumulation cleared after first trace"
                        );
                    }

                    final_color
                } else {
                    let scene_target = shell_frame
                        .default_hdr_scene_target("scene_color", self.actual_msaa_samples())?;
                    let scene_color = shell_frame
                        .resolve_default_hdr_scene_target(&scene_target, "scene_color")?;
                    scene_target.execute_shader_with_constants_auto(
                        &self.scene_program,
                        &FrameConstants {
                            time: elapsed,
                            aspect,
                            resolution: [ext.width as f32, ext.height as f32],
                            scene: self.selected_scene.id(),
                            frame_index,
                        },
                    )?;
                    scene_color
                }
            } else {
                let scene_target = shell_frame
                    .default_hdr_scene_target("scene_color", self.actual_msaa_samples())?;
                let scene_color =
                    shell_frame.resolve_default_hdr_scene_target(&scene_target, "scene_color")?;
                scene_target.execute_shader_with_constants_auto(
                    &self.scene_program,
                    &FrameConstants {
                        time: elapsed,
                        aspect,
                        resolution: [ext.width as f32, ext.height as f32],
                        scene: self.selected_scene.id(),
                        frame_index,
                    },
                )?;
                scene_color
            };
            shell_frame.register_debug_image("hdr_scene_color", &scene_color);
            let motion_vectors = if let Some(motion_vectors) = path_tracer_motion_vectors {
                motion_vectors
            } else {
                let motion_vectors = self.motion_vector_image(frame, ext.width, ext.height)?;
                motion_vectors.execute_shader_with_constants_auto(
                    &self.motion_program,
                    &FrameConstants {
                        time: elapsed,
                        aspect,
                        resolution: [ext.width as f32, ext.height as f32],
                        scene: self.selected_scene.id(),
                        frame_index,
                    },
                )?;
                motion_vectors
            };
            shell_frame.register_debug_image("camera_local_motion_vectors", &motion_vectors);
            (scene_color, Some(motion_vectors))
        };

        // When auto-exposure is active, replace the manual exposure dial with the
        // GPU-derived value from the previous frame (1-frame lag, imperceptible at 60+ fps).
        // REF_EV=1.5 calibrates so a scene at avg_luma≈0.35 linear (EV100≈1.5) needs
        // no exposure adjustment.  Brighter scenes are dimmed; darker scenes are lifted.
        // The adapt shader excludes bin-0 (near-black / no-hit rays) from the mean, so
        // path-traced scenes with black backgrounds auto-expose correctly on the lit region.
        const AUTO_EXPOSURE_REF_EV: f32 = 1.5;
        let is_cornell = self.selected_scene == ShowcaseScene::CornellPathTracing;
        let mut tonemap_constants = self.tonemap_settings.params(
            self.tone_mapping,
            self.hdr_output,
            self.selected_tonemap_dial,
        );
        if let Some(adapted_ev) = self.auto_exposure_adapted_ev {
            tonemap_constants.exposure = f32::exp2(AUTO_EXPOSURE_REF_EV - adapted_ev);
        }

        let shadow_showcase = self.selected_scene == ShowcaseScene::RealtimeShadows;
        if shadow_showcase {
            // Keep the deferred shadow showcase on a minimal present path: HDR scene
            // target -> tonemap -> swapchain. This avoids shared post-process/debug
            // graph image aliases while we exercise the 3D CSM path.
            scene_color.register_as("hdr_composite");
            swapchain
                .execute_shader_with_constants_auto(&self.tonemap_program, &tonemap_constants)?;
        } else {
            let cornell_bloom_config;
            let bloom_config = if self.selected_scene == ShowcaseScene::CornellPathTracing {
                cornell_bloom_config = BloomConfig {
                    threshold: 4.0,
                    knee: 1.0,
                    intensity: 0.08,
                    mip_count: self.bloom_config.mip_count,
                };
                &cornell_bloom_config
            } else {
                &self.bloom_config
            };
            let post = shell_frame.run_default_post_process(RuntimePostProcessDesc {
                scene_color: &scene_color,
                motion_vectors: motion_vectors_opt
                    .as_ref()
                    .map(|mv| RuntimeMotionVectorDesc {
                        image: mv,
                        space: MotionVectorSpace::CameraLocal,
                        layer: MotionVectorLayer::World,
                    }),
                bloom_pass: self.bloom_enabled.then_some(&self.bloom_pass),
                bloom_config: self.bloom_enabled.then_some(bloom_config),
                bloom_only: self.bloom_only,
                aa_pass: &self.aa_pass,
                aa_mode: if self.selected_scene == ShowcaseScene::CornellPathTracing {
                    sturdy_engine::AntiAliasingMode::Fxaa(Default::default())
                } else {
                    self.aa.mode
                },
                current_jitter_uv: None,
                swapchain: &swapchain,
                tonemap_program: &self.tonemap_program,
                tonemap_constants: &tonemap_constants,
                // Cornell uses a dedicated config with a high metering floor so miss-ray
                // background pixels (luma < 0.02) are excluded from the histogram mean.
                // Other scenes use the general config with metering_floor=1e-5.
                auto_exposure_pass: Some(&self.auto_exposure_pass),
                auto_exposure_config: Some(if is_cornell {
                    &self.auto_exposure_config_pt
                } else {
                    &self.auto_exposure_config
                }),
            })?;
            // Store the GPU-computed EV for next frame's tonemap exposure calculation.
            self.auto_exposure_adapted_ev = post.adapted_ev;
        }
        shell_frame.publish_runtime_diagnostics(
            self.aa.mode.label(),
            self.actual_msaa_samples(),
            self.bloom_enabled,
            self.bloom_only,
        );
        let _ = self
            .debug_view_picker
            .present_selected(&shell_frame, &swapchain)?;
        if runtime_controller
            .bool_setting(RuntimeSettingKey::OverlayVisibility)
            .unwrap_or(true)
        {
            shell_frame
                .set_runtime_overlay_lines(self.overlay_lines(&shell_frame, &runtime_controller));
        }

        shell_frame.run_camera_locked_pass("hud_overlay", &swapchain, |frame, target| {
            self.draw_hud(&shell_frame, frame, target, ext.width, ext.height)
        })?;
        frame.present_image(&swapchain)?;
        if self.pending_debug_image_export {
            self.pending_debug_image_export = false;
            self.export_selected_debug_image(&shell_frame)?;
        }

        // In debug builds, validate the recorded graph and print any diagnostics.
        #[cfg(debug_assertions)]
        for d in frame.validate() {
            tracing::info!("[graph {:?}] {}", d.level, d.message);
        }

        Ok(())
    }

    fn key_input(&mut self, input: &KeyInput) -> EngineResult<()> {
        if input.repeat {
            return Ok(());
        }

        let pressed = input.state == KeyInputState::Pressed;
        match &input.key {
            KeyToken::Key(key) => self.set_path_tracer_camera_key(key, pressed),
            KeyToken::Modifier(KeyModifier::Ctrl) => {
                self.path_tracer_camera_input.fast = pressed;
            }
            KeyToken::Modifier(_) => {}
        }
        Ok(())
    }

    fn key_pressed(&mut self, key: &str) -> EngineResult<()> {
        if let Some(slot) = PathTracerSubscene::shifted_digit_slot(key) {
            self.select_path_tracer_subscene_slot(slot)?;
            return Ok(());
        }

        if self.outdoor_camera_active() && matches!(key, "w" | "a" | "s" | "d" | "q" | "e") {
            return Ok(());
        }

        if let Some(scene) = ShowcaseScene::from_number_key(key) {
            self.select_scene(scene)?;
            return Ok(());
        }

        let mut runtime_controller = self.runtime_controller.clone();
        if key == "b" {
            if let Some(controller) = runtime_controller.as_mut() {
                let next = !controller
                    .bool_setting(RuntimeSettingKey::BloomOnly)
                    .unwrap_or(self.bloom_only);
                controller
                    .transact()
                    .set_engine_value(RuntimeSettingKey::BloomOnly, next)
                    .apply()?;
                tracing::info!("bloom-only: {next}");
            }
        } else if key == "B" {
            if let Some(controller) = runtime_controller.as_mut() {
                let next = !controller
                    .bool_setting(RuntimeSettingKey::BloomEnabled)
                    .unwrap_or(self.bloom_enabled);
                controller
                    .transact()
                    .set_engine_value(RuntimeSettingKey::BloomEnabled, next)
                    .apply()?;
                tracing::info!("bloom: {}", if next { "on" } else { "off" });
            }
        } else if key == "V" || key == "v" {
            if self.selected_scene == ShowcaseScene::TemporalAndPost {
                if let Some(controller) = runtime_controller.as_mut() {
                    let next = !controller
                        .bool_setting(RuntimeSettingKey::MotionDebugView)
                        .unwrap_or(self.show_motion_vectors);
                    controller
                        .transact()
                        .set_engine_value(RuntimeSettingKey::MotionDebugView, next)
                        .apply()?;
                    tracing::info!("motion vectors: {}", if next { "shown" } else { "hidden" });
                }
            } else {
                tracing::info!("motion-vector debug is a scene 5 temporal/post control");
            }
        } else if key == "T" || key == "t" {
            if let Some(controller) = runtime_controller.as_mut() {
                let next = next_tone_mapping(self.tone_mapping);
                controller
                    .transact()
                    .set_engine_value(
                        RuntimeSettingKey::ToneMappingOperator,
                        tone_mapping_setting_name(next),
                    )
                    .apply()?;
                tracing::info!("tone mapping: {}", tone_mapping_label(next));
            }
        } else if key == "H" || key == "h" {
            if let Some(controller) = runtime_controller.as_mut() {
                let next = !controller
                    .bool_setting(RuntimeSettingKey::HdrMode)
                    .unwrap_or(self.hdr_output);
                controller
                    .transact()
                    .set_engine_value(RuntimeSettingKey::HdrMode, next)
                    .apply()?;
                tracing::info!("HDR output requested: {}", if next { "on" } else { "off" });
            }
        } else if key == "P" || key == "p" {
            self.selected_tonemap_dial = self.selected_tonemap_dial.next();
            tracing::info!(
                "tonemap dial: {} = {:.3}",
                self.selected_tonemap_dial.label(),
                self.tonemap_settings.get(self.selected_tonemap_dial),
            );
        } else if key == "A" {
            if self.selected_scene == ShowcaseScene::CornellPathTracing {
                self.reset_path_tracer_accumulation();
                tracing::info!(
                    "path tracer accumulation cleared for {}",
                    self.path_tracer_subscene.label()
                );
            } else {
                tracing::info!("RT accumulation reset is a scene 8 path tracer control");
            }
        } else if key == "a" {
            let mut next = self.aa.clone();
            next.next_mode();
            if let Some(controller) = runtime_controller.as_mut() {
                controller
                    .transact()
                    .set_engine_value(
                        RuntimeSettingKey::AntiAliasingMode,
                        aa_mode_setting_name(next.mode),
                    )
                    .apply()?;
            }
            tracing::info!("aa mode: {}", next.mode.label());
        } else if key == "D" || key == "d" {
            self.aa.cycle_dial();
            tracing::info!("aa dial: {}", self.aa.selected_dial.label());
        } else if key == "O" || key == "o" {
            if let Some(controller) = runtime_controller.as_mut() {
                let visible = !controller
                    .bool_setting(RuntimeSettingKey::OverlayVisibility)
                    .unwrap_or(true);
                controller
                    .transact()
                    .set_engine_value(RuntimeSettingKey::OverlayVisibility, visible)
                    .apply()?;
                tracing::info!("overlay: {}", if visible { "shown" } else { "hidden" });
            }
        } else if key == "I" || key == "i" {
            if self.selected_scene == ShowcaseScene::DebugGraph {
                self.show_graph_inspector = !self.show_graph_inspector;
                tracing::info!(
                    "graph inspector: {}",
                    if self.show_graph_inspector {
                        "shown"
                    } else {
                        "hidden"
                    }
                );
            } else {
                tracing::info!("graph inspector is a scene 9 debug-graph control");
            }
        } else if key == "E" || key == "e" {
            if matches!(
                self.selected_scene,
                ShowcaseScene::DebugGraph | ShowcaseScene::CornellPathTracing
            ) {
                self.pending_debug_image_export = true;
                tracing::info!("debug image export queued");
            } else {
                tracing::info!("debug image export is a debug-graph or Cornell control");
            }
        } else if key == "X" || key == "x" {
            if let Some(controller) = runtime_controller.as_mut() {
                let enabled = !controller
                    .bool_setting(RuntimeSettingKey::SurfaceTransparency)
                    .unwrap_or(false);
                controller
                    .transact()
                    .set_engine_value(RuntimeSettingKey::SurfaceTransparency, enabled)
                    .apply()?;
                tracing::debug!(
                    "surface transparency: {}",
                    if enabled { "on" } else { "off" }
                );
            }
        } else if key == "G" || key == "g" {
            self.cycle_window_background_effect()?;
        } else if key == "N" || key == "n" {
            if matches!(
                self.selected_scene,
                ShowcaseScene::DebugGraph | ShowcaseScene::CornellPathTracing
            ) {
                if let Some(controller) = runtime_controller.as_mut() {
                    let selection = self
                        .debug_view_picker
                        .cycle_next(controller, &self.current_debug_image_names())?;
                    tracing::info!(
                        "debug view: {}",
                        selection.unwrap_or_else(|| "Off".to_string())
                    );
                }
            } else {
                tracing::info!("debug view cycling is a debug-graph or Cornell control");
            }
        } else if key == "M" || key == "m" {
            if matches!(
                self.selected_scene,
                ShowcaseScene::DebugGraph | ShowcaseScene::CornellPathTracing
            ) {
                if let Some(controller) = runtime_controller.as_mut() {
                    let selection = self
                        .debug_view_picker
                        .cycle_previous(controller, &self.current_debug_image_names())?;
                    tracing::info!(
                        "debug view: {}",
                        selection.unwrap_or_else(|| "Off".to_string())
                    );
                }
            } else {
                tracing::info!("debug view cycling is a debug-graph or Cornell control");
            }
        } else if key == "F1" {
            if self.selected_scene == ShowcaseScene::ProceduralTextures {
                self.set_texture_resolution_setting(TextureResolutionTier::Low)?;
            } else {
                tracing::info!("texture resolution is a scene 6 procedural-textures control");
            }
        } else if key == "F2" {
            if self.selected_scene == ShowcaseScene::ProceduralTextures {
                self.set_texture_resolution_setting(TextureResolutionTier::Medium)?;
            } else {
                tracing::info!("texture resolution is a scene 6 procedural-textures control");
            }
        } else if key == "F3" {
            if self.selected_scene == ShowcaseScene::ProceduralTextures {
                self.set_texture_resolution_setting(TextureResolutionTier::High)?;
            } else {
                tracing::info!("texture resolution is a scene 6 procedural-textures control");
            }
        } else if matches!(
            key,
            "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" | "PageUp" | "PageDown"
        ) {
            if self.selected_scene == ShowcaseScene::RealtimeShadows {
                self.shadow_scene.on_key(key);
            }
        } else if key == "]" || key == "=" || key == "+" {
            let value = self.preview_tonemap_dial(1.0);
            if let Some(controller) = runtime_controller.as_mut() {
                controller
                    .transact()
                    .set_engine_value(RuntimeSettingKey::ToneMappingDial, value as f64)
                    .apply()?;
            }
            tracing::info!(
                "{} {}: {:.3}",
                tone_mapping_label(self.tone_mapping),
                self.selected_tonemap_dial.label(),
                value
            );
        } else if key == "[" || key == "-" || key == "_" {
            let value = self.preview_tonemap_dial(-1.0);
            if let Some(controller) = runtime_controller.as_mut() {
                controller
                    .transact()
                    .set_engine_value(RuntimeSettingKey::ToneMappingDial, value as f64)
                    .apply()?;
            }
            tracing::info!(
                "{} {}: {:.3}",
                tone_mapping_label(self.tone_mapping),
                self.selected_tonemap_dial.label(),
                value
            );
        } else if key == "." || key == ">" {
            let value = self.preview_aa_dial(1.0);
            if let Some(controller) = runtime_controller.as_mut() {
                controller
                    .transact()
                    .set_engine_value(RuntimeSettingKey::AntiAliasingDial, value as f64)
                    .apply()?;
            }
            tracing::info!("aa {}: {:.3}", self.aa.selected_dial.label(), value);
        } else if key == "," || key == "<" {
            let value = self.preview_aa_dial(-1.0);
            if let Some(controller) = runtime_controller.as_mut() {
                controller
                    .transact()
                    .set_engine_value(RuntimeSettingKey::AntiAliasingDial, value as f64)
                    .apply()?;
            }
            tracing::info!("aa {}: {:.3}", self.aa.selected_dial.label(), value);
        } else if key == "R" || key == "r" {
            self.tonemap_settings.reset_for(self.tone_mapping);
            if let Some(controller) = runtime_controller.as_mut() {
                controller
                    .transact()
                    .set_engine_value(
                        RuntimeSettingKey::ToneMappingDial,
                        self.tonemap_settings.get(self.selected_tonemap_dial) as f64,
                    )
                    .apply()?;
            }
            tracing::info!(
                "reset {} tonemap dials",
                tone_mapping_label(self.tone_mapping)
            );
        } else if key == "U" || key == "u" {
            self.aa = AntiAliasingConfig::default();
            if let Some(controller) = runtime_controller.as_mut() {
                controller
                    .transact()
                    .set_engine_value(
                        RuntimeSettingKey::AntiAliasingMode,
                        aa_mode_setting_name(self.aa.mode),
                    )
                    .set_engine_value(
                        RuntimeSettingKey::AntiAliasingDial,
                        self.current_aa_dial_value() as f64,
                    )
                    .apply()?;
            }
            tracing::info!("reset aa dials");
        }
        Ok(())
    }

    fn runtime_settings_changed(
        &mut self,
        controller: &RuntimeController,
        changes: &[sturdy_engine::RuntimeSettingChange],
    ) -> EngineResult<()> {
        self.runtime_controller = Some(controller.clone());
        self.apply_runtime_settings(controller, changes)
    }

    fn resize(&mut self, _runtime: &mut AppRuntime, width: u32, height: u32) -> EngineResult<()> {
        self.reset_path_tracer_accumulation();
        self.engine
            .evict_cached_graph_images_with_prefix("cornell_rt_sample");
        self.engine
            .evict_cached_graph_images_with_prefix("cornell_accumulation");
        tracing::info!(
            "Cornell accumulation and cached images reset for resize to {}x{}",
            width,
            height
        );
        Ok(())
    }
}

impl Testbed {
    fn select_scene(&mut self, scene: ShowcaseScene) -> EngineResult<()> {
        if self.selected_scene != scene {
            self.reset_path_tracer_accumulation();
            self.selected_scene = scene;
            self.apply_scene_scoped_runtime_state()?;
        }
        tracing::info!("showcase scene: {}", scene.label());
        Ok(())
    }

    fn select_path_tracer_subscene_slot(&mut self, slot: u32) -> EngineResult<()> {
        let Some(subscene) = PathTracerSubscene::from_slot(slot) else {
            tracing::info!("path tracer subscene {slot} is not assigned yet");
            return Ok(());
        };

        let subscene_changed = self.path_tracer_subscene != subscene;
        if self.selected_scene != ShowcaseScene::CornellPathTracing {
            self.select_scene(ShowcaseScene::CornellPathTracing)?;
        }
        if subscene_changed {
            self.path_tracer_subscene = subscene;
            self.reset_path_tracer_accumulation();
        }

        tracing::info!(
            "path tracer subscene {}: {}",
            subscene.slot(),
            subscene.label()
        );
        Ok(())
    }

    fn reset_path_tracer_accumulation(&mut self) {
        self.pt_history.reset();
        self.previous_path_tracer_camera = self.path_tracer_camera.gpu_data();
        self.clear_path_tracer_after_first_trace = true;
    }

    fn advance_path_tracer_camera(&mut self, delta_seconds: f32) -> bool {
        if !self.outdoor_camera_active() {
            return false;
        }
        self.path_tracer_camera
            .apply_input(self.path_tracer_camera_input, delta_seconds)
    }

    fn outdoor_camera_active(&self) -> bool {
        self.selected_scene == ShowcaseScene::CornellPathTracing
            && self.path_tracer_subscene == PathTracerSubscene::Outdoor
    }

    fn set_path_tracer_camera_key(&mut self, key: &str, pressed: bool) {
        match key {
            "KeyW" => self.path_tracer_camera_input.forward = pressed,
            "KeyS" => self.path_tracer_camera_input.backward = pressed,
            "KeyA" => self.path_tracer_camera_input.left = pressed,
            "KeyD" => self.path_tracer_camera_input.right = pressed,
            "KeyE" => self.path_tracer_camera_input.up = pressed,
            "KeyQ" => self.path_tracer_camera_input.down = pressed,
            "ArrowLeft" => self.path_tracer_camera_input.yaw_left = pressed,
            "ArrowRight" => self.path_tracer_camera_input.yaw_right = pressed,
            "ArrowUp" => self.path_tracer_camera_input.pitch_up = pressed,
            "ArrowDown" => self.path_tracer_camera_input.pitch_down = pressed,
            _ => {}
        }
    }

    fn apply_scene_scoped_runtime_state(&mut self) -> EngineResult<()> {
        if self.selected_scene != ShowcaseScene::DebugGraph {
            self.show_graph_inspector = false;
            self.pending_debug_image_export = false;
            if let Some(controller) = self.runtime_controller.as_mut() {
                self.debug_view_picker.set_selected_name(controller, None)?;
            }
        }

        if self.selected_scene != ShowcaseScene::TemporalAndPost && self.show_motion_vectors {
            self.show_motion_vectors = false;
            if let Some(controller) = self.runtime_controller.as_mut() {
                controller
                    .transact()
                    .set_engine_value(RuntimeSettingKey::MotionDebugView, false)
                    .apply()?;
            }
        }

        Ok(())
    }

    fn register_runtime_settings(&mut self, controller: &RuntimeController) -> EngineResult<()> {
        controller.register_app_setting(
            RuntimeSettingDescriptor::new(
                RuntimeSettingId::app("testbed.texture_resolution"),
                "Procedural Texture Resolution",
                sturdy_engine::RuntimeApplyPath::Immediate,
                self.texture_resolution.label(),
            )
            .with_description(
                "Scene 6 / Procedural Textures: swap the procedural mask resolution immediately.",
            )
            .with_options(vec![
                RuntimeSettingOption {
                    value: "low".into(),
                    label: "Low".to_string(),
                },
                RuntimeSettingOption {
                    value: "medium".into(),
                    label: "Medium".to_string(),
                },
                RuntimeSettingOption {
                    value: "high".into(),
                    label: "High".to_string(),
                },
            ]),
        )?;
        controller.register_app_setting(
            RuntimeSettingDescriptor::new(
                RuntimeSettingId::app(PT_QUALITY_SETTING),
                "PT Accumulation Quality",
                sturdy_engine::RuntimeApplyPath::Immediate,
                self.pt_quality.label(),
            )
            .with_description(
                "Hardware path tracing: temporal history budget for accumulation and display denoise.",
            )
            .with_options(vec![
                RuntimeSettingOption {
                    value: PtQuality::Preview.label().into(),
                    label: PtQuality::Preview.display_label().to_string(),
                },
                RuntimeSettingOption {
                    value: PtQuality::Balanced.label().into(),
                    label: PtQuality::Balanced.display_label().to_string(),
                },
                RuntimeSettingOption {
                    value: PtQuality::Stable.label().into(),
                    label: PtQuality::Stable.display_label().to_string(),
                },
            ]),
        )?;
        controller.register_app_setting(
            RuntimeSettingDescriptor::new(
                RuntimeSettingId::app(PT_RESET_SETTING),
                "PT Reset History",
                sturdy_engine::RuntimeApplyPath::Immediate,
                false,
            )
            .with_description(
                "Hardware path tracing: clear temporal history on the next runtime setting apply.",
            )
            .with_options(vec![
                RuntimeSettingOption {
                    value: false.into(),
                    label: "No".to_string(),
                },
                RuntimeSettingOption {
                    value: true.into(),
                    label: "Reset now".to_string(),
                },
            ]),
        )?;
        self.debug_view_picker.register(controller)?;
        Ok(())
    }

    fn seed_runtime_settings(&mut self, controller: &RuntimeController) -> EngineResult<()> {
        let mut controller = controller.clone();
        controller
            .transact()
            .set_engine_value(RuntimeSettingKey::BloomEnabled, self.bloom_enabled)
            .set_engine_value(RuntimeSettingKey::BloomOnly, self.bloom_only)
            .set_engine_value(RuntimeSettingKey::MotionDebugView, self.show_motion_vectors)
            .set_engine_value(RuntimeSettingKey::HdrMode, self.hdr_output)
            .set_engine_value(
                RuntimeSettingKey::ToneMappingOperator,
                tone_mapping_setting_name(self.tone_mapping),
            )
            .set_engine_value(
                RuntimeSettingKey::ToneMappingDial,
                self.tonemap_settings.get(self.selected_tonemap_dial) as f64,
            )
            .set_engine_value(
                RuntimeSettingKey::AntiAliasingMode,
                aa_mode_setting_name(self.aa.mode),
            )
            .set_engine_value(
                RuntimeSettingKey::AntiAliasingDial,
                self.current_aa_dial_value() as f64,
            )
            .set_engine_value(RuntimeSettingKey::OverlayVisibility, true)
            .set_app_value(
                "testbed.texture_resolution",
                self.texture_resolution.label(),
            )
            .set_app_value(PT_QUALITY_SETTING, self.pt_quality.label())
            .set_app_value(PT_RESET_SETTING, false)
            .apply()?;
        Ok(())
    }

    fn apply_runtime_settings(
        &mut self,
        controller: &RuntimeController,
        changes: &[sturdy_engine::RuntimeSettingChange],
    ) -> EngineResult<()> {
        self.bloom_enabled = controller
            .bool_setting(RuntimeSettingKey::BloomEnabled)
            .unwrap_or(self.bloom_enabled);
        self.bloom_only = controller
            .bool_setting(RuntimeSettingKey::BloomOnly)
            .unwrap_or(self.bloom_only);
        self.show_motion_vectors = controller
            .bool_setting(RuntimeSettingKey::MotionDebugView)
            .unwrap_or(self.show_motion_vectors);
        self.hdr_output = controller.settings().hdr_enabled;
        if let Some(tone_mapping) = controller
            .text_setting(RuntimeSettingKey::ToneMappingOperator)
            .and_then(|value| parse_tone_mapping_setting(&value))
        {
            self.tone_mapping = tone_mapping;
        }
        if let Some(aa_mode) = controller
            .text_setting(RuntimeSettingKey::AntiAliasingMode)
            .and_then(|value| parse_aa_mode_setting(&value, self.actual_msaa_samples()))
        {
            self.aa.mode = aa_mode;
        }

        for change in changes {
            if change.setting == RuntimeSettingId::from(RuntimeSettingKey::ToneMappingDial)
                && let sturdy_engine::RuntimeSettingValue::Float(value) = change.value
            {
                self.apply_tonemap_dial_value(value as f32);
            }
            if change.setting == RuntimeSettingId::from(RuntimeSettingKey::AntiAliasingDial)
                && let sturdy_engine::RuntimeSettingValue::Float(value) = change.value
            {
                self.apply_aa_dial_value(value as f32);
            }
            if change.setting == RuntimeSettingId::app("testbed.texture_resolution")
                && let sturdy_engine::RuntimeSettingValue::Text(value) = &change.value
                && let Some(tier) = TextureResolutionTier::from_setting(value)
            {
                self.recreate_procedural_mask(tier)?;
            }
            if change.setting == RuntimeSettingId::app(PT_QUALITY_SETTING)
                && let sturdy_engine::RuntimeSettingValue::Text(value) = &change.value
                && let Some(preset) = PtQuality::from_setting(value)
            {
                self.apply_pt_quality(preset);
            }
            if change.setting == RuntimeSettingId::app(PT_RESET_SETTING)
                && let sturdy_engine::RuntimeSettingValue::Bool(true) = &change.value
            {
                self.reset_path_tracer_accumulation();
                controller
                    .clone()
                    .transact()
                    .set_app_value(PT_RESET_SETTING, false)
                    .apply()?;
            }
        }
        Ok(())
    }

    fn apply_pt_quality(&mut self, preset: PtQuality) {
        if self.pt_quality == preset {
            return;
        }
        self.pt_max_frames = preset.max_frames();
        self.pt_quality = preset;
        self.reset_path_tracer_accumulation();
        tracing::info!(
            quality = preset.label(),
            max_frames = preset.max_frames(),
            "PT accumulation quality preset changed"
        );
    }

    fn recreate_procedural_mask(&mut self, tier: TextureResolutionTier) -> EngineResult<()> {
        if self.texture_resolution == tier {
            return Ok(());
        }
        self.procedural_mask = CpuProceduralTexture2d::from_recipe_rgba8(
            &self.engine,
            "procedural_mask",
            tier.size(),
            tier.size(),
            ProceduralTextureUpdatePolicy::Once,
            ProceduralTextureRecipe::RadialMask {
                inner_radius: 0.18,
                outer_radius: 1.0,
                color: [255, 255, 255, 255],
            },
        )?;
        self.texture_resolution = tier;
        tracing::info!("texture resolution: {}", tier.label());
        Ok(())
    }

    fn current_debug_image_names(&self) -> Vec<String> {
        self.runtime_controller
            .as_ref()
            .map(|controller| controller.diagnostics().debug_images)
            .unwrap_or_default()
    }

    fn export_selected_debug_image(&mut self, shell_frame: &ShellFrame<'_>) -> EngineResult<()> {
        let controller = shell_frame.runtime_controller();
        let Some(name) = self.debug_view_picker.selected_name(&controller) else {
            tracing::warn!("debug image export skipped: debug view is Off");
            return Ok(());
        };
        if !shell_frame
            .debug_image_names()
            .iter()
            .any(|entry| entry == &name)
        {
            tracing::warn!("debug image export skipped: `{name}` is not available this frame");
            return Ok(());
        }

        self.debug_image_export_index += 1;
        let directory = PathBuf::from("debug-exports");
        std::fs::create_dir_all(&directory).map_err(|error| {
            Error::Unknown(format!(
                "failed to create debug export directory {}: {error}",
                directory.display()
            ))
        })?;
        let path = directory.join(format!(
            "{:04}-{}.png",
            self.debug_image_export_index,
            sanitize_debug_image_name(&name)
        ));
        let report = shell_frame.save_named_graph_image_png(&name, &path)?;
        tracing::info!(
            "exported debug image `{name}` to {} ({}x{} {:?}, submitted={}, waited={})",
            report.path.display(),
            report.width,
            report.height,
            report.format,
            report.flush.submitted,
            report.wait.waited
        );
        Ok(())
    }

    fn set_texture_resolution_setting(&mut self, tier: TextureResolutionTier) -> EngineResult<()> {
        if let Some(controller) = self.runtime_controller.as_mut() {
            controller
                .transact()
                .set_app_value("testbed.texture_resolution", tier.label())
                .apply()?;
        } else {
            self.recreate_procedural_mask(tier)?;
        }
        Ok(())
    }

    fn cycle_window_background_effect(&mut self) -> EngineResult<()> {
        let Some(controller) = self.runtime_controller.as_mut() else {
            return Ok(());
        };
        let entry = match controller.setting_entry(RuntimeSettingKey::WindowBackgroundEffect) {
            Some(entry) => entry,
            None => return Ok(()),
        };
        let options = entry.descriptor.options;
        if options.is_empty() {
            return Ok(());
        }
        let current = controller
            .text_setting(RuntimeSettingKey::WindowBackgroundEffect)
            .unwrap_or_else(|| "None".to_string());
        let current_index = options
            .iter()
            .position(|option| option.value.serialized() == current)
            .unwrap_or(0);
        let next = &options[(current_index + 1) % options.len()];
        controller
            .transact()
            .set_engine_value(
                RuntimeSettingKey::WindowBackgroundEffect,
                next.value.serialized(),
            )
            .apply()?;
        tracing::info!("window background effect: {}", next.label);
        Ok(())
    }

    fn preview_tonemap_dial(&self, direction: f32) -> f32 {
        let dial = self.selected_tonemap_dial;
        (self.tonemap_settings.get(dial) + dial.step() * direction).clamp(dial.min(), dial.max())
    }

    fn apply_tonemap_dial_value(&mut self, value: f32) {
        self.tonemap_settings.set(
            self.selected_tonemap_dial,
            value.clamp(
                self.selected_tonemap_dial.min(),
                self.selected_tonemap_dial.max(),
            ),
        );
        self.tonemap_settings
            .sync_operator_white_point(self.tone_mapping, self.selected_tonemap_dial);
    }

    fn preview_aa_dial(&self, direction: f32) -> f32 {
        let mut preview = self.aa.clone();
        preview.adjust(direction, self.engine.caps().max_color_sample_count);
        aa_dial_value(preview.mode, preview.selected_dial)
    }

    fn apply_aa_dial_value(&mut self, value: f32) {
        if self.aa.selected_dial == AntiAliasingDial::Mode {
            return;
        }
        apply_aa_value(
            &mut self.aa,
            value,
            self.engine.caps().max_color_sample_count,
        );
    }

    fn current_aa_dial_value(&self) -> f32 {
        aa_dial_value(self.aa.mode, self.aa.selected_dial)
    }

    fn draw_hud(
        &mut self,
        shell_frame: &ShellFrame<'_>,
        frame: &sturdy_engine::RenderFrame,
        target: &sturdy_engine::GraphImage,
        width: u32,
        height: u32,
    ) -> EngineResult<()> {
        let hud_text = std::iter::once("SturdyEngine testbed".to_string())
            .chain(shell_frame.runtime_overlay_lines())
            .chain(std::iter::once(
                "Resize window to test graph image recreation\nClose window to exit".to_string(),
            ))
            .collect::<Vec<_>>()
            .join("\n");
        let mut overlay = DebugOverlay::new();
        overlay.add_screen_text(hud_text, 18.0, 18.0);
        self.debug_overlay
            .draw(frame, target, width, height, &overlay)
    }

    fn actual_msaa_samples(&self) -> u8 {
        self.aa
            .mode
            .msaa_samples()
            .clamp(1, self.engine.caps().max_color_sample_count.max(1))
            .min(16)
    }

    fn motion_vector_image(
        &self,
        frame: &sturdy_engine::RenderFrame,
        width: u32,
        height: u32,
    ) -> EngineResult<sturdy_engine::GraphImage> {
        frame.image(
            "motion_vectors",
            ImageDesc {
                dimension: ImageDimension::D2,
                extent: Extent3d {
                    width: width.max(1),
                    height: height.max(1),
                    depth: 1,
                },
                mip_levels: 1,
                layers: 1,
                samples: 1,
                format: Format::Rgba16Float,
                usage: ImageUsage::SAMPLED | ImageUsage::RENDER_TARGET,
                transient: false,
                clear_value: None,
                debug_name: Some("testbed motion vector"),
                ..ImageDesc::new()
            },
        )
    }
}

fn next_tone_mapping(op: ToneMappingOp) -> ToneMappingOp {
    match op {
        ToneMappingOp::Aces => ToneMappingOp::Reinhard,
        ToneMappingOp::Reinhard => ToneMappingOp::Hermite,
        ToneMappingOp::Hermite => ToneMappingOp::Linear,
        ToneMappingOp::Linear => ToneMappingOp::PbrNeutral,
        ToneMappingOp::PbrNeutral => ToneMappingOp::AgX,
        ToneMappingOp::AgX => ToneMappingOp::PsychoV11,
        ToneMappingOp::PsychoV11 => ToneMappingOp::PsychoV17,
        ToneMappingOp::PsychoV17 => ToneMappingOp::Aces,
    }
}

fn tone_mapping_label(op: ToneMappingOp) -> &'static str {
    match op {
        ToneMappingOp::Aces => "ACES",
        ToneMappingOp::Reinhard => "Reinhard",
        ToneMappingOp::Hermite => "Hermite",
        ToneMappingOp::Linear => "Linear",
        ToneMappingOp::PbrNeutral => "PBR Neutral",
        ToneMappingOp::AgX => "AgX",
        ToneMappingOp::PsychoV11 => "PsychoV-11",
        ToneMappingOp::PsychoV17 => "PsychoV-17",
    }
}

fn tone_mapping_setting_name(op: ToneMappingOp) -> &'static str {
    match op {
        ToneMappingOp::Aces => "Aces",
        ToneMappingOp::Reinhard => "Reinhard",
        ToneMappingOp::Hermite => "Hermite",
        ToneMappingOp::Linear => "Linear",
        ToneMappingOp::PbrNeutral => "PbrNeutral",
        ToneMappingOp::AgX => "AgX",
        ToneMappingOp::PsychoV11 => "PsychoV11",
        ToneMappingOp::PsychoV17 => "PsychoV17",
    }
}

fn parse_tone_mapping_setting(value: &str) -> Option<ToneMappingOp> {
    match value {
        "Aces" | "ACES" => Some(ToneMappingOp::Aces),
        "Reinhard" => Some(ToneMappingOp::Reinhard),
        "Hermite" => Some(ToneMappingOp::Hermite),
        "Linear" => Some(ToneMappingOp::Linear),
        "PbrNeutral" | "PBR Neutral" => Some(ToneMappingOp::PbrNeutral),
        "AgX" | "agx" => Some(ToneMappingOp::AgX),
        "PsychoV11" | "PsychoV-11" => Some(ToneMappingOp::PsychoV11),
        "PsychoV17" | "PsychoV-17" => Some(ToneMappingOp::PsychoV17),
        _ => None,
    }
}

fn aa_mode_setting_name(mode: sturdy_engine::AntiAliasingMode) -> &'static str {
    match mode {
        sturdy_engine::AntiAliasingMode::Off => "Off",
        sturdy_engine::AntiAliasingMode::Msaa(_) => "MSAA",
        sturdy_engine::AntiAliasingMode::Fxaa(_) => "FXAA",
        sturdy_engine::AntiAliasingMode::Taa(_) => "TAA",
        sturdy_engine::AntiAliasingMode::FxaaTaa { .. } => "FXAA+TAA",
    }
}

fn parse_aa_mode_setting(
    value: &str,
    current_msaa_samples: u8,
) -> Option<sturdy_engine::AntiAliasingMode> {
    match value {
        "Off" | "off" => Some(sturdy_engine::AntiAliasingMode::Off),
        "MSAA" => Some(sturdy_engine::AntiAliasingMode::Msaa(
            sturdy_engine::MsaaSettings {
                samples: current_msaa_samples.max(1),
            },
        )),
        "FXAA" => Some(sturdy_engine::AntiAliasingMode::Fxaa(Default::default())),
        "TAA" => Some(sturdy_engine::AntiAliasingMode::Taa(Default::default())),
        "FXAA+TAA" => Some(sturdy_engine::AntiAliasingMode::FxaaTaa {
            fxaa: Default::default(),
            taa: Default::default(),
        }),
        _ => None,
    }
}

fn aa_dial_value(mode: sturdy_engine::AntiAliasingMode, dial: AntiAliasingDial) -> f32 {
    match (mode, dial) {
        (sturdy_engine::AntiAliasingMode::Msaa(settings), AntiAliasingDial::MsaaSamples) => {
            settings.samples as f32
        }
        (
            sturdy_engine::AntiAliasingMode::Fxaa(settings),
            AntiAliasingDial::FxaaSubpixelQuality,
        )
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { fxaa: settings, .. },
            AntiAliasingDial::FxaaSubpixelQuality,
        ) => settings.subpixel_quality,
        (sturdy_engine::AntiAliasingMode::Fxaa(settings), AntiAliasingDial::FxaaEdgeThreshold)
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { fxaa: settings, .. },
            AntiAliasingDial::FxaaEdgeThreshold,
        ) => settings.edge_threshold,
        (
            sturdy_engine::AntiAliasingMode::Fxaa(settings),
            AntiAliasingDial::FxaaEdgeThresholdMin,
        )
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { fxaa: settings, .. },
            AntiAliasingDial::FxaaEdgeThresholdMin,
        ) => settings.edge_threshold_min,
        (sturdy_engine::AntiAliasingMode::Taa(settings), AntiAliasingDial::TaaHistoryWeight)
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { taa: settings, .. },
            AntiAliasingDial::TaaHistoryWeight,
        ) => settings.history_weight,
        (sturdy_engine::AntiAliasingMode::Taa(settings), AntiAliasingDial::TaaJitterScale)
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { taa: settings, .. },
            AntiAliasingDial::TaaJitterScale,
        ) => settings.jitter_scale,
        (sturdy_engine::AntiAliasingMode::Taa(settings), AntiAliasingDial::TaaClampFactor)
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { taa: settings, .. },
            AntiAliasingDial::TaaClampFactor,
        ) => settings.clamp_factor,
        _ => 1.0,
    }
}

fn apply_aa_value(config: &mut AntiAliasingConfig, value: f32, max_msaa_samples: u8) {
    match (&mut config.mode, config.selected_dial) {
        (sturdy_engine::AntiAliasingMode::Msaa(settings), AntiAliasingDial::MsaaSamples) => {
            let rounded = value.round().clamp(1.0, max_msaa_samples.max(1) as f32);
            let candidates = [1.0_f32, 2.0, 4.0, 8.0, 16.0];
            settings.samples = candidates
                .into_iter()
                .min_by(|left, right| {
                    (left - rounded)
                        .abs()
                        .partial_cmp(&(right - rounded).abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap_or(1.0)
                .min(max_msaa_samples.max(1) as f32) as u8;
        }
        (
            sturdy_engine::AntiAliasingMode::Fxaa(settings),
            AntiAliasingDial::FxaaSubpixelQuality,
        )
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { fxaa: settings, .. },
            AntiAliasingDial::FxaaSubpixelQuality,
        ) => settings.subpixel_quality = value.clamp(0.0, 1.0),
        (sturdy_engine::AntiAliasingMode::Fxaa(settings), AntiAliasingDial::FxaaEdgeThreshold)
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { fxaa: settings, .. },
            AntiAliasingDial::FxaaEdgeThreshold,
        ) => settings.edge_threshold = value.clamp(0.0, 1.0),
        (
            sturdy_engine::AntiAliasingMode::Fxaa(settings),
            AntiAliasingDial::FxaaEdgeThresholdMin,
        )
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { fxaa: settings, .. },
            AntiAliasingDial::FxaaEdgeThresholdMin,
        ) => settings.edge_threshold_min = value.clamp(0.0, 1.0),
        (sturdy_engine::AntiAliasingMode::Taa(settings), AntiAliasingDial::TaaHistoryWeight)
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { taa: settings, .. },
            AntiAliasingDial::TaaHistoryWeight,
        ) => settings.history_weight = value.clamp(0.0, 1.0),
        (sturdy_engine::AntiAliasingMode::Taa(settings), AntiAliasingDial::TaaJitterScale)
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { taa: settings, .. },
            AntiAliasingDial::TaaJitterScale,
        ) => settings.jitter_scale = value.max(0.0),
        (sturdy_engine::AntiAliasingMode::Taa(settings), AntiAliasingDial::TaaClampFactor)
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { taa: settings, .. },
            AntiAliasingDial::TaaClampFactor,
        ) => settings.clamp_factor = value.max(0.0),
        _ => {}
    }
}

fn surface_is_hdr(color_space: SurfaceColorSpace) -> bool {
    matches!(
        color_space,
        SurfaceColorSpace::ExtendedSrgbLinear
            | SurfaceColorSpace::Hdr10St2084
            | SurfaceColorSpace::Hdr10Hlg
    )
}

fn shader_path(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(name)
}

fn sanitize_debug_image_name(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "debug-image".to_string()
    } else {
        sanitized
    }
}

fn main() {
    init_testbed_tracing();
    run_with_runtime::<Testbed>(
        WindowConfig::new("SturdyEngine Systems Showcase", 1600, 900)
            .with_resizable(true)
            .with_hdr(true),
    );
}

fn testbed_default_tracing_filter() -> &'static str {
    if cfg!(debug_assertions) {
        "trace"
    } else {
        "warn"
    }
}

fn init_testbed_tracing() {
    let default_filter = testbed_default_tracing_filter();
    let installed = init_tracing_with_default_filter(default_filter);
    ensure_testbed_tracing_filter();
    tracing::warn!(
        default_filter,
        installed,
        rust_log_was_set = std::env::var_os("RUST_LOG").is_some(),
        "testbed tracing ready"
    );
}

fn ensure_testbed_tracing_filter() {
    if std::env::var_os("RUST_LOG").is_none() {
        set_log_level(testbed_default_tracing_filter());
    }
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
