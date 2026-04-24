use crate::{
    NativeWindowAppearanceError, PlatformCapabilityState, PlatformKind, WindowAppearance,
    WindowAppearanceCaps, WindowEffectQuality, WindowMaterialKind, WindowMaterialSupport,
};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
#[cfg(target_os = "windows")]
use crate::{WindowBackdrop, WindowCornerStyle, WindowShadowMode};
#[cfg(target_os = "windows")]
use windows_sys::Win32::{
    Foundation::{FALSE, HWND, TRUE},
    Graphics::Dwm::{
        DWM_SYSTEMBACKDROP_TYPE, DWM_WINDOW_CORNER_PREFERENCE, DWMWA_NCRENDERING_POLICY,
        DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_HOSTBACKDROPBRUSH, DWMWA_WINDOW_CORNER_PREFERENCE,
        DWMWA_WINDOW_CORNER_PREFERENCE_ROUND, DWMWA_WINDOW_CORNER_PREFERENCE_ROUNDSMALL,
        DWMWA_WINDOW_CORNER_PREFERENCE_DEFAULT, DWMWA_WINDOW_CORNER_PREFERENCE_DONOTROUND,
        DWMNCRP_DISABLED, DWMNCRP_ENABLED, DWMSBT_AUTO, DWMSBT_MAINWINDOW,
        DWMSBT_NONE, DWMSBT_TABBEDWINDOW, DWMSBT_TRANSIENTWINDOW, DwmExtendFrameIntoClientArea,
        DwmSetWindowAttribute, MARGINS,
    },
};

pub fn platform_kind() -> PlatformKind {
    PlatformKind::Windows
}

pub fn window_appearance_caps() -> WindowAppearanceCaps {
    WindowAppearanceCaps {
        transparency: Some(PlatformCapabilityState::RuntimeReconfigureSupported),
        blur: Some(PlatformCapabilityState::RuntimeReconfigureSupported),
        materials: vec![
            WindowMaterialSupport {
                kind: WindowMaterialKind::Auto,
                quality: WindowEffectQuality::Default,
            },
            WindowMaterialSupport {
                kind: WindowMaterialKind::ThinTranslucent,
                quality: WindowEffectQuality::High,
            },
            WindowMaterialSupport {
                kind: WindowMaterialKind::ThickTranslucent,
                quality: WindowEffectQuality::Medium,
            },
            WindowMaterialSupport {
                kind: WindowMaterialKind::NoiseTranslucent,
                quality: WindowEffectQuality::Medium,
            },
            WindowMaterialSupport {
                kind: WindowMaterialKind::TitlebarTranslucent,
                quality: WindowEffectQuality::High,
            },
            WindowMaterialSupport {
                kind: WindowMaterialKind::Hud,
                quality: WindowEffectQuality::Medium,
            },
        ],
        custom_regions: Some(PlatformCapabilityState::Unsupported),
        live_reconfiguration: Some(PlatformCapabilityState::RuntimeReconfigureSupported),
    }
}

#[cfg(target_os = "windows")]
pub fn apply_native_window_appearance(
    _display: RawDisplayHandle,
    window: RawWindowHandle,
    _size: Option<(u32, u32)>,
    appearance: WindowAppearance,
) -> Result<(), NativeWindowAppearanceError> {
    let RawWindowHandle::Win32(handle) = window else {
        return Err(NativeWindowAppearanceError::UnsupportedWindowHandle);
    };
    let hwnd = handle.hwnd.get() as HWND;
    unsafe {
        set_backdrop(hwnd, appearance)?;
        set_frame_margins(hwnd, appearance)?;
        set_corner_style(hwnd, appearance)?;
        set_shadow(hwnd, appearance)?;
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn apply_native_window_appearance(
    _display: RawDisplayHandle,
    _window: RawWindowHandle,
    _size: Option<(u32, u32)>,
    _appearance: WindowAppearance,
) -> Result<(), NativeWindowAppearanceError> {
    Err(NativeWindowAppearanceError::PlatformUnavailable(
        "Windows native appearance is only available on Windows",
    ))
}

#[cfg(target_os = "windows")]
unsafe fn set_backdrop(
    hwnd: HWND,
    appearance: WindowAppearance,
) -> Result<(), NativeWindowAppearanceError> {
    let backdrop = windows_backdrop_for_appearance(appearance);
    let status = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            &backdrop as *const _ as *const _,
            std::mem::size_of::<DWM_SYSTEMBACKDROP_TYPE>() as u32,
        )
    };
    if status != 0 {
        return Err(NativeWindowAppearanceError::ApplyFailed(format!(
            "DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE) failed with HRESULT {status:#x}",
        )));
    }

    let host_brush = if matches!(appearance.backdrop, WindowBackdrop::Transparent(_)) {
        TRUE
    } else {
        FALSE
    };
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_HOSTBACKDROPBRUSH,
            &host_brush as *const _ as *const _,
            std::mem::size_of_val(&host_brush) as u32,
        )
    };
    Ok(())
}

#[cfg(target_os = "windows")]
unsafe fn set_frame_margins(
    hwnd: HWND,
    appearance: WindowAppearance,
) -> Result<(), NativeWindowAppearanceError> {
    let wants_glass = !matches!(appearance.backdrop, WindowBackdrop::None);
    let margins = if wants_glass {
        MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        }
    } else {
        MARGINS {
            cxLeftWidth: 0,
            cxRightWidth: 0,
            cyTopHeight: 0,
            cyBottomHeight: 0,
        }
    };
    let status = unsafe { DwmExtendFrameIntoClientArea(hwnd, &margins) };
    if status != 0 {
        return Err(NativeWindowAppearanceError::ApplyFailed(format!(
            "DwmExtendFrameIntoClientArea failed with HRESULT {status:#x}",
        )));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
unsafe fn set_corner_style(
    hwnd: HWND,
    appearance: WindowAppearance,
) -> Result<(), NativeWindowAppearanceError> {
    let preference = match appearance.corner_style.unwrap_or(WindowCornerStyle::Default) {
        WindowCornerStyle::Default => DWMWA_WINDOW_CORNER_PREFERENCE_DEFAULT,
        WindowCornerStyle::Rounded => {
            if matches!(appearance.backdrop, WindowBackdrop::Material(_)) {
                DWMWA_WINDOW_CORNER_PREFERENCE_ROUNDSMALL
            } else {
                DWMWA_WINDOW_CORNER_PREFERENCE_ROUND
            }
        }
        WindowCornerStyle::Square => DWMWA_WINDOW_CORNER_PREFERENCE_DONOTROUND,
    };
    let status = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const _ as *const _,
            std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        )
    };
    if status != 0 {
        return Err(NativeWindowAppearanceError::ApplyFailed(format!(
            "DwmSetWindowAttribute(DWMWA_WINDOW_CORNER_PREFERENCE) failed with HRESULT {status:#x}",
        )));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
unsafe fn set_shadow(
    hwnd: HWND,
    appearance: WindowAppearance,
) -> Result<(), NativeWindowAppearanceError> {
    let policy: i32 = match appearance.shadow {
        WindowShadowMode::Default | WindowShadowMode::Enabled => DWMNCRP_ENABLED,
        WindowShadowMode::Disabled => DWMNCRP_DISABLED,
    };
    let status = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_NCRENDERING_POLICY,
            &policy as *const _ as *const _,
            std::mem::size_of_val(&policy) as u32,
        )
    };
    if status != 0 {
        return Err(NativeWindowAppearanceError::ApplyFailed(format!(
            "DwmSetWindowAttribute(DWMWA_NCRENDERING_POLICY) failed with HRESULT {status:#x}",
        )));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_backdrop_for_appearance(appearance: WindowAppearance) -> DWM_SYSTEMBACKDROP_TYPE {
    match appearance.backdrop {
        WindowBackdrop::Material(material) => match material.kind {
            WindowMaterialKind::Auto => DWMSBT_AUTO,
            WindowMaterialKind::ThinTranslucent => DWMSBT_MAINWINDOW,
            WindowMaterialKind::NoiseTranslucent => DWMSBT_TRANSIENTWINDOW,
            WindowMaterialKind::TitlebarTranslucent => DWMSBT_TABBEDWINDOW,
            WindowMaterialKind::ThickTranslucent | WindowMaterialKind::Hud => {
                DWMSBT_TRANSIENTWINDOW
            }
        },
        WindowBackdrop::Blurred(_) => DWMSBT_TRANSIENTWINDOW,
        WindowBackdrop::Transparent(_) | WindowBackdrop::None => DWMSBT_NONE,
    }
}
