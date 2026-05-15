use super::{KeyInput, KeyInputState, KeyModifier, KeyModifiers, KeyToken};

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
