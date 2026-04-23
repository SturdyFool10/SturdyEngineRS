/// The antialiasing strategy used by the UI or a rendering view.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum AntiAliasingMode {
    Off,
    Msaa(MsaaSettings),
    Fxaa(FxaaSettings),
    Taa(TaaSettings),
    FxaaTaa {
        fxaa: FxaaSettings,
        taa: TaaSettings,
    },
}

impl Default for AntiAliasingMode {
    fn default() -> Self {
        Self::Off
    }
}

impl AntiAliasingMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Msaa(_) => "MSAA",
            Self::Fxaa(_) => "FXAA",
            Self::Taa(_) => "TAA",
            Self::FxaaTaa { .. } => "FXAA+TAA",
        }
    }

    pub fn is_post_process(self) -> bool {
        matches!(self, Self::Fxaa(_) | Self::Taa(_) | Self::FxaaTaa { .. })
    }

    pub fn uses_taa(self) -> bool {
        matches!(self, Self::Taa(_) | Self::FxaaTaa { .. })
    }

    pub fn msaa_samples(self) -> u8 {
        match self {
            Self::Msaa(settings) => settings.samples,
            _ => 1,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct MsaaSettings {
    pub samples: u8,
}

impl Default for MsaaSettings {
    fn default() -> Self {
        Self { samples: 4 }
    }
}

impl MsaaSettings {
    pub fn new(samples: u8) -> Self {
        Self {
            samples: samples.clamp(1, 16),
        }
    }

    pub fn set_samples(mut self, samples: u8) -> Self {
        self.samples = samples.clamp(1, 16);
        self
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FxaaSettings {
    pub subpixel_quality: f32,
    pub edge_threshold: f32,
    pub edge_threshold_min: f32,
}

impl Default for FxaaSettings {
    fn default() -> Self {
        Self {
            subpixel_quality: 0.75,
            edge_threshold: 0.125,
            edge_threshold_min: 0.0312,
        }
    }
}

impl FxaaSettings {
    pub fn subpixel_quality(mut self, value: f32) -> Self {
        self.subpixel_quality = value.clamp(0.0, 1.0);
        self
    }

    pub fn edge_threshold(mut self, value: f32) -> Self {
        self.edge_threshold = value.clamp(0.0, 1.0);
        self
    }

    pub fn edge_threshold_min(mut self, value: f32) -> Self {
        self.edge_threshold_min = value.clamp(0.0, 1.0);
        self
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TaaSettings {
    pub history_weight: f32,
    pub jitter_scale: f32,
    pub clamp_factor: f32,
}

impl Default for TaaSettings {
    fn default() -> Self {
        Self {
            history_weight: 0.9,
            jitter_scale: 1.0,
            clamp_factor: 1.0,
        }
    }
}

impl TaaSettings {
    pub fn history_weight(mut self, value: f32) -> Self {
        self.history_weight = value.clamp(0.0, 1.0);
        self
    }

    pub fn jitter_scale(mut self, value: f32) -> Self {
        self.jitter_scale = value.max(0.0);
        self
    }

    pub fn clamp_factor(mut self, value: f32) -> Self {
        self.clamp_factor = value.max(0.0);
        self
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AntiAliasingDial {
    Mode,
    MsaaSamples,
    FxaaSubpixelQuality,
    FxaaEdgeThreshold,
    FxaaEdgeThresholdMin,
    TaaHistoryWeight,
    TaaJitterScale,
    TaaClampFactor,
}

impl AntiAliasingDial {
    pub fn next(self) -> Self {
        match self {
            Self::Mode => Self::MsaaSamples,
            Self::MsaaSamples => Self::FxaaSubpixelQuality,
            Self::FxaaSubpixelQuality => Self::FxaaEdgeThreshold,
            Self::FxaaEdgeThreshold => Self::FxaaEdgeThresholdMin,
            Self::FxaaEdgeThresholdMin => Self::TaaHistoryWeight,
            Self::TaaHistoryWeight => Self::TaaJitterScale,
            Self::TaaJitterScale => Self::TaaClampFactor,
            Self::TaaClampFactor => Self::Mode,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Mode => "aa mode",
            Self::MsaaSamples => "MSAA samples",
            Self::FxaaSubpixelQuality => "FXAA subpixel",
            Self::FxaaEdgeThreshold => "FXAA edge",
            Self::FxaaEdgeThresholdMin => "FXAA edge min",
            Self::TaaHistoryWeight => "TAA history",
            Self::TaaJitterScale => "TAA jitter",
            Self::TaaClampFactor => "TAA clamp",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AntiAliasingConfig {
    pub mode: AntiAliasingMode,
    pub selected_dial: AntiAliasingDial,
}

impl Default for AntiAliasingConfig {
    fn default() -> Self {
        Self {
            mode: AntiAliasingMode::Off,
            selected_dial: AntiAliasingDial::Mode,
        }
    }
}

impl AntiAliasingConfig {
    pub fn next_mode(&mut self) {
        self.mode = match self.mode {
            AntiAliasingMode::Off => AntiAliasingMode::Msaa(MsaaSettings::default()),
            AntiAliasingMode::Msaa(_) => AntiAliasingMode::Fxaa(FxaaSettings::default()),
            AntiAliasingMode::Fxaa(_) => AntiAliasingMode::Taa(TaaSettings::default()),
            AntiAliasingMode::Taa(_) => AntiAliasingMode::FxaaTaa {
                fxaa: FxaaSettings::default(),
                taa: TaaSettings::default(),
            },
            AntiAliasingMode::FxaaTaa { .. } => AntiAliasingMode::Off,
        };
    }

    pub fn cycle_dial(&mut self) {
        self.selected_dial = self.selected_dial.next();
    }

    pub fn adjust(&mut self, direction: f32, max_msaa_samples: u8) {
        let delta = direction.signum();
        match (&mut self.mode, self.selected_dial) {
            (AntiAliasingMode::Msaa(settings), AntiAliasingDial::MsaaSamples) => {
                let samples = adjust_msaa_samples(settings.samples, delta as i32, max_msaa_samples);
                settings.samples = samples;
            }
            (AntiAliasingMode::Fxaa(settings), AntiAliasingDial::FxaaSubpixelQuality)
            | (
                AntiAliasingMode::FxaaTaa { fxaa: settings, .. },
                AntiAliasingDial::FxaaSubpixelQuality,
            ) => {
                settings.subpixel_quality =
                    adjust_float(settings.subpixel_quality, delta, 0.0, 1.0, 0.05);
            }
            (AntiAliasingMode::Fxaa(settings), AntiAliasingDial::FxaaEdgeThreshold)
            | (
                AntiAliasingMode::FxaaTaa { fxaa: settings, .. },
                AntiAliasingDial::FxaaEdgeThreshold,
            ) => {
                settings.edge_threshold =
                    adjust_float(settings.edge_threshold, delta, 0.0, 1.0, 0.01);
            }
            (AntiAliasingMode::Fxaa(settings), AntiAliasingDial::FxaaEdgeThresholdMin)
            | (
                AntiAliasingMode::FxaaTaa { fxaa: settings, .. },
                AntiAliasingDial::FxaaEdgeThresholdMin,
            ) => {
                settings.edge_threshold_min =
                    adjust_float(settings.edge_threshold_min, delta, 0.0, 1.0, 0.005);
            }
            (AntiAliasingMode::Taa(settings), AntiAliasingDial::TaaHistoryWeight)
            | (
                AntiAliasingMode::FxaaTaa { taa: settings, .. },
                AntiAliasingDial::TaaHistoryWeight,
            ) => {
                settings.history_weight =
                    adjust_float(settings.history_weight, delta, 0.0, 1.0, 0.02);
            }
            (AntiAliasingMode::Taa(settings), AntiAliasingDial::TaaJitterScale)
            | (AntiAliasingMode::FxaaTaa { taa: settings, .. }, AntiAliasingDial::TaaJitterScale) =>
            {
                settings.jitter_scale = adjust_float(settings.jitter_scale, delta, 0.0, 4.0, 0.1);
            }
            (AntiAliasingMode::Taa(settings), AntiAliasingDial::TaaClampFactor)
            | (AntiAliasingMode::FxaaTaa { taa: settings, .. }, AntiAliasingDial::TaaClampFactor) =>
            {
                settings.clamp_factor = adjust_float(settings.clamp_factor, delta, 0.0, 4.0, 0.1);
            }
            _ => {}
        }
    }
}

fn adjust_msaa_samples(current: u8, delta: i32, max_supported: u8) -> u8 {
    let supported = max_supported.clamp(1, 16);
    let candidates = [1, 2, 4, 8, 16];
    let current_idx = candidates
        .iter()
        .position(|&value| value == current)
        .unwrap_or(0);
    let mut idx = current_idx as i32 + delta;
    idx = idx.clamp(0, candidates.len() as i32 - 1);
    let mut samples = candidates[idx as usize];
    if samples > supported {
        samples = supported;
    }
    samples.max(1)
}

fn adjust_float(current: f32, delta: f32, min: f32, max: f32, step: f32) -> f32 {
    (current + delta * step).clamp(min, max)
}
