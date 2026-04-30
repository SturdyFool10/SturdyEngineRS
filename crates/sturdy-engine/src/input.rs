use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt,
    str::FromStr,
};

/// Modifier keys recognized by the runtime input layer.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum KeyModifier {
    Ctrl,
    Alt,
    Shift,
    Super,
}

impl KeyModifier {
    pub fn token(self) -> &'static str {
        match self {
            Self::Ctrl => "Ctrl",
            Self::Alt => "Alt",
            Self::Shift => "Shift",
            Self::Super => "Super",
        }
    }
}

impl fmt::Display for KeyModifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.token())
    }
}

impl FromStr for KeyModifier {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "Ctrl" | "Control" => Ok(Self::Ctrl),
            "Alt" => Ok(Self::Alt),
            "Shift" => Ok(Self::Shift),
            "Super" | "Meta" | "Cmd" | "Command" => Ok(Self::Super),
            _ => Err("unknown modifier"),
        }
    }
}

/// A physical key token used for bindings and matching.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum KeyToken {
    Modifier(KeyModifier),
    Key(String),
}

impl From<&str> for KeyToken {
    fn from(value: &str) -> Self {
        Self::Key(value.to_string())
    }
}

impl From<String> for KeyToken {
    fn from(value: String) -> Self {
        Self::Key(value)
    }
}

impl KeyToken {
    pub fn key(name: impl Into<String>) -> Self {
        Self::Key(name.into())
    }

    pub fn display_label(&self) -> String {
        match self {
            Self::Modifier(modifier) => modifier.to_string(),
            Self::Key(name) => display_key_name(name),
        }
    }

    pub fn serialization_token(&self) -> String {
        match self {
            Self::Modifier(modifier) => modifier.to_string(),
            Self::Key(name) => name.clone(),
        }
    }
}

impl fmt::Display for KeyToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.serialization_token())
    }
}

/// A serializable keybind.
///
/// Modifier-only bindings are represented with `key == None`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Keybind {
    modifiers: Vec<KeyModifier>,
    key: Option<String>,
}

impl Keybind {
    pub fn new(modifiers: impl IntoIterator<Item = KeyModifier>, key: Option<String>) -> Self {
        let mut modifiers: Vec<_> = modifiers.into_iter().collect();
        modifiers.sort();
        modifiers.dedup();
        Self { modifiers, key }
    }

    pub fn modifiers(&self) -> &[KeyModifier] {
        &self.modifiers
    }

    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    pub fn is_modifier_only(&self) -> bool {
        self.key.is_none()
    }

    pub fn display_label(&self) -> String {
        let mut parts: Vec<String> = self.modifiers.iter().map(ToString::to_string).collect();
        if let Some(key) = &self.key {
            parts.push(display_key_name(key));
        }
        parts.join("+")
    }
}

impl fmt::Display for Keybind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<String> = self.modifiers.iter().map(ToString::to_string).collect();
        if let Some(key) = &self.key {
            parts.push(key.clone());
        }
        f.write_str(&parts.join("+"))
    }
}

impl FromStr for Keybind {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        if value.is_empty() {
            return Err("empty keybind");
        }

        let mut modifiers = Vec::new();
        let mut key = None;
        for part in value.split('+') {
            let part = part.trim();
            if part.is_empty() {
                return Err("empty keybind token");
            }
            if let Ok(modifier) = KeyModifier::from_str(part) {
                modifiers.push(modifier);
                continue;
            }
            if key.is_some() {
                return Err("multiple non-modifier keys are not supported");
            }
            key = Some(part.to_string());
        }

        if modifiers.is_empty() && key.is_none() {
            return Err("empty keybind");
        }

        Ok(Self::new(modifiers, key))
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum KeyInputState {
    Pressed,
    Released,
}

/// Snapshot of currently held modifier keys.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct KeyModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_: bool,
}

impl KeyModifiers {
    pub fn contains(self, modifier: KeyModifier) -> bool {
        match modifier {
            KeyModifier::Ctrl => self.ctrl,
            KeyModifier::Alt => self.alt,
            KeyModifier::Shift => self.shift,
            KeyModifier::Super => self.super_,
        }
    }

    pub fn iter(self) -> impl Iterator<Item = KeyModifier> {
        [
            (self.ctrl, KeyModifier::Ctrl),
            (self.alt, KeyModifier::Alt),
            (self.shift, KeyModifier::Shift),
            (self.super_, KeyModifier::Super),
        ]
        .into_iter()
        .filter_map(|(present, modifier)| present.then_some(modifier))
    }
}

/// A runtime key input event suitable for action dispatch and rebinding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyInput {
    pub key: KeyToken,
    pub state: KeyInputState,
    pub modifiers: KeyModifiers,
    pub repeat: bool,
    pub text: Option<String>,
}

/// Stable runtime gamepad identifier.
///
/// Backend adapters map their native gamepad/device id to this compact value.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct GamepadId(pub u32);

/// Gamepad buttons recognized by the runtime input layer.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum GamepadButton {
    South,
    East,
    West,
    North,
    LeftBumper,
    RightBumper,
    LeftTrigger,
    RightTrigger,
    Select,
    Start,
    Guide,
    LeftStick,
    RightStick,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
    Other(u16),
}

/// Gamepad analog axes recognized by the runtime input layer.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum GamepadAxis {
    LeftStickX,
    LeftStickY,
    RightStickX,
    RightStickY,
    LeftTrigger,
    RightTrigger,
    Other(u16),
}

/// A runtime gamepad button event suitable for polling and action dispatch.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct GamepadButtonInput {
    pub gamepad: GamepadId,
    pub button: GamepadButton,
    pub state: KeyInputState,
}

/// A runtime gamepad axis event suitable for polling and action dispatch.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GamepadAxisInput {
    pub gamepad: GamepadId,
    pub axis: GamepadAxis,
    pub value: f32,
}

/// Captures a keybind from a sequence of key events.
pub struct KeybindCapture {
    held_modifiers: BTreeSet<KeyModifier>,
    encountered_modifiers: BTreeSet<KeyModifier>,
    held_non_modifiers: BTreeSet<String>,
    completed: Option<Keybind>,
}

impl Default for KeybindCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl KeybindCapture {
    pub fn new() -> Self {
        Self {
            held_modifiers: BTreeSet::new(),
            encountered_modifiers: BTreeSet::new(),
            held_non_modifiers: BTreeSet::new(),
            completed: None,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn is_complete(&self) -> bool {
        self.completed.is_some()
    }

    pub fn handle_input(&mut self, input: &KeyInput) -> Option<Keybind> {
        if self.completed.is_some() {
            return self.completed.clone();
        }

        match (&input.key, input.state) {
            (KeyToken::Modifier(modifier), KeyInputState::Pressed) => {
                self.held_modifiers.insert(*modifier);
                self.encountered_modifiers.insert(*modifier);
                None
            }
            (KeyToken::Modifier(modifier), KeyInputState::Released) => {
                self.held_modifiers.remove(modifier);
                if self.held_modifiers.is_empty()
                    && self.held_non_modifiers.is_empty()
                    && !self.encountered_modifiers.is_empty()
                {
                    let binding = Keybind::new(self.encountered_modifiers.iter().copied(), None);
                    self.completed = Some(binding.clone());
                    Some(binding)
                } else {
                    None
                }
            }
            (KeyToken::Key(key), KeyInputState::Pressed) => {
                self.held_non_modifiers.insert(key.clone());
                let binding = Keybind::new(self.held_modifiers.iter().copied(), Some(key.clone()));
                self.completed = Some(binding.clone());
                Some(binding)
            }
            (KeyToken::Key(key), KeyInputState::Released) => {
                self.held_non_modifiers.remove(key);
                None
            }
        }
    }
}

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
///
/// # Example
/// ```ignore
/// let mut actions = ActionMap::new();
/// actions.bind("Jump", "Space".parse().unwrap());
/// actions.bind("Jump", "KeyW".parse().unwrap()); // second binding
///
/// // Each frame — after InputSimulator::update()
/// for event in &raw_key_events {
///     actions.process(event, ui_result.key_consumed);
/// }
/// if actions.just_pressed("Jump") { player.jump(); }
/// actions.end_frame(); // clears just_pressed / just_released
/// ```
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
    /// If `ui_consumed` is `true` the event is skipped — UI always takes
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
            let matches = self
                .bindings
                .get(&action)
                .is_some_and(|binds| {
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

    fn process_polling(
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
}

fn clay_modifiers(modifiers: KeyModifiers) -> clay_ui::ModifierKeys {
    clay_ui::ModifierKeys {
        ctrl: modifiers.ctrl,
        alt: modifiers.alt,
        shift: modifiers.shift,
        super_: modifiers.super_,
    }
}

// ── InputHub ──────────────────────────────────────────────────────────────────

/// Drop-in input coordinator that bridges raw `EngineApp` callbacks to both
/// the UI [`clay_ui::InputSimulator`] and the game-action [`ActionMap`].
///
/// # Usage
///
/// Add `hub: InputHub` to your app struct, implement `EngineApp::input_hub`
/// to return `Some(&mut self.hub)`, and the engine shell routes all keyboard
/// and pointer events automatically. In `render`, call `hub.update(&layout)`
/// once after building the layout tree, then query widget and action states.
///
/// ```ignore
/// struct MyApp {
///     hub: InputHub,
/// }
///
/// impl EngineApp for MyApp {
///     fn input_hub(&mut self) -> Option<&mut InputHub> { Some(&mut self.hub) }
///
///     fn render(&mut self, frame: &mut ShellFrame, image: &SurfaceImage) -> Result<()> {
///         let layout = build_layout(...);
///         self.hub.update(&layout);
///         if self.hub.actions().just_pressed("Jump") { /* ... */ }
///         if self.hub.widget_state(&button_id).activated  { /* ... */ }
///         Ok(())
///     }
/// }
/// ```
///
/// For tests or replay, queue events directly:
/// ```ignore
/// hub.queue(InputEvent::Pointer(...));      // UI events
/// hub.simulate_key(&key_input);            // key → both UI and ActionMap
/// hub.update(&layout);
/// ```
pub struct InputHub {
    simulator: clay_ui::InputSimulator,
    actions: ActionMap,
    cursor: clay_ui::WindowLogicalPx,
    cursor_initialized: bool,
    mouse_delta: glam::Vec2,
    pending_mouse_delta: glam::Vec2,
    held_keys: HashSet<KeyToken>,
    key_just_pressed: HashSet<KeyToken>,
    key_just_released: HashSet<KeyToken>,
    pending_key_pressed: HashSet<KeyToken>,
    pending_key_released: HashSet<KeyToken>,
    held_mouse_buttons: HashSet<u8>,
    mouse_button_just_pressed: HashSet<u8>,
    mouse_button_just_released: HashSet<u8>,
    pending_mouse_button_pressed: HashSet<u8>,
    pending_mouse_button_released: HashSet<u8>,
    held_gamepad_buttons: HashSet<(GamepadId, GamepadButton)>,
    gamepad_button_just_pressed: HashSet<(GamepadId, GamepadButton)>,
    gamepad_button_just_released: HashSet<(GamepadId, GamepadButton)>,
    pending_gamepad_button_pressed: HashSet<(GamepadId, GamepadButton)>,
    pending_gamepad_button_released: HashSet<(GamepadId, GamepadButton)>,
    gamepad_axes: HashMap<(GamepadId, GamepadAxis), f32>,
    primary_held: bool,
    /// `KeyInput` events received since the last `update()`, drained into
    /// `ActionMap` after the simulator has run (so UI priority is respected).
    pending_key_inputs: Vec<KeyInput>,
}

impl Default for InputHub {
    fn default() -> Self {
        Self::new()
    }
}

impl InputHub {
    pub fn new() -> Self {
        Self {
            simulator: clay_ui::InputSimulator::default(),
            actions: ActionMap::new(),
            cursor: clay_ui::WindowLogicalPx::ZERO,
            cursor_initialized: false,
            mouse_delta: glam::Vec2::ZERO,
            pending_mouse_delta: glam::Vec2::ZERO,
            held_keys: HashSet::new(),
            key_just_pressed: HashSet::new(),
            key_just_released: HashSet::new(),
            pending_key_pressed: HashSet::new(),
            pending_key_released: HashSet::new(),
            held_mouse_buttons: HashSet::new(),
            mouse_button_just_pressed: HashSet::new(),
            mouse_button_just_released: HashSet::new(),
            pending_mouse_button_pressed: HashSet::new(),
            pending_mouse_button_released: HashSet::new(),
            held_gamepad_buttons: HashSet::new(),
            gamepad_button_just_pressed: HashSet::new(),
            gamepad_button_just_released: HashSet::new(),
            pending_gamepad_button_pressed: HashSet::new(),
            pending_gamepad_button_released: HashSet::new(),
            gamepad_axes: HashMap::new(),
            primary_held: false,
            pending_key_inputs: Vec::new(),
        }
    }

    // ── EngineApp bridge ──────────────────────────────────────────────────────

    /// Call from `EngineApp::pointer_moved`.
    pub fn on_pointer_moved(&mut self, pos: clay_ui::WindowLogicalPx) {
        use clay_ui::{InputEvent, InteractionPhase, PointerButton, PointerState};
        if self.cursor_initialized {
            self.pending_mouse_delta += pos.to_vec2() - self.cursor.to_vec2();
        } else {
            self.cursor_initialized = true;
        }
        self.cursor = pos;
        let phase = if self.primary_held {
            InteractionPhase::Pressed
        } else {
            InteractionPhase::Released
        };
        self.simulator.queue(InputEvent::Pointer(PointerState {
            position: pos.to_vec2(),
            button: PointerButton::Primary,
            phase,
        }));
    }

    /// Call from `EngineApp::pointer_button`.
    ///
    /// `button` follows the convention 0 = primary, 1 = secondary, 2 = middle.
    pub fn on_pointer_button(&mut self, pos: clay_ui::WindowLogicalPx, button: u8, pressed: bool) {
        use clay_ui::{InputEvent, InteractionPhase, PointerButton, PointerState};
        self.cursor = pos;
        self.cursor_initialized = true;
        if button == 0 {
            self.primary_held = pressed;
        }
        if pressed {
            if self.held_mouse_buttons.insert(button) {
                self.pending_mouse_button_pressed.insert(button);
            }
            self.pending_mouse_button_released.remove(&button);
        } else {
            if self.held_mouse_buttons.remove(&button) {
                self.pending_mouse_button_released.insert(button);
            }
            self.pending_mouse_button_pressed.remove(&button);
        }
        let btn = match button {
            0 => PointerButton::Primary,
            1 => PointerButton::Secondary,
            2 => PointerButton::Middle,
            n => PointerButton::Extra(n),
        };
        let phase = if pressed {
            InteractionPhase::PressedThisFrame
        } else {
            InteractionPhase::ReleasedThisFrame
        };
        self.simulator.queue(InputEvent::Pointer(PointerState {
            position: pos.to_vec2(),
            button: btn,
            phase,
        }));
    }

    /// Call from `EngineApp::pointer_scroll`.
    pub fn on_pointer_scroll(&mut self, delta_x: f32, delta_y: f32) {
        use clay_ui::InputEvent;
        self.simulator.queue(InputEvent::Scroll {
            target: None,
            delta: glam::Vec2::new(delta_x, delta_y),
        });
    }

    /// Call from `EngineApp::key_input`.
    ///
    /// Routes the key to the UI simulator and buffers it for action dispatch.
    pub fn on_key_input(&mut self, input: &KeyInput) {
        use clay_ui::InputEvent;

        let modifiers = clay_modifiers(input.modifiers);
        if !input.repeat {
            match input.state {
                KeyInputState::Pressed => {
                    if self.held_keys.insert(input.key.clone()) {
                        self.pending_key_pressed.insert(input.key.clone());
                    }
                    self.pending_key_released.remove(&input.key);
                }
                KeyInputState::Released => {
                    if self.held_keys.remove(&input.key) {
                        self.pending_key_released.insert(input.key.clone());
                    }
                    self.pending_key_pressed.remove(&input.key);
                }
            }
        }

        // Route key name to simulator.
        if let KeyToken::Key(name) = &input.key {
            self.simulator.queue(InputEvent::KeyWithModifiers {
                name: name.clone(),
                pressed: input.state == KeyInputState::Pressed,
                repeat: input.repeat,
                modifiers,
            });
        }

        // Route text on every press. Text callbacks should see repeating text
        // exactly when the platform's text-input path produced it.
        if input.state == KeyInputState::Pressed {
            if let Some(text) = &input.text {
                self.simulator.queue(InputEvent::TextWithModifiers {
                    text: text.clone(),
                    modifiers,
                });
            }
        }

        // Buffer for deferred action dispatch after simulator.update().
        self.pending_key_inputs.push(input.clone());
    }

    /// Call from a gamepad backend when a button is pressed or released.
    pub fn on_gamepad_button(&mut self, gamepad: GamepadId, button: GamepadButton, pressed: bool) {
        let key = (gamepad, button);
        if pressed {
            if self.held_gamepad_buttons.insert(key) {
                self.pending_gamepad_button_pressed.insert(key);
            }
            self.pending_gamepad_button_released.remove(&key);
        } else {
            if self.held_gamepad_buttons.remove(&key) {
                self.pending_gamepad_button_released.insert(key);
            }
            self.pending_gamepad_button_pressed.remove(&key);
        }
    }

    /// Call from a gamepad backend when an analog axis changes.
    pub fn on_gamepad_axis(&mut self, gamepad: GamepadId, axis: GamepadAxis, value: f32) {
        self.gamepad_axes
            .insert((gamepad, axis), value.clamp(-1.0, 1.0));
    }

    /// Clear all cached state for a disconnected gamepad.
    pub fn clear_gamepad(&mut self, gamepad: GamepadId) {
        self.held_gamepad_buttons.retain(|(id, _)| *id != gamepad);
        self.gamepad_button_just_pressed
            .retain(|(id, _)| *id != gamepad);
        self.gamepad_button_just_released
            .retain(|(id, _)| *id != gamepad);
        self.pending_gamepad_button_pressed
            .retain(|(id, _)| *id != gamepad);
        self.pending_gamepad_button_released
            .retain(|(id, _)| *id != gamepad);
        self.gamepad_axes.retain(|(id, _), _| *id != gamepad);
    }

    /// Feed a gamepad button input through the same path as
    /// [`InputHub::on_gamepad_button`]. Use in tests or replay.
    pub fn simulate_gamepad_button(&mut self, input: GamepadButtonInput) {
        self.on_gamepad_button(
            input.gamepad,
            input.button,
            input.state == KeyInputState::Pressed,
        );
    }

    /// Feed a gamepad axis input through the same path as
    /// [`InputHub::on_gamepad_axis`]. Use in tests or replay.
    pub fn simulate_gamepad_axis(&mut self, input: GamepadAxisInput) {
        self.on_gamepad_axis(input.gamepad, input.axis, input.value);
    }

    // ── Simulation / testing ──────────────────────────────────────────────────

    /// Queue a low-level UI event directly into the simulator.
    ///
    /// Use this in tests or replay scenarios instead of the `on_*` methods.
    pub fn queue(&mut self, event: clay_ui::InputEvent) {
        self.simulator.queue(event);
    }

    /// Feed a `KeyInput` through the same path as `on_key_input` — routes to
    /// both the UI simulator and the action map buffer. Use in tests.
    pub fn simulate_key(&mut self, input: &KeyInput) {
        self.on_key_input(input);
    }

    // ── Frame update ──────────────────────────────────────────────────────────

    /// Process all queued events for this frame.
    ///
    /// Call once per frame after the layout tree is built. Returns the
    /// topmost interactive element under the pointer (same as
    /// [`InputSimulator::update`]).
    ///
    /// Internally:
    /// 1. Clears `ActionMap` per-frame state (`just_pressed` / `just_released`).
    /// 2. Runs `InputSimulator::update` — resolves UI events against the layout.
    /// 3. Dispatches buffered key inputs to `ActionMap`, using per-key UI
    ///    consumption data so unrelated game actions are not blocked.
    pub fn update(&mut self, tree: &clay_ui::LayoutTree) -> Option<clay_ui::Hit> {
        self.publish_polling_frame();
        self.actions.end_frame();
        let hit = self.simulator.update(tree);
        let pending = std::mem::take(&mut self.pending_key_inputs);
        for ki in &pending {
            let key_name = match &ki.key {
                KeyToken::Key(name) => name.as_str(),
                KeyToken::Modifier(_) => continue,
            };
            let this_key_consumed = self.simulator.key_input_consumed(key_name);
            self.actions.process(ki, this_key_consumed);
        }
        self.actions.process_polling(
            &self.held_mouse_buttons,
            &self.held_gamepad_buttons,
            &self.gamepad_axes,
        );
        hit
    }

    fn publish_polling_frame(&mut self) {
        self.mouse_delta = self.pending_mouse_delta;
        self.pending_mouse_delta = glam::Vec2::ZERO;
        self.key_just_pressed = std::mem::take(&mut self.pending_key_pressed);
        self.key_just_released = std::mem::take(&mut self.pending_key_released);
        self.mouse_button_just_pressed = std::mem::take(&mut self.pending_mouse_button_pressed);
        self.mouse_button_just_released = std::mem::take(&mut self.pending_mouse_button_released);
        self.gamepad_button_just_pressed = std::mem::take(&mut self.pending_gamepad_button_pressed);
        self.gamepad_button_just_released =
            std::mem::take(&mut self.pending_gamepad_button_released);
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    /// Access the underlying UI input simulator for advanced configuration
    /// (registering scroll configs, widget behaviors, focus scopes, etc.).
    pub fn simulator(&self) -> &clay_ui::InputSimulator {
        &self.simulator
    }

    pub fn simulator_mut(&mut self) -> &mut clay_ui::InputSimulator {
        &mut self.simulator
    }

    /// Access the action map to bind and query game actions.
    pub fn actions(&self) -> &ActionMap {
        &self.actions
    }

    pub fn actions_mut(&mut self) -> &mut ActionMap {
        &mut self.actions
    }

    /// Current cursor position in top-left/Y-down `WindowLogicalPx`.
    pub fn cursor_pos(&self) -> clay_ui::WindowLogicalPx {
        self.cursor
    }

    /// Current mouse position in top-left/Y-down `WindowLogicalPx`.
    pub fn mouse_position(&self) -> clay_ui::WindowLogicalPx {
        self.cursor
    }

    /// Mouse movement accumulated since the previous [`InputHub::update`].
    pub fn mouse_delta(&self) -> glam::Vec2 {
        self.mouse_delta
    }

    /// `true` while the raw key is held down, regardless of UI consumption.
    pub fn is_key_pressed(&self, key: impl Into<KeyToken>) -> bool {
        self.held_keys.contains(&key.into())
    }

    /// `true` on the first frame the raw key was pressed, regardless of UI consumption.
    pub fn is_key_just_pressed(&self, key: impl Into<KeyToken>) -> bool {
        self.key_just_pressed.contains(&key.into())
    }

    /// `true` on the frame the raw key was released, regardless of UI consumption.
    pub fn is_key_just_released(&self, key: impl Into<KeyToken>) -> bool {
        self.key_just_released.contains(&key.into())
    }

    /// `true` while the raw mouse button is held down.
    ///
    /// Buttons follow the shell convention: 0 = primary, 1 = secondary, 2 = middle.
    pub fn is_mouse_button_pressed(&self, button: u8) -> bool {
        self.held_mouse_buttons.contains(&button)
    }

    /// `true` on the first frame the raw mouse button was pressed.
    pub fn is_mouse_button_just_pressed(&self, button: u8) -> bool {
        self.mouse_button_just_pressed.contains(&button)
    }

    /// `true` on the frame the raw mouse button was released.
    pub fn is_mouse_button_just_released(&self, button: u8) -> bool {
        self.mouse_button_just_released.contains(&button)
    }

    /// `true` while the gamepad button is held down.
    pub fn is_gamepad_button_pressed(&self, gamepad: GamepadId, button: GamepadButton) -> bool {
        self.held_gamepad_buttons.contains(&(gamepad, button))
    }

    /// `true` on the first frame the gamepad button was pressed.
    pub fn is_gamepad_button_just_pressed(
        &self,
        gamepad: GamepadId,
        button: GamepadButton,
    ) -> bool {
        self.gamepad_button_just_pressed
            .contains(&(gamepad, button))
    }

    /// `true` on the frame the gamepad button was released.
    pub fn is_gamepad_button_just_released(
        &self,
        gamepad: GamepadId,
        button: GamepadButton,
    ) -> bool {
        self.gamepad_button_just_released
            .contains(&(gamepad, button))
    }

    /// Current value for a gamepad axis in the normalized `[-1, 1]` range.
    ///
    /// Returns `0.0` if the axis has not been seen yet.
    pub fn gamepad_axis(&self, gamepad: GamepadId, axis: GamepadAxis) -> f32 {
        self.gamepad_axes
            .get(&(gamepad, axis))
            .copied()
            .unwrap_or(0.0)
    }

    // ── Simulator convenience forwards ────────────────────────────────────────

    pub fn widget_state(&self, id: &clay_ui::ElementId) -> clay_ui::WidgetState {
        self.simulator.widget_state(id)
    }

    pub fn scroll_offset(&self, id: &clay_ui::ElementId) -> glam::Vec2 {
        self.simulator.scroll_offset(id)
    }

    pub fn scroll_layout_offset(&self, id: &clay_ui::ElementId) -> glam::Vec2 {
        self.simulator.scroll_layout_offset(id)
    }

    pub fn slider_value(&self, id: &clay_ui::ElementId) -> f32 {
        self.simulator.slider_value(id)
    }

    pub fn last_event_result(&self) -> &clay_ui::UiEventResult {
        self.simulator.last_event_result()
    }

    pub fn bubble_activated(&self, id: &clay_ui::ElementId) -> bool {
        self.simulator.bubble_activated(id)
    }

    pub fn hovered(&self) -> Option<&clay_ui::ElementId> {
        self.simulator.hovered()
    }

    pub fn focused(&self) -> Option<&clay_ui::ElementId> {
        self.simulator.focused()
    }
}

/// Returns `true` when `input` satisfies `binding`.
///
/// All required modifiers must be held; extra modifiers beyond those listed in
/// the binding are allowed (lenient matching suits most game bindings).
fn keybind_matches(binding: &Keybind, input: &KeyInput) -> bool {
    let key_ok = match (binding.key(), &input.key) {
        (Some(k), KeyToken::Key(ik)) => k == ik,
        _ => false,
    };
    key_ok
        && binding
            .modifiers()
            .iter()
            .all(|&m| input.modifiers.contains(m))
}

fn display_key_name(name: &str) -> String {
    if let Some(rest) = name.strip_prefix("Key") {
        return rest.to_string();
    }
    if let Some(rest) = name.strip_prefix("Digit") {
        return rest.to_string();
    }
    match name {
        "Escape" => "Esc".to_string(),
        "Space" => "Space".to_string(),
        "Enter" => "Enter".to_string(),
        "Tab" => "Tab".to_string(),
        "Backspace" => "Backspace".to_string(),
        "ArrowLeft" => "Left".to_string(),
        "ArrowRight" => "Right".to_string(),
        "ArrowUp" => "Up".to_string(),
        "ArrowDown" => "Down".to_string(),
        other => other.to_string(),
    }
}

#[cfg(feature = "app-shell")]
impl KeyInput {
    pub(crate) fn from_winit(
        event: &winit::event::KeyEvent,
        modifiers: KeyModifiers,
    ) -> Option<Self> {
        use winit::keyboard::{Key, PhysicalKey};

        let key = match event.physical_key {
            PhysicalKey::Code(code) => key_token_from_key_code(code),
            PhysicalKey::Unidentified(_) => return None,
        };
        let text = match &event.logical_key {
            Key::Character(text) => Some(text.to_string()),
            _ => None,
        };
        Some(Self {
            key,
            state: match event.state {
                winit::event::ElementState::Pressed => KeyInputState::Pressed,
                winit::event::ElementState::Released => KeyInputState::Released,
            },
            modifiers,
            repeat: event.repeat,
            text,
        })
    }
}

#[cfg(feature = "app-shell")]
pub(crate) fn key_modifiers_from_winit(modifiers: winit::keyboard::ModifiersState) -> KeyModifiers {
    KeyModifiers {
        ctrl: modifiers.control_key(),
        alt: modifiers.alt_key(),
        shift: modifiers.shift_key(),
        super_: modifiers.super_key(),
    }
}

#[cfg(feature = "app-shell")]
fn key_token_from_key_code(code: winit::keyboard::KeyCode) -> KeyToken {
    use winit::keyboard::KeyCode;

    match code {
        KeyCode::ShiftLeft | KeyCode::ShiftRight => KeyToken::Modifier(KeyModifier::Shift),
        KeyCode::ControlLeft | KeyCode::ControlRight => KeyToken::Modifier(KeyModifier::Ctrl),
        KeyCode::AltLeft | KeyCode::AltRight => KeyToken::Modifier(KeyModifier::Alt),
        KeyCode::SuperLeft | KeyCode::SuperRight => KeyToken::Modifier(KeyModifier::Super),
        other => KeyToken::Key(format!("{other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
