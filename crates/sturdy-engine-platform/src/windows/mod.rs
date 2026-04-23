use crate::{PlatformKind, WindowAppearanceCaps};

pub fn platform_kind() -> PlatformKind {
    PlatformKind::Windows
}

pub fn window_appearance_caps() -> WindowAppearanceCaps {
    WindowAppearanceCaps::default()
}
