#![allow(dead_code)]

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
#[path = "lib_tests.rs"]
mod tests;
