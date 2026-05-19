// Tests extracted from crates/sturdy-engine/src/post_process.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn lens_config_defaults_to_disabled() {
    let config = LensConfig::default();

    assert!(!config.enabled());
    assert_eq!(config.dirt_strength, 0.0);
    assert!(!config.flare_enabled);
    assert_eq!(config.flare_strength, 0.25);
}

#[test]
fn lens_config_enables_for_dirt_or_flare() {
    assert!(
        LensConfig {
            dirt_strength: 0.1,
            ..Default::default()
        }
        .enabled()
    );
    assert!(
        LensConfig {
            flare_enabled: true,
            ..Default::default()
        }
        .enabled()
    );
}

#[test]
fn auto_exposure_documents_reserved_runtime_behavior() {
    let config = AutoExposureConfig {
        enabled: true,
        ..Default::default()
    };

    assert!(config.enabled);
}
