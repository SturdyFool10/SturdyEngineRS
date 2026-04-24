use crate::{WindowEffectRegion, WindowMaterialKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum WindowAppearancePreset {
    None,
    Transparent,
    Blur,
    ThinMaterial,
    ThickMaterial,
    NoiseMaterial,
    TitlebarMaterial,
    HudMaterial,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SurfaceTransparency {
    Disabled,
    Enabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum WindowCornerStyle {
    Default,
    Rounded,
    Square,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum WindowShadowMode {
    Default,
    Enabled,
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum WindowEffectQuality {
    Default,
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowTransparencyDesc {
    pub opacity: f32,
}

impl Default for WindowTransparencyDesc {
    fn default() -> Self {
        Self { opacity: 1.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowBlurDesc {
    pub radius: Option<f32>,
    pub tint: Option<[f32; 4]>,
    pub opacity: f32,
    pub region: WindowEffectRegion,
    pub quality: WindowEffectQuality,
}

impl Default for WindowBlurDesc {
    fn default() -> Self {
        Self {
            radius: None,
            tint: None,
            opacity: 1.0,
            region: WindowEffectRegion::FullWindow,
            quality: WindowEffectQuality::Default,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowMaterialDesc {
    pub kind: WindowMaterialKind,
    pub tint: Option<[f32; 4]>,
    pub fallback_blur: Option<WindowBlurDesc>,
    pub region: WindowEffectRegion,
}

impl Default for WindowMaterialDesc {
    fn default() -> Self {
        Self {
            kind: WindowMaterialKind::Auto,
            tint: None,
            fallback_blur: None,
            region: WindowEffectRegion::FullWindow,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WindowBackdrop {
    None,
    Transparent(WindowTransparencyDesc),
    Blurred(WindowBlurDesc),
    Material(WindowMaterialDesc),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowAppearance {
    pub transparency: SurfaceTransparency,
    pub backdrop: WindowBackdrop,
    pub corner_style: Option<WindowCornerStyle>,
    pub shadow: WindowShadowMode,
}

impl Default for WindowAppearance {
    fn default() -> Self {
        Self {
            transparency: SurfaceTransparency::Disabled,
            backdrop: WindowBackdrop::None,
            corner_style: None,
            shadow: WindowShadowMode::Default,
        }
    }
}

impl WindowAppearance {
    pub fn transparent() -> Self {
        Self {
            transparency: SurfaceTransparency::Enabled,
            backdrop: WindowBackdrop::Transparent(WindowTransparencyDesc::default()),
            ..Self::default()
        }
    }

    pub fn blurred() -> Self {
        Self {
            transparency: SurfaceTransparency::Enabled,
            backdrop: WindowBackdrop::Blurred(WindowBlurDesc::default()),
            ..Self::default()
        }
    }

    pub fn material(kind: WindowMaterialKind) -> Self {
        Self {
            transparency: SurfaceTransparency::Enabled,
            backdrop: WindowBackdrop::Material(WindowMaterialDesc {
                kind,
                ..WindowMaterialDesc::default()
            }),
            ..Self::default()
        }
    }

    pub fn from_preset(preset: WindowAppearancePreset) -> Self {
        match preset {
            WindowAppearancePreset::None => Self::default(),
            WindowAppearancePreset::Transparent => Self::transparent(),
            WindowAppearancePreset::Blur => Self::blurred(),
            WindowAppearancePreset::ThinMaterial => {
                Self::material(WindowMaterialKind::ThinTranslucent)
            }
            WindowAppearancePreset::ThickMaterial => {
                Self::material(WindowMaterialKind::ThickTranslucent)
            }
            WindowAppearancePreset::NoiseMaterial => {
                Self::material(WindowMaterialKind::NoiseTranslucent)
            }
            WindowAppearancePreset::TitlebarMaterial => {
                Self::material(WindowMaterialKind::TitlebarTranslucent)
            }
            WindowAppearancePreset::HudMaterial => Self::material(WindowMaterialKind::Hud),
        }
    }

    pub fn preset_name(preset: WindowAppearancePreset) -> &'static str {
        match preset {
            WindowAppearancePreset::None => "None",
            WindowAppearancePreset::Transparent => "Transparent",
            WindowAppearancePreset::Blur => "Blur",
            WindowAppearancePreset::ThinMaterial => "ThinTranslucent",
            WindowAppearancePreset::ThickMaterial => "ThickTranslucent",
            WindowAppearancePreset::NoiseMaterial => "NoiseTranslucent",
            WindowAppearancePreset::TitlebarMaterial => "TitlebarTranslucent",
            WindowAppearancePreset::HudMaterial => "Hud",
        }
    }
}
