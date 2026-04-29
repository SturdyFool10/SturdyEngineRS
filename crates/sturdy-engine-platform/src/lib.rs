mod capability;
mod native_window_appearance;
mod platform;
mod window_appearance;
mod window_effect_region;
mod window_material_kind;

pub mod linux;
pub mod macos;
pub mod windows;

pub use capability::{PlatformCapabilityState, WindowAppearanceCaps, WindowMaterialSupport};
pub use native_window_appearance::{
    NativeWindowAppearanceApplyReport, NativeWindowAppearanceError, NativeWindowAppearanceStatus,
    appearance_wants_native_blur, apply_native_window_appearance,
    apply_native_window_appearance_for_window, apply_native_window_appearance_report_for_window,
    native_window_appearance_protocol, requested_backdrop_name,
};
pub use platform::{PlatformKind, current_platform};
pub use window_appearance::{
    SurfaceTransparency, WindowAppearance, WindowAppearancePreset, WindowBackdrop, WindowBlurDesc,
    WindowCornerStyle, WindowEffectQuality, WindowShadowMode, WindowTransparencyDesc,
};
pub use window_effect_region::WindowEffectRegion;
pub use window_material_kind::WindowMaterialKind;

pub fn current_window_appearance_caps() -> WindowAppearanceCaps {
    match current_platform() {
        PlatformKind::Windows => windows::window_appearance_caps(),
        PlatformKind::Macos => macos::window_appearance_caps(),
        PlatformKind::Linux => linux::window_appearance_caps(),
        PlatformKind::Unknown => WindowAppearanceCaps::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_presets_expand_to_expected_backdrops() {
        assert_eq!(
            WindowAppearance::from_preset(WindowAppearancePreset::Transparent).backdrop,
            WindowBackdrop::Transparent(WindowTransparencyDesc::default())
        );
        assert_eq!(
            WindowAppearance::from_preset(WindowAppearancePreset::Blur).backdrop,
            WindowBackdrop::Blurred(WindowBlurDesc::default())
        );
        assert!(matches!(
            WindowAppearance::from_preset(WindowAppearancePreset::TitlebarMaterial).backdrop,
            WindowBackdrop::Material(_)
        ));
    }

    #[test]
    fn native_window_appearance_error_marks_degraded_results() {
        let degraded = NativeWindowAppearanceError::Degraded("fallback".into());
        let failed = NativeWindowAppearanceError::ApplyFailed("failed".into());

        assert!(degraded.is_degraded());
        assert!(!failed.is_degraded());
        assert!(degraded.to_string().contains("degraded"));
    }

    #[test]
    fn native_window_appearance_report_formats_degraded_apply_result() {
        let report = NativeWindowAppearanceApplyReport::from_error(
            WindowAppearance::from_preset(WindowAppearancePreset::Blur),
            NativeWindowAppearanceError::Degraded("no compositor protocol".into()),
        );

        assert_eq!(report.status, NativeWindowAppearanceStatus::Degraded);
        assert_eq!(report.requested, "blur");
        assert_eq!(report.fallback, Some("winit"));
        assert!(report.diagnostic_string().contains("status=degraded"));
        assert!(report.diagnostic_string().contains("reason="));
    }

    #[test]
    fn current_window_appearance_caps_include_corner_style_policy() {
        let caps = current_window_appearance_caps();

        assert!(caps.corner_style.is_some());
    }
}
