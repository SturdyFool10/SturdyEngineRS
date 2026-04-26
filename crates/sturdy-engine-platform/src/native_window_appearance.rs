use std::fmt;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};

use crate::{WindowAppearance, current_platform};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeWindowAppearanceError {
    UnsupportedWindowHandle,
    UnsupportedDisplayHandle,
    PlatformUnavailable(&'static str),
    ApplyFailed(String),
}

impl fmt::Display for NativeWindowAppearanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedWindowHandle => write!(f, "unsupported native window handle"),
            Self::UnsupportedDisplayHandle => write!(f, "unsupported native display handle"),
            Self::PlatformUnavailable(reason) => write!(f, "{reason}"),
            Self::ApplyFailed(reason) => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for NativeWindowAppearanceError {}

pub fn apply_native_window_appearance_for_window(
    window: &(impl HasWindowHandle + HasDisplayHandle),
    size: Option<(u32, u32)>,
    appearance: WindowAppearance,
) -> Result<(), NativeWindowAppearanceError> {
    let display = window
        .display_handle()
        .map_err(|_| NativeWindowAppearanceError::UnsupportedDisplayHandle)?;
    let window = window
        .window_handle()
        .map_err(|_| NativeWindowAppearanceError::UnsupportedWindowHandle)?;
    apply_native_window_appearance(display.as_raw(), window.as_raw(), size, appearance)
}

pub fn apply_native_window_appearance(
    display: RawDisplayHandle,
    window: RawWindowHandle,
    size: Option<(u32, u32)>,
    appearance: WindowAppearance,
) -> Result<(), NativeWindowAppearanceError> {
    match current_platform() {
        #[cfg(target_os = "windows")]
        crate::PlatformKind::Windows => {
            crate::windows::apply_native_window_appearance(display, window, size, appearance)
        }
        #[cfg(target_os = "macos")]
        crate::PlatformKind::Macos => {
            crate::macos::apply_native_window_appearance(display, window, size, appearance)
        }
        #[cfg(target_os = "linux")]
        crate::PlatformKind::Linux => {
            crate::linux::apply_native_window_appearance(display, window, size, appearance)
        }
        _ => Err(NativeWindowAppearanceError::PlatformUnavailable(
            "native window appearance is not implemented on this platform",
        )),
    }
}
