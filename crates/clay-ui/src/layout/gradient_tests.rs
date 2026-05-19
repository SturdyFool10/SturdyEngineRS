// Tests extracted from crates/clay-ui/src/layout/gradient.rs
// Runtime code should stay separate from test code.

use super::*;

struct HoldThenJump;

impl EasingFunction for HoldThenJump {
    fn ease(&self, t: f32) -> f32 {
        if t < 1.0 { 0.0 } else { 1.0 }
    }
}

#[test]
fn preset_easing_implements_easing_function_trait() {
    assert!((Easing::CubicInOut.ease(0.5) - 0.5).abs() < 1e-6);
}

#[test]
fn registry_accepts_custom_easing_function_trait_objects() {
    let mut registry = EasingRegistry::default();
    registry.register_function(7, HoldThenJump);

    assert_eq!(registry.evaluate(Easing::Custom(7), 0.5), 0.0);
    assert_eq!(registry.evaluate(Easing::Custom(7), 1.0), 1.0);
}

#[test]
fn registry_still_accepts_closure_easing_functions() {
    let mut registry = EasingRegistry::default();
    registry.register(9, |t| 1.0 - t);

    assert_eq!(registry.evaluate(Easing::Custom(9), 0.25), 0.75);
}
