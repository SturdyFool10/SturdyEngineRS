// Tests extracted from crates/sturdy-engine/src/input.rs
// Runtime code should stay separate from test code.

use super::*;
use std::collections::BTreeMap;

fn press(key: &str) -> KeyInput {
    KeyInput {
        key: KeyToken::Key(key.into()),
        state: KeyInputState::Pressed,
        modifiers: KeyModifiers::default(),
        repeat: false,
        text: None,
    }
}

fn release(key: &str) -> KeyInput {
    KeyInput {
        key: KeyToken::Key(key.into()),
        state: KeyInputState::Released,
        modifiers: KeyModifiers::default(),
        repeat: false,
        text: None,
    }
}

fn press_with_ctrl(key: &str) -> KeyInput {
    KeyInput {
        key: KeyToken::Key(key.into()),
        state: KeyInputState::Pressed,
        modifiers: KeyModifiers {
            ctrl: true,
            ..KeyModifiers::default()
        },
        repeat: false,
        text: None,
    }
}

#[test]
fn just_pressed_is_true_on_first_press_then_clears_after_end_frame() {
    let mut map = ActionMap::new();
    map.bind("Jump", "Space".parse().unwrap());

    map.process(&press("Space"), false);
    assert!(map.just_pressed("Jump"));
    assert!(map.is_held("Jump"));

    map.end_frame();
    assert!(!map.just_pressed("Jump"));
    assert!(map.is_held("Jump"));
}

#[test]
fn just_released_is_true_on_key_up_then_clears_after_end_frame() {
    let mut map = ActionMap::new();
    map.bind("Jump", "Space".parse().unwrap());

    map.process(&press("Space"), false);
    map.end_frame();
    map.process(&release("Space"), false);

    assert!(map.just_released("Jump"));
    assert!(!map.is_held("Jump"));

    map.end_frame();
    assert!(!map.just_released("Jump"));
}

#[test]
fn ui_consumed_suppresses_action_dispatch() {
    let mut map = ActionMap::new();
    map.bind("Confirm", "Enter".parse().unwrap());

    map.process(&press("Enter"), true); // ui_consumed = true
    assert!(!map.just_pressed("Confirm"));
    assert!(!map.is_held("Confirm"));
}

#[test]
fn repeat_events_do_not_re_trigger_just_pressed() {
    let mut map = ActionMap::new();
    map.bind("Fire", "Space".parse().unwrap());

    map.process(&press("Space"), false);
    map.end_frame();

    let repeat_event = KeyInput {
        key: KeyToken::Key("Space".into()),
        state: KeyInputState::Pressed,
        modifiers: KeyModifiers::default(),
        repeat: true,
        text: None,
    };
    map.process(&repeat_event, false);
    assert!(!map.just_pressed("Fire"));
    assert!(map.is_held("Fire"));
}

#[test]
fn multi_binding_either_key_triggers_action() {
    let mut map = ActionMap::new();
    map.bind("MoveForward", "KeyW".parse().unwrap());
    map.bind("MoveForward", "ArrowUp".parse().unwrap());

    map.process(&press("ArrowUp"), false);
    assert!(map.just_pressed("MoveForward"));
    map.end_frame();
    map.process(&release("ArrowUp"), false);
    map.end_frame();

    map.process(&press("KeyW"), false);
    assert!(map.just_pressed("MoveForward"));
}

#[test]
fn modifier_binding_requires_modifier_held() {
    let mut map = ActionMap::new();
    map.bind("Undo", "Ctrl+KeyZ".parse().unwrap());

    // Press without Ctrl — should not fire.
    map.process(&press("KeyZ"), false);
    assert!(!map.just_pressed("Undo"));

    // Press with Ctrl — should fire.
    map.process(&press_with_ctrl("KeyZ"), false);
    assert!(map.just_pressed("Undo"));
}

#[test]
fn lenient_matching_allows_extra_modifiers() {
    let mut map = ActionMap::new();
    // Binding has no modifiers.
    map.bind("Jump", "Space".parse().unwrap());

    // Press Space while Shift is held — should still fire.
    let event = KeyInput {
        key: KeyToken::Key("Space".into()),
        state: KeyInputState::Pressed,
        modifiers: KeyModifiers {
            shift: true,
            ..KeyModifiers::default()
        },
        repeat: false,
        text: None,
    };
    map.process(&event, false);
    assert!(map.just_pressed("Jump"));
}

#[test]
fn save_and_load_config_round_trips() {
    let mut map = ActionMap::new();
    map.bind("Jump", "Space".parse().unwrap());
    map.bind("Jump", "KeyW".parse().unwrap());
    map.bind("Fire", "Ctrl+Space".parse().unwrap());

    let config = map.save_config();
    let mut map2 = ActionMap::new();
    map2.load_config(&config);

    // Both Jump bindings survive the round-trip.
    assert_eq!(map2.bindings_for("Jump").len(), 2);
    map2.process(&press("Space"), false);
    assert!(map2.just_pressed("Jump"));
    map2.end_frame();
    map2.process(&release("Space"), false); // release first before pressing second
    map2.end_frame();
    map2.process(&press("KeyW"), false);
    assert!(map2.just_pressed("Jump"));
}

#[test]
fn clear_bindings_removes_action() {
    let mut map = ActionMap::new();
    map.bind("Jump", "Space".parse().unwrap());
    map.clear_bindings("Jump");

    map.process(&press("Space"), false);
    assert!(!map.just_pressed("Jump"));
}

#[test]
fn load_config_replaces_existing_bindings() {
    let mut map = ActionMap::new();
    map.bind("Jump", "Space".parse().unwrap());

    let mut cfg = BTreeMap::new();
    cfg.insert("Jump".to_string(), "KeyW".to_string());
    map.load_config(&cfg);

    // Old "Space" binding is gone.
    map.process(&press("Space"), false);
    assert!(!map.just_pressed("Jump"));

    // New "KeyW" binding works.
    map.process(&press("KeyW"), false);
    assert!(map.just_pressed("Jump"));
}

#[test]
fn action_map_config_saves_only_keybinds() {
    let mut map = ActionMap::new();
    map.bind("Jump", "Space".parse().unwrap());
    map.bind_input("Jump", ActionBinding::mouse_button(0));

    let config = map.save_config();

    assert_eq!(config.get("Jump").map(String::as_str), Some("Space"));
    assert_eq!(map.bindings_for("Jump").len(), 2);
    assert_eq!(map.keybinds_for("Jump").len(), 1);
}

#[test]
fn input_hub_routes_mouse_button_to_action_map() {
    let mut hub = InputHub::new();
    hub.actions_mut()
        .bind_input("Fire", ActionBinding::mouse_button(0));

    hub.on_pointer_button(clay_ui::WindowLogicalPx::ZERO, 0, true);
    hub.update(&clay_ui::LayoutTree::default());
    assert!(hub.actions().is_held("Fire"));
    assert!(hub.actions().just_pressed("Fire"));
    assert_eq!(hub.actions().value("Fire"), 1.0);

    hub.update(&clay_ui::LayoutTree::default());
    assert!(hub.actions().is_held("Fire"));
    assert!(!hub.actions().just_pressed("Fire"));

    hub.on_pointer_button(clay_ui::WindowLogicalPx::ZERO, 0, false);
    hub.update(&clay_ui::LayoutTree::default());
    assert!(!hub.actions().is_held("Fire"));
    assert!(hub.actions().just_released("Fire"));
    assert_eq!(hub.actions().value("Fire"), 0.0);
}

#[test]
fn input_hub_routes_gamepad_button_to_action_map() {
    let mut hub = InputHub::new();
    let gamepad = GamepadId(7);
    hub.actions_mut().bind_input(
        "Jump",
        ActionBinding::gamepad_button_for(gamepad, GamepadButton::South),
    );

    hub.on_gamepad_button(gamepad, GamepadButton::South, true);
    hub.update(&clay_ui::LayoutTree::default());
    assert!(hub.actions().is_held("Jump"));
    assert!(hub.actions().just_pressed("Jump"));

    hub.on_gamepad_button(gamepad, GamepadButton::South, false);
    hub.update(&clay_ui::LayoutTree::default());
    assert!(!hub.actions().is_held("Jump"));
    assert!(hub.actions().just_released("Jump"));
}

#[test]
fn input_hub_routes_gamepad_axis_to_analog_action_value() {
    let mut hub = InputHub::new();
    let gamepad = GamepadId(4);
    hub.actions_mut().bind_input(
        "MoveRight",
        ActionBinding::gamepad_axis_for(
            gamepad,
            GamepadAxis::LeftStickX,
            ActionAxisDirection::Positive,
        )
        .with_threshold(0.25),
    );
    hub.actions_mut().bind_input(
        "MoveLeft",
        ActionBinding::gamepad_axis_for(
            gamepad,
            GamepadAxis::LeftStickX,
            ActionAxisDirection::Negative,
        )
        .with_threshold(0.25),
    );

    hub.on_gamepad_axis(gamepad, GamepadAxis::LeftStickX, 0.6);
    hub.update(&clay_ui::LayoutTree::default());
    assert_eq!(hub.actions().value("MoveRight"), 0.6);
    assert_eq!(hub.actions().value("MoveLeft"), 0.0);
    assert!(hub.actions().is_held("MoveRight"));
    assert!(!hub.actions().is_held("MoveLeft"));

    hub.on_gamepad_axis(gamepad, GamepadAxis::LeftStickX, -0.75);
    hub.update(&clay_ui::LayoutTree::default());
    assert_eq!(hub.actions().value("MoveRight"), 0.0);
    assert_eq!(hub.actions().value("MoveLeft"), 0.75);
    assert!(!hub.actions().is_held("MoveRight"));
    assert!(hub.actions().is_held("MoveLeft"));
}

#[test]
fn input_hub_publishes_raw_key_polling_state_per_update() {
    let mut hub = InputHub::new();
    hub.on_key_input(&press("KeyW"));
    assert!(hub.is_key_pressed("KeyW"));
    assert!(!hub.is_key_just_pressed("KeyW"));

    hub.update(&clay_ui::LayoutTree::default());
    assert!(hub.is_key_pressed("KeyW"));
    assert!(hub.is_key_just_pressed("KeyW"));
    assert!(!hub.is_key_just_released("KeyW"));

    hub.update(&clay_ui::LayoutTree::default());
    assert!(hub.is_key_pressed("KeyW"));
    assert!(!hub.is_key_just_pressed("KeyW"));

    hub.on_key_input(&release("KeyW"));
    hub.update(&clay_ui::LayoutTree::default());
    assert!(!hub.is_key_pressed("KeyW"));
    assert!(hub.is_key_just_released("KeyW"));
}

#[test]
fn input_hub_publishes_raw_mouse_polling_state_per_update() {
    let mut hub = InputHub::new();
    hub.on_pointer_moved(clay_ui::WindowLogicalPx::new(10.0, 20.0));
    hub.on_pointer_moved(clay_ui::WindowLogicalPx::new(13.0, 25.0));
    hub.on_pointer_button(clay_ui::WindowLogicalPx::new(13.0, 25.0), 0, true);

    hub.update(&clay_ui::LayoutTree::default());
    assert_eq!(
        hub.mouse_position(),
        clay_ui::WindowLogicalPx::new(13.0, 25.0)
    );
    assert_eq!(hub.mouse_delta(), glam::Vec2::new(3.0, 5.0));
    assert!(hub.is_mouse_button_pressed(0));
    assert!(hub.is_mouse_button_just_pressed(0));

    hub.update(&clay_ui::LayoutTree::default());
    assert_eq!(hub.mouse_delta(), glam::Vec2::ZERO);
    assert!(hub.is_mouse_button_pressed(0));
    assert!(!hub.is_mouse_button_just_pressed(0));

    hub.on_pointer_button(clay_ui::WindowLogicalPx::new(13.0, 25.0), 0, false);
    hub.update(&clay_ui::LayoutTree::default());
    assert!(!hub.is_mouse_button_pressed(0));
    assert!(hub.is_mouse_button_just_released(0));
}

#[test]
fn input_hub_reports_optional_gaze_direction() {
    let mut hub = InputHub::new();
    assert_eq!(hub.gaze_direction(), None);

    hub.on_gaze_direction(Some(glam::Vec2::new(0.25, -0.5)));
    assert_eq!(hub.gaze_direction(), Some(glam::Vec2::new(0.25, -0.5)));

    hub.update(&clay_ui::LayoutTree::default());
    assert_eq!(hub.gaze_direction(), Some(glam::Vec2::new(0.25, -0.5)));

    hub.on_gaze_direction(None);
    assert_eq!(hub.gaze_direction(), None);
}

#[test]
fn input_hub_ignores_non_finite_gaze_direction() {
    let mut hub = InputHub::new();
    hub.on_gaze_direction(Some(glam::Vec2::new(0.0, 0.0)));
    hub.on_gaze_direction(Some(glam::Vec2::new(f32::NAN, 0.25)));

    assert_eq!(hub.gaze_direction(), Some(glam::Vec2::new(0.0, 0.0)));
}

#[test]
fn input_hub_publishes_gamepad_button_state_per_update() {
    let mut hub = InputHub::new();
    let gamepad = GamepadId(2);

    hub.on_gamepad_button(gamepad, GamepadButton::South, true);
    assert!(hub.is_gamepad_button_pressed(gamepad, GamepadButton::South));
    assert!(!hub.is_gamepad_button_just_pressed(gamepad, GamepadButton::South));

    hub.update(&clay_ui::LayoutTree::default());
    assert!(hub.is_gamepad_button_pressed(gamepad, GamepadButton::South));
    assert!(hub.is_gamepad_button_just_pressed(gamepad, GamepadButton::South));
    assert!(!hub.is_gamepad_button_just_released(gamepad, GamepadButton::South));

    hub.update(&clay_ui::LayoutTree::default());
    assert!(hub.is_gamepad_button_pressed(gamepad, GamepadButton::South));
    assert!(!hub.is_gamepad_button_just_pressed(gamepad, GamepadButton::South));

    hub.on_gamepad_button(gamepad, GamepadButton::South, false);
    hub.update(&clay_ui::LayoutTree::default());
    assert!(!hub.is_gamepad_button_pressed(gamepad, GamepadButton::South));
    assert!(hub.is_gamepad_button_just_released(gamepad, GamepadButton::South));
}

#[test]
fn input_hub_tracks_gamepad_axis_values() {
    let mut hub = InputHub::new();
    let gamepad = GamepadId(1);

    assert_eq!(hub.gamepad_axis(gamepad, GamepadAxis::LeftStickX), 0.0);
    hub.on_gamepad_axis(gamepad, GamepadAxis::LeftStickX, 0.5);
    hub.on_gamepad_axis(gamepad, GamepadAxis::RightTrigger, 3.0);

    assert_eq!(hub.gamepad_axis(gamepad, GamepadAxis::LeftStickX), 0.5);
    assert_eq!(hub.gamepad_axis(gamepad, GamepadAxis::RightTrigger), 1.0);
}

#[test]
fn input_hub_clears_disconnected_gamepad_state() {
    let mut hub = InputHub::new();
    let gamepad = GamepadId(3);

    hub.on_gamepad_button(gamepad, GamepadButton::East, true);
    hub.on_gamepad_axis(gamepad, GamepadAxis::LeftStickY, -0.75);
    hub.update(&clay_ui::LayoutTree::default());

    assert!(hub.is_gamepad_button_pressed(gamepad, GamepadButton::East));
    assert_eq!(hub.gamepad_axis(gamepad, GamepadAxis::LeftStickY), -0.75);

    hub.clear_gamepad(gamepad);
    assert!(!hub.is_gamepad_button_pressed(gamepad, GamepadButton::East));
    assert!(!hub.is_gamepad_button_just_pressed(gamepad, GamepadButton::East));
    assert_eq!(hub.gamepad_axis(gamepad, GamepadAxis::LeftStickY), 0.0);
}
