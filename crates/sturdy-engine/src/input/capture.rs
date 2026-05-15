use std::collections::BTreeSet;

use super::{KeyInput, KeyInputState, KeyModifier, KeyToken, Keybind};

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
