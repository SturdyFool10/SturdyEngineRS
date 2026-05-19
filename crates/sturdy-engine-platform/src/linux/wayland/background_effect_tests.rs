// Tests extracted from crates/sturdy-engine-platform/src/linux/wayland/background_effect.rs
// Runtime code should stay separate from test code.

use super::*;
use crate::{WindowAppearance, WindowAppearancePreset};

#[test]
fn protocol_selection_prefers_ext_background_effect() {
    let mut kde_called = false;
    let selected = choose_backdrop_protocol(
        || Ok((WaylandBackdropProtocol::ExtBackground, "ext")),
        || {
            kde_called = true;
            Ok((WaylandBackdropProtocol::KdeBlur, "kde"))
        },
    )
    .expect("ext path should be selected");

    assert_eq!(selected, (WaylandBackdropProtocol::ExtBackground, "ext"));
    assert!(!kde_called);
}

#[test]
fn protocol_selection_uses_kde_when_ext_is_unavailable() {
    let selected = choose_backdrop_protocol(
        || Err(NativeWindowAppearanceError::Degraded("missing ext".into())),
        || Ok((WaylandBackdropProtocol::KdeBlur, "kde")),
    )
    .expect("kde fallback should be selected");

    assert_eq!(selected, (WaylandBackdropProtocol::KdeBlur, "kde"));
}

#[test]
fn protocol_selection_uses_kde_when_ext_setup_is_refused() {
    let selected = choose_backdrop_protocol(
        || Err(NativeWindowAppearanceError::Degraded("ext refused".into())),
        || Ok((WaylandBackdropProtocol::KdeBlur, "kde")),
    )
    .expect("kde fallback should be selected after ext setup refusal");

    assert_eq!(selected, (WaylandBackdropProtocol::KdeBlur, "kde"));
}

#[test]
fn protocol_selection_reports_no_supported_protocol() {
    let err = choose_backdrop_protocol::<()>(
        || Err(NativeWindowAppearanceError::Degraded("missing ext".into())),
        || Err(NativeWindowAppearanceError::Degraded("missing kde".into())),
    )
    .expect_err("both missing protocols should degrade");

    assert!(err.is_degraded());
    let message = err.to_string();
    assert!(message.contains("missing ext"));
    assert!(message.contains("missing kde"));
}

#[test]
fn protocol_selection_does_not_fallback_after_hard_failure() {
    let mut kde_called = false;
    let err = choose_backdrop_protocol::<()>(
        || {
            Err(NativeWindowAppearanceError::ApplyFailed(
                "registry failed".into(),
            ))
        },
        || {
            kde_called = true;
            Ok((WaylandBackdropProtocol::KdeBlur, ()))
        },
    )
    .expect_err("hard ext failure should stop selection");

    assert_eq!(
        err,
        NativeWindowAppearanceError::ApplyFailed("registry failed".into())
    );
    assert!(!kde_called);
}

#[test]
fn no_blur_appearances_skip_native_blur_protocols() {
    assert!(!wants_native_blur(WindowAppearance::default()));
    assert!(!wants_native_blur(WindowAppearance::from_preset(
        WindowAppearancePreset::Transparent
    )));
    assert!(wants_native_blur(WindowAppearance::from_preset(
        WindowAppearancePreset::Blur
    )));
    assert!(wants_native_blur(WindowAppearance::from_preset(
        WindowAppearancePreset::ThinMaterial
    )));
}
