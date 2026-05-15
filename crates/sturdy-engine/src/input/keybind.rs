use std::{fmt, str::FromStr};

use super::display::display_key_name;

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
