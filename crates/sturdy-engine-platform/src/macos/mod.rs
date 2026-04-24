#[cfg(target_os = "macos")]
use std::{cell::RefCell, collections::HashMap};

#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSColor, NSView, NSVisualEffectBlendingMode,
    NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow,
    NSWindowOrderingMode,
};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use crate::{
    NativeWindowAppearanceError, PlatformCapabilityState, PlatformKind, WindowAppearance,
    WindowAppearanceCaps, WindowMaterialKind,
};
#[cfg(target_os = "macos")]
use crate::{SurfaceTransparency, WindowBackdrop, WindowShadowMode};

pub fn platform_kind() -> PlatformKind {
    PlatformKind::Macos
}

pub fn window_appearance_caps() -> WindowAppearanceCaps {
    WindowAppearanceCaps {
        transparency: Some(PlatformCapabilityState::RuntimeReconfigureSupported),
        blur: Some(PlatformCapabilityState::RuntimeReconfigureSupported),
        materials: vec![
            crate::WindowMaterialSupport {
                kind: WindowMaterialKind::Auto,
                quality: crate::WindowEffectQuality::Default,
            },
            crate::WindowMaterialSupport {
                kind: WindowMaterialKind::ThinTranslucent,
                quality: crate::WindowEffectQuality::High,
            },
            crate::WindowMaterialSupport {
                kind: WindowMaterialKind::ThickTranslucent,
                quality: crate::WindowEffectQuality::High,
            },
            crate::WindowMaterialSupport {
                kind: WindowMaterialKind::NoiseTranslucent,
                quality: crate::WindowEffectQuality::Medium,
            },
            crate::WindowMaterialSupport {
                kind: WindowMaterialKind::TitlebarTranslucent,
                quality: crate::WindowEffectQuality::High,
            },
            crate::WindowMaterialSupport {
                kind: WindowMaterialKind::Hud,
                quality: crate::WindowEffectQuality::High,
            },
        ],
        custom_regions: Some(PlatformCapabilityState::Unsupported),
        live_reconfiguration: Some(PlatformCapabilityState::RuntimeReconfigureSupported),
    }
}

#[cfg(target_os = "macos")]
thread_local! {
    static EFFECT_VIEWS: RefCell<HashMap<usize, Retained<NSVisualEffectView>>> =
        RefCell::new(HashMap::new());
}

#[cfg(target_os = "macos")]
pub fn apply_native_window_appearance(
    _display: RawDisplayHandle,
    window: RawWindowHandle,
    _size: Option<(u32, u32)>,
    appearance: WindowAppearance,
) -> Result<(), NativeWindowAppearanceError> {
    let RawWindowHandle::AppKit(handle) = window else {
        return Err(NativeWindowAppearanceError::UnsupportedWindowHandle);
    };
    let ns_view = unsafe { &*(handle.ns_view.as_ptr() as *mut NSView) };
    let ns_window = ns_view.window().ok_or_else(|| {
        NativeWindowAppearanceError::ApplyFailed("NSView was not attached to an NSWindow".into())
    })?;

    ns_window.setOpaque(appearance.transparency != SurfaceTransparency::Enabled);
    ns_window.setHasShadow(!matches!(appearance.shadow, WindowShadowMode::Disabled));
    ns_window.setTitlebarAppearsTransparent(matches!(
        appearance.backdrop,
        WindowBackdrop::Material(material)
            if material.kind == WindowMaterialKind::TitlebarTranslucent
    ));

    let clear = unsafe { NSColor::clearColor() };
    ns_window.setBackgroundColor(Some(&clear));

    let content_view = ns_window.contentView().ok_or_else(|| {
        NativeWindowAppearanceError::ApplyFailed("NSWindow had no content view".into())
    })?;
    sync_effect_view(ns_view, &content_view, appearance);
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn apply_native_window_appearance(
    _display: RawDisplayHandle,
    _window: RawWindowHandle,
    _size: Option<(u32, u32)>,
    _appearance: WindowAppearance,
) -> Result<(), NativeWindowAppearanceError> {
    Err(NativeWindowAppearanceError::PlatformUnavailable(
        "macOS native appearance is only available on macOS",
    ))
}

#[cfg(target_os = "macos")]
fn sync_effect_view(host_view: &NSView, content_view: &NSView, appearance: WindowAppearance) {
    let key = host_view as *const NSView as usize;
    let wants_effect = matches!(
        appearance.backdrop,
        WindowBackdrop::Blurred(_) | WindowBackdrop::Material(_)
    );

    EFFECT_VIEWS.with(|views| {
        let mut views = views.borrow_mut();
        if !wants_effect {
            if let Some(effect) = views.remove(&key) {
                unsafe { effect.removeFromSuperview() };
            }
            return;
        }

        let effect = views.entry(key).or_insert_with(|| {
            let frame = content_view.bounds();
            let effect = unsafe {
                NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(), frame)
            };
            unsafe {
                effect.setAutoresizingMask(
                    NSAutoresizingMaskOptions::NSViewWidthSizable
                        | NSAutoresizingMaskOptions::NSViewHeightSizable,
                );
                effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
                effect.setState(NSVisualEffectState::Active);
                content_view.addSubview_positioned_relativeTo(
                    effect.as_ref(),
                    NSWindowOrderingMode::Below,
                    None,
                );
            }
            effect
        });

        unsafe {
            effect.setFrame(content_view.bounds());
            effect.setMaterial(material_for_appearance(appearance));
            effect.setState(NSVisualEffectState::Active);
        }
    });
}

#[cfg(target_os = "macos")]
fn material_for_appearance(appearance: WindowAppearance) -> NSVisualEffectMaterial {
    match appearance.backdrop {
        WindowBackdrop::Blurred(_) => NSVisualEffectMaterial::UnderWindowBackground,
        WindowBackdrop::Material(material) => match material.kind {
            WindowMaterialKind::Auto => NSVisualEffectMaterial::UnderWindowBackground,
            WindowMaterialKind::ThinTranslucent => NSVisualEffectMaterial::Sidebar,
            WindowMaterialKind::ThickTranslucent => NSVisualEffectMaterial::WindowBackground,
            WindowMaterialKind::NoiseTranslucent => NSVisualEffectMaterial::ContentBackground,
            WindowMaterialKind::TitlebarTranslucent => NSVisualEffectMaterial::Titlebar,
            WindowMaterialKind::Hud => NSVisualEffectMaterial::HUDWindow,
        },
        WindowBackdrop::Transparent(_) | WindowBackdrop::None => {
            NSVisualEffectMaterial::UnderWindowBackground
        }
    }
}
