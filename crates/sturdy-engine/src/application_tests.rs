// Tests extracted from crates/sturdy-engine/src/application.rs
// Runtime code should stay separate from test code.

use crate::{RuntimePassTiming, RuntimeSettingValue, SurfacePresentMode};

use super::*;

#[test]
fn window_desc_captures_window_config() {
    let config = WindowConfig::new("primary", 1280, 720)
        .with_position(10, 20)
        .with_resizable(true)
        .with_decorations(false)
        .with_maximized(true)
        .with_always_on_top(true)
        .with_borderless_fullscreen(true)
        .with_hdr(true)
        .with_window_appearance_preset(WindowAppearancePreset::Blur);

    let desc = WindowDesc::from_config(&config);

    assert_eq!(desc.title, "primary");
    assert_eq!(desc.width, 1280);
    assert_eq!(desc.height, 720);
    assert_eq!(desc.position, Some((10, 20)));
    assert!(desc.resizable);
    assert!(!desc.decorations);
    assert!(desc.maximized);
    assert!(desc.always_on_top);
    assert_eq!(desc.window_mode, WindowMode::BorderlessFullscreen);
    assert!(desc.prefer_hdr);
    assert_eq!(
        desc.appearance,
        WindowAppearance::from_preset(WindowAppearancePreset::Blur)
    );
}

#[test]
fn shell_event_loop_command_queue_preserves_create_window_order() {
    let first = WindowDesc::from_config(&WindowConfig::new("first", 100, 100));
    let second = WindowDesc::from_config(&WindowConfig::new("second", 200, 200));
    let mut queue = ShellEventLoopCommandQueue::new();

    queue.create_window(first.clone());
    queue.create_window(second.clone());

    assert_eq!(queue.len(), 2);
    assert_eq!(
        queue.pop_front(),
        Some(ShellEventLoopCommand::CreateWindow(first))
    );
    assert_eq!(
        queue.pop_front(),
        Some(ShellEventLoopCommand::CreateWindow(second))
    );
    assert_eq!(queue.pop_front(), None);
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn close_action_exits_for_primary_window() {
    let mut windows = WindowRegistry::new();
    let primary = windows.insert("primary");

    assert_eq!(
        close_action_for_window(primary, windows.contains(primary), primary),
        ShellWindowCloseAction::ExitApplication
    );
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn close_action_removes_non_primary_window() {
    let mut windows = WindowRegistry::new();
    let primary = windows.insert("primary");
    let secondary = windows.insert("secondary");

    assert_eq!(
        close_action_for_window(primary, windows.contains(secondary), secondary),
        ShellWindowCloseAction::RemoveWindow(secondary)
    );
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn close_action_ignores_unknown_window() {
    let mut windows = WindowRegistry::new();
    let primary = windows.insert("primary");
    let stale = windows.insert("stale");
    assert_eq!(windows.remove(stale), Some("stale"));

    assert_eq!(
        close_action_for_window(primary, windows.contains(stale), stale),
        ShellWindowCloseAction::IgnoreUnknown
    );
}

#[test]
fn present_policy_maps_to_preferred_present_mode() {
    assert_eq!(parse_present_policy_setting("Auto", None), None);
    assert_eq!(
        parse_present_policy_setting("NoTear", None),
        Some(SurfacePresentMode::Fifo)
    );
    assert_eq!(
        parse_present_policy_setting("LowLatencyNoTear", None),
        Some(SurfacePresentMode::Mailbox)
    );
    assert_eq!(
        parse_present_policy_setting("LowLatencyAllowTear", None),
        Some(SurfacePresentMode::RelaxedFifo)
    );
    assert_eq!(
        parse_present_policy_setting("Explicit", Some(SurfacePresentMode::RelaxedFifo)),
        Some(SurfacePresentMode::RelaxedFifo)
    );
}

#[test]
fn window_reconfigure_report_marks_basic_window_changes_applied() {
    let changes = vec![RuntimeSettingChange {
        setting: RuntimeSettingId::from(RuntimeSettingKey::WindowTitle),
        value: RuntimeSettingValue::Text("new title".to_string()),
        path: RuntimeApplyPath::WindowReconfigure,
        revision: 1,
    }];

    let report = window_reconfigure_apply_report(&changes, None);

    assert_eq!(
        report.changes,
        vec![RuntimeChangeResult::Applied {
            setting: RuntimeSettingId::from(RuntimeSettingKey::WindowTitle),
            path: RuntimeApplyPath::WindowReconfigure,
        }]
    );
}

#[test]
fn window_reconfigure_report_surfaces_native_appearance_degradation() {
    let changes = vec![
        RuntimeSettingChange {
            setting: RuntimeSettingId::from(RuntimeSettingKey::WindowBackgroundEffect),
            value: RuntimeSettingValue::Text("Blur".to_string()),
            path: RuntimeApplyPath::WindowReconfigure,
            revision: 1,
        },
        RuntimeSettingChange {
            setting: RuntimeSettingId::from(RuntimeSettingKey::SurfaceTransparency),
            value: RuntimeSettingValue::Bool(true),
            path: RuntimeApplyPath::SurfaceRecreate,
            revision: 2,
        },
    ];
    let native_report = NativeWindowAppearanceApplyReport {
        requested: "blur",
        protocol: "test-protocol",
        status: NativeWindowAppearanceStatus::Degraded,
        fallback: Some("winit"),
        reason: Some("blur protocol unavailable".to_string()),
    };

    let report = window_reconfigure_apply_report(&changes, Some(&native_report));

    assert_eq!(report.changes.len(), 1);
    assert!(matches!(
        &report.changes[0],
        RuntimeChangeResult::Degraded {
            setting,
            path: RuntimeApplyPath::WindowReconfigure,
            reason,
        } if setting == &RuntimeSettingId::from(RuntimeSettingKey::WindowBackgroundEffect)
            && reason.contains("status=degraded")
            && reason.contains("blur protocol unavailable")
    ));
}

fn pt(name: &str, ms: f32) -> RuntimePassTiming {
    RuntimePassTiming { name: name.to_string(), gpu_time_ms: Some(ms) }
}

#[test]
fn pass_timing_overlay_empty_when_no_timings() {
    let lines = pass_timing_overlay_lines(&[]);
    assert!(lines.is_empty());
}

#[test]
fn pass_timing_overlay_groups_bloom_passes() {
    let timings = vec![
        pt("Bloom: bright", 0.05),
        pt("Bloom: down/0", 0.10),
        pt("Bloom: down/1", 0.08),
        pt("Bloom: up/1", 0.09),
        pt("Bloom: up/0", 0.11),
        pt("Bloom: composite", 0.07),
        pt("Deferred", 2.10),
    ];
    let lines = pass_timing_overlay_lines(&timings);
    // Header line present
    assert!(lines[0].starts_with("passes: total="));
    // Bloom is grouped into one line
    let bloom_line = lines.iter().find(|l| l.contains("Bloom:")).unwrap();
    assert!(bloom_line.contains("6 ops"), "expected 6 ops, got: {bloom_line}");
    // Deferred is individual
    assert!(lines.iter().any(|l| l.contains("Deferred:")));
    // No individual Bloom down/up lines
    assert!(!lines.iter().any(|l| l.contains("down/")));
}

#[test]
fn pass_timing_overlay_singles_listed_individually() {
    let timings = vec![
        pt("Shadow CSM 0", 0.15),
        pt("Shadow CSM 1", 0.12),
        pt("Deferred", 1.80),
    ];
    let lines = pass_timing_overlay_lines(&timings);
    assert!(lines.iter().any(|l| l.contains("Shadow CSM 0:")));
    assert!(lines.iter().any(|l| l.contains("Shadow CSM 1:")));
    assert!(lines.iter().any(|l| l.contains("Deferred:")));
}

#[test]
fn pass_timing_overlay_skips_zero_ms_passes() {
    let timings = vec![
        pt("Bloom: bright", 0.0),
        pt("Deferred", 1.80),
    ];
    let lines = pass_timing_overlay_lines(&timings);
    assert!(!lines.iter().any(|l| l.contains("Bloom")));
    assert!(lines.iter().any(|l| l.contains("Deferred:")));
}
