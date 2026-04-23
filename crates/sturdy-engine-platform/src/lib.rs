mod capability;
mod platform;
mod window_appearance;
mod window_effect_region;
mod window_material_kind;

pub mod linux;
pub mod macos;
pub mod windows;

pub use capability::{PlatformCapabilityState, WindowAppearanceCaps, WindowMaterialSupport};
pub use platform::{PlatformKind, current_platform};
pub use window_appearance::{
    SurfaceTransparency, WindowAppearance, WindowBackdrop, WindowBlurDesc, WindowCornerStyle,
    WindowEffectQuality, WindowShadowMode, WindowTransparencyDesc,
};
pub use window_effect_region::WindowEffectRegion;
pub use window_material_kind::WindowMaterialKind;
