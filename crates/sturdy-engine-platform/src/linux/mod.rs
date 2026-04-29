use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use crate::{
    NativeWindowAppearanceError, PlatformCapabilityState, PlatformKind, WindowAppearance,
    WindowAppearanceCaps,
};

mod wayland;

/// Linux platform adapter entry point.
///
/// Planned backdrop/effect support:
/// - Wayland `ext-background-effect-v1` as the primary protocol path
/// - older compositor-specific blur protocols only as compatibility fallbacks
/// - clean fallback to transparency/no backdrop effect when no supported
///   compositor protocol is available
pub fn platform_kind() -> PlatformKind {
    PlatformKind::Linux
}

pub fn window_appearance_caps() -> WindowAppearanceCaps {
    WindowAppearanceCaps {
        transparency: Some(PlatformCapabilityState::Supported),
        blur: Some(PlatformCapabilityState::RuntimeReconfigureSupported),
        materials: Vec::new(),
        custom_regions: Some(PlatformCapabilityState::Unsupported),
        corner_style: Some(PlatformCapabilityState::Unsupported),
        live_reconfiguration: Some(PlatformCapabilityState::RuntimeReconfigureSupported),
    }
}

pub fn apply_native_window_appearance(
    display: RawDisplayHandle,
    window: RawWindowHandle,
    size: Option<(u32, u32)>,
    appearance: WindowAppearance,
) -> Result<(), NativeWindowAppearanceError> {
    wayland::apply_native_window_appearance(display, window, size, appearance)
}
