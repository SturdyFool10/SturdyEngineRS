use super::KeyInputState;

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
