pub const CORNELL_RT_MAX_ACCUMULATION_FRAMES: u32 = 65536;
pub const SRD_QUALITY_SETTING: &str = "testbed.srd_quality";
pub const SRD_RESET_SETTING: &str = "testbed.srd_reset_history";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SrdQualityPreset {
    Preview,
    Balanced,
    Stable,
}

impl SrdQualityPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Balanced => "balanced",
            Self::Stable => "stable",
        }
    }

    pub fn display_label(self) -> &'static str {
        match self {
            Self::Preview => "Preview",
            Self::Balanced => "Balanced",
            Self::Stable => "Stable",
        }
    }

    pub fn max_frames(self) -> u32 {
        match self {
            Self::Preview => 32,
            Self::Balanced => 512,
            Self::Stable => CORNELL_RT_MAX_ACCUMULATION_FRAMES,
        }
    }

    pub fn from_setting(value: &str) -> Option<Self> {
        match value {
            "preview" | "Preview" => Some(Self::Preview),
            "balanced" | "Balanced" => Some(Self::Balanced),
            "stable" | "Stable" => Some(Self::Stable),
            _ => None,
        }
    }
}
