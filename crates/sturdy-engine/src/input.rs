use std::{
    collections::{BTreeMap, BTreeSet},
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
