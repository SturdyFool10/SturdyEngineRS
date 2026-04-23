use crate::{PlatformKind, WindowAppearanceCaps};

/// Linux platform adapter entry point.
///
/// Planned backdrop/effect support:
/// - Wayland `ext-background-effect-v1` as the primary protocol path
/// - older compositor-specific blur protocols only as compatibility fallbacks
/// - clean fallback to transparency/no backdrop effect when no supported
///   compositor protocol is available
pub fn platform_kind() -> PlatformKind {
    PlatformKind::Linux
}

pub fn window_appearance_caps() -> WindowAppearanceCaps {
    WindowAppearanceCaps::default()
}
