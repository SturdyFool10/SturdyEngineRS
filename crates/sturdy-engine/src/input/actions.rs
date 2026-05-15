use std::collections::{BTreeMap, HashMap, HashSet};

use super::{
    GamepadAxis, GamepadButton, GamepadId, KeyInput, KeyInputState, KeyToken, Keybind,
    KeybindCapture,
};

/// A small action binding registry with rebind support.
#[derive(Default)]
pub struct ActionBindingRegistry {
    bindings: BTreeMap<String, Keybind>,
    pending_rebind: Option<PendingRebind>,
}

struct PendingRebind {
    action: String,
    capture: KeybindCapture,
}

impl ActionBindingRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_binding(&mut self, action: impl Into<String>, binding: Keybind) {
        self.bindings.insert(action.into(), binding);
    }

    pub fn binding(&self, action: &str) -> Option<&Keybind> {
        self.bindings.get(action)
    }

    pub fn bindings(&self) -> &BTreeMap<String, Keybind> {
        &self.bindings
    }

    pub fn serialized_bindings(&self) -> BTreeMap<String, String> {
        self.bindings
            .iter()
            .map(|(action, binding)| (action.clone(), binding.to_string()))
            .collect()
    }

    pub fn request_rebind(&mut self, action: impl Into<String>) {
        self.pending_rebind = Some(PendingRebind {
            action: action.into(),
            capture: KeybindCapture::new(),
        });
    }

    pub fn pending_rebind_action(&self) -> Option<&str> {
        self.pending_rebind
            .as_ref()
            .map(|pending| pending.action.as_str())
    }

    pub fn handle_input(&mut self, input: &KeyInput) -> Option<BindingChange> {
        let pending = self.pending_rebind.as_mut()?;
        let binding = pending.capture.handle_input(input)?;
        let action = pending.action.clone();
        self.bindings.insert(action.clone(), binding.clone());
        self.pending_rebind = None;
        Some(BindingChange { action, binding })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindingChange {
    pub action: String,
    pub binding: Keybind,
}

/// Directional interpretation for an analog action binding.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ActionAxisDirection {
    /// Use positive axis values only.
    Positive,
    /// Use negative axis values as positive action values.
    Negative,
    /// Use the full signed axis value.
    Full,
}

/// A single input binding for an [`ActionMap`] action.
#[derive(Clone, Debug, PartialEq)]
pub enum ActionBinding {
    Key(Keybind),
    MouseButton(u8),
    GamepadButton {
        gamepad: Option<GamepadId>,
        button: GamepadButton,
    },
    GamepadAxis {
        gamepad: Option<GamepadId>,
        axis: GamepadAxis,
        direction: ActionAxisDirection,
        threshold: f32,
    },
}

impl From<Keybind> for ActionBinding {
    fn from(value: Keybind) -> Self {
        Self::Key(value)
    }
}

impl ActionBinding {
    pub fn mouse_button(button: u8) -> Self {
        Self::MouseButton(button)
    }

    pub fn gamepad_button(button: GamepadButton) -> Self {
        Self::GamepadButton {
            gamepad: None,
            button,
        }
    }

    pub fn gamepad_button_for(gamepad: GamepadId, button: GamepadButton) -> Self {
        Self::GamepadButton {
            gamepad: Some(gamepad),
            button,
        }
    }

    pub fn gamepad_axis(axis: GamepadAxis, direction: ActionAxisDirection) -> Self {
        Self::GamepadAxis {
            gamepad: None,
            axis,
            direction,
            threshold: 0.5,
        }
    }

    pub fn gamepad_axis_for(
        gamepad: GamepadId,
        axis: GamepadAxis,
        direction: ActionAxisDirection,
    ) -> Self {
        Self::GamepadAxis {
            gamepad: Some(gamepad),
            axis,
            direction,
            threshold: 0.5,
        }
    }

    pub fn with_threshold(mut self, threshold: f32) -> Self {
        if let Self::GamepadAxis { threshold: t, .. } = &mut self {
            *t = threshold.abs().clamp(0.0, 1.0);
        }
        self
    }
}

/// Frame-level action dispatcher.
///
/// Maps logical action names to one or more input bindings and tracks
/// per-frame digital state plus analog values.
///
/// Integrates with `clay_ui::UiEventResult`: pass `ui_result.key_consumed` to
/// [`process`](Self::process) so the UI layer always takes priority over game
/// actions.
#[derive(Default)]
pub struct ActionMap {
    bindings: HashMap<String, Vec<ActionBinding>>,
    held: HashSet<String>,
    just_pressed: HashSet<String>,
    just_released: HashSet<String>,
    analog_values: HashMap<String, f32>,
}

impl ActionMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a binding for an action. Multiple bindings per action are supported
    /// (e.g. both `Space` and `KeyW` for "Jump").
    pub fn bind(&mut self, action: impl Into<String>, binding: Keybind) {
        self.bind_input(action, binding);
    }

    /// Add any supported input binding for an action.
    pub fn bind_input(&mut self, action: impl Into<String>, binding: impl Into<ActionBinding>) {
        self.bindings
            .entry(action.into())
            .or_default()
            .push(binding.into());
    }

    /// Remove all bindings for an action.
    pub fn clear_bindings(&mut self, action: &str) {
        self.bindings.remove(action);
    }

    /// Replace all bindings from a plain-text config map.
    ///
    /// Each map value is a `;`-separated list of [`Keybind`] strings
    /// (e.g. `"Space;KeyW"` for two bindings on one action). Entries that fail
    /// to parse are silently skipped. Existing bindings are cleared first.
    pub fn load_config(&mut self, config: &BTreeMap<String, String>) {
        self.bindings.clear();
        for (action, value) in config {
            let bindings: Vec<ActionBinding> = value
                .split(';')
                .filter_map(|s| s.trim().parse().ok())
                .map(ActionBinding::Key)
                .collect();
            if !bindings.is_empty() {
                self.bindings.insert(action.clone(), bindings);
            }
        }
    }

    /// Export all bindings as a plain-text map suitable for persistence.
    ///
    /// Multiple bindings per action are joined with `;`.
    pub fn save_config(&self) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for (action, binds) in &self.bindings {
            let value = binds
                .iter()
                .filter_map(|b| match b {
                    ActionBinding::Key(keybind) => Some(keybind.to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(";");
            if !value.is_empty() {
                out.insert(action.clone(), value);
            }
        }
        out
    }

    /// Return all registered bindings for an action (empty slice if none).
    pub fn bindings_for(&self, action: &str) -> &[ActionBinding] {
        self.bindings.get(action).map_or(&[], Vec::as_slice)
    }

    /// Return keyboard bindings for an action.
    pub fn keybinds_for(&self, action: &str) -> Vec<&Keybind> {
        self.bindings_for(action)
            .iter()
            .filter_map(|binding| match binding {
                ActionBinding::Key(keybind) => Some(keybind),
                _ => None,
            })
            .collect()
    }

    /// Process one [`KeyInput`] event against all registered bindings.
    ///
    /// If `ui_consumed` is `true` the event is skipped: UI always takes
    /// priority. Pass `clay_ui::UiEventResult::key_consumed` here.
    ///
    /// Key-repeat events are also skipped; only initial press transitions
    /// drive `just_pressed`.
    ///
    /// Returns the names of actions whose state changed (useful for logging).
    pub fn process(&mut self, input: &KeyInput, ui_consumed: bool) -> Vec<String> {
        if ui_consumed || input.repeat {
            return Vec::new();
        }

        let mut changed = Vec::new();
        let actions: Vec<String> = self.bindings.keys().cloned().collect();
        for action in actions {
            let matches = self.bindings.get(&action).is_some_and(|binds| {
                binds.iter().any(|b| match b {
                    ActionBinding::Key(keybind) => keybind_matches(keybind, input),
                    _ => false,
                })
            });
            if !matches {
                continue;
            }
            match input.state {
                KeyInputState::Pressed => {
                    if self.held.insert(action.clone()) {
                        self.just_pressed.insert(action.clone());
                        changed.push(action);
                    }
                }
                KeyInputState::Released => {
                    if self.held.remove(&action) {
                        self.just_released.insert(action.clone());
                        changed.push(action);
                    }
                }
            }
        }
        changed
    }

    /// Clear `just_pressed` and `just_released` for the next frame.
    ///
    /// Call once per frame after reading all action states.
    pub fn end_frame(&mut self) {
        self.just_pressed.clear();
        self.just_released.clear();
        self.analog_values.clear();
    }

    pub(super) fn process_polling(
        &mut self,
        mouse_buttons: &HashSet<u8>,
        gamepad_buttons: &HashSet<(GamepadId, GamepadButton)>,
        gamepad_axes: &HashMap<(GamepadId, GamepadAxis), f32>,
    ) {
        let actions: Vec<(String, Vec<ActionBinding>)> = self
            .bindings
            .iter()
            .map(|(action, bindings)| (action.clone(), bindings.clone()))
            .collect();
        for (action, bindings) in actions {
            let mut digital_pressed = false;
            let mut analog_value = 0.0f32;
            let mut has_polling_binding = false;
            let mut has_analog_binding = false;
            for binding in bindings {
                match binding {
                    ActionBinding::MouseButton(button) => {
                        has_polling_binding = true;
                        if mouse_buttons.contains(&button) {
                            digital_pressed = true;
                        }
                    }
                    ActionBinding::GamepadButton { gamepad, button } => {
                        has_polling_binding = true;
                        if gamepad_buttons.iter().any(|(id, b)| {
                            *b == button && gamepad.is_none_or(|expected| expected == *id)
                        }) {
                            digital_pressed = true;
                        }
                    }
                    ActionBinding::GamepadAxis {
                        gamepad,
                        axis,
                        direction,
                        threshold,
                    } => {
                        has_polling_binding = true;
                        has_analog_binding = true;
                        for ((id, a), value) in gamepad_axes {
                            if *a != axis || gamepad.is_some_and(|expected| expected != *id) {
                                continue;
                            }
                            let mapped = match direction {
                                ActionAxisDirection::Positive => value.max(0.0),
                                ActionAxisDirection::Negative => (-value).max(0.0),
                                ActionAxisDirection::Full => *value,
                            };
                            if mapped.abs() > analog_value.abs() {
                                analog_value = mapped;
                            }
                            if mapped.abs() >= threshold {
                                digital_pressed = true;
                            }
                        }
                    }
                    ActionBinding::Key(_) => {}
                }
            }
            if has_polling_binding {
                self.sync_digital_action(&action, digital_pressed);
                if has_analog_binding {
                    self.set_analog_value(&action, analog_value);
                }
            }
        }
    }

    /// `true` while the action's key is held down.
    pub fn is_held(&self, action: &str) -> bool {
        self.held.contains(action)
    }

    /// `true` on the first frame the action's key was pressed.
    pub fn just_pressed(&self, action: &str) -> bool {
        self.just_pressed.contains(action)
    }

    /// `true` on the frame the action's key was released.
    pub fn just_released(&self, action: &str) -> bool {
        self.just_released.contains(action)
    }

    /// Analog value for an action this frame.
    ///
    /// Digital held actions return `1.0`; inactive actions return `0.0`.
    pub fn value(&self, action: &str) -> f32 {
        self.analog_values
            .get(action)
            .copied()
            .unwrap_or_else(|| self.is_held(action).then_some(1.0).unwrap_or(0.0))
    }

    fn sync_digital_action(&mut self, action: &str, pressed: bool) {
        if pressed {
            if self.held.insert(action.to_string()) {
                self.just_pressed.insert(action.to_string());
            }
        } else if self.held.remove(action) {
            self.just_released.insert(action.to_string());
        }
    }

    fn set_analog_value(&mut self, action: &str, value: f32) {
        let entry = self.analog_values.entry(action.to_string()).or_insert(0.0);
        if value.abs() > entry.abs() {
            *entry = value;
        }
    }
}

fn keybind_matches(binding: &Keybind, input: &KeyInput) -> bool {
    let key_matches = match (binding.key(), &input.key) {
        (Some(k), KeyToken::Key(ik)) => k == ik,
        _ => false,
    };
    key_matches
        && binding
            .modifiers()
            .iter()
            .all(|modifier| input.modifiers.contains(*modifier))
}
