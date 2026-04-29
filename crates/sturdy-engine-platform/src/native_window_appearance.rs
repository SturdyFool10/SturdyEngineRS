use std::fmt;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};

use crate::{WindowAppearance, WindowBackdrop, current_platform};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeWindowAppearanceError {
    UnsupportedWindowHandle,
    UnsupportedDisplayHandle,
    PlatformUnavailable(&'static str),
    Degraded(String),
    ApplyFailed(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeWindowAppearanceStatus {
    Applied,
    Degraded,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeWindowAppearanceApplyReport {
    pub requested: &'static str,
    pub protocol: &'static str,
    pub status: NativeWindowAppearanceStatus,
    pub fallback: Option<&'static str>,
    pub reason: Option<String>,
}

impl NativeWindowAppearanceApplyReport {
    pub fn applied(appearance: WindowAppearance) -> Self {
        Self {
            requested: requested_backdrop_name(appearance),
            protocol: native_window_appearance_protocol(appearance),
            status: NativeWindowAppearanceStatus::Applied,
            fallback: None,
            reason: None,
        }
    }

    pub fn from_error(appearance: WindowAppearance, error: NativeWindowAppearanceError) -> Self {
        let status = if error.is_degraded() {
            NativeWindowAppearanceStatus::Degraded
        } else {
            NativeWindowAppearanceStatus::Failed
        };
        Self {
            requested: requested_backdrop_name(appearance),
            protocol: native_window_appearance_protocol(appearance),
            status,
            fallback: Some("winit"),
            reason: Some(error.to_string()),
        }
    }

    pub fn is_degraded(&self) -> bool {
        self.status == NativeWindowAppearanceStatus::Degraded
    }

    pub fn is_failed(&self) -> bool {
        self.status == NativeWindowAppearanceStatus::Failed
    }

    pub fn diagnostic_string(&self) -> String {
        let status = match self.status {
            NativeWindowAppearanceStatus::Applied => "applied",
            NativeWindowAppearanceStatus::Degraded => "degraded",
            NativeWindowAppearanceStatus::Failed => "failed",
        };
        match (self.fallback, &self.reason) {
            (Some(fallback), Some(reason)) => format!(
                "protocol={} requested={} status={status} fallback={fallback} reason={reason}",
                self.protocol, self.requested
            ),
            _ => format!(
                "protocol={} requested={} status={status}",
                self.protocol, self.requested
            ),
        }
    }
}

impl fmt::Display for NativeWindowAppearanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedWindowHandle => write!(f, "unsupported native window handle"),
            Self::UnsupportedDisplayHandle => write!(f, "unsupported native display handle"),
            Self::PlatformUnavailable(reason) => write!(f, "{reason}"),
            Self::Degraded(reason) => write!(f, "degraded native window appearance: {reason}"),
            Self::ApplyFailed(reason) => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for NativeWindowAppearanceError {}

impl NativeWindowAppearanceError {
    pub fn is_degraded(&self) -> bool {
        matches!(self, Self::Degraded(_))
    }
}

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

pub fn apply_native_window_appearance_report_for_window(
    window: &(impl HasWindowHandle + HasDisplayHandle),
    size: Option<(u32, u32)>,
    appearance: WindowAppearance,
) -> NativeWindowAppearanceApplyReport {
    match apply_native_window_appearance_for_window(window, size, appearance) {
        Ok(()) => NativeWindowAppearanceApplyReport::applied(appearance),
        Err(error) => NativeWindowAppearanceApplyReport::from_error(appearance, error),
    }
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

pub fn requested_backdrop_name(appearance: WindowAppearance) -> &'static str {
    match appearance.backdrop {
        WindowBackdrop::None => "none",
        WindowBackdrop::Transparent(_) => "transparent",
        WindowBackdrop::Blurred(_) => "blur",
        WindowBackdrop::Material(_) => "material",
    }
}

pub fn appearance_wants_native_blur(appearance: WindowAppearance) -> bool {
    matches!(
        appearance.backdrop,
        WindowBackdrop::Blurred(_) | WindowBackdrop::Material(_)
    )
}

pub fn native_window_appearance_protocol(appearance: WindowAppearance) -> &'static str {
    if !appearance_wants_native_blur(appearance) {
        return "none";
    }

    match current_platform() {
        crate::PlatformKind::Windows => "windows/system-backdrop",
        crate::PlatformKind::Macos => "macos/native-visual-effect",
        crate::PlatformKind::Linux => "wayland/ext-background-effect-v1-or-kde-blur",
        crate::PlatformKind::Unknown => "unsupported",
    }
}
