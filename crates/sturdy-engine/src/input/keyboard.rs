use super::{KeyModifier, KeyToken};

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
