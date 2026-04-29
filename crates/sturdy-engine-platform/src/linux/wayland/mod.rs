use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use crate::{NativeWindowAppearanceError, WindowAppearance};

mod background_effect;

pub fn apply_native_window_appearance(
    display: RawDisplayHandle,
    window: RawWindowHandle,
    size: Option<(u32, u32)>,
    appearance: WindowAppearance,
) -> Result<(), NativeWindowAppearanceError> {
    background_effect::apply(display, window, size, appearance)
}
