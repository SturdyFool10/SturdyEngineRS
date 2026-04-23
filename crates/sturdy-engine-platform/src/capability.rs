use crate::{WindowEffectQuality, WindowMaterialKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PlatformCapabilityState {
    Unsupported,
    Supported,
    RuntimeReconfigureSupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct WindowMaterialSupport {
    pub kind: WindowMaterialKind,
    pub quality: WindowEffectQuality,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WindowAppearanceCaps {
    pub transparency: Option<PlatformCapabilityState>,
    pub blur: Option<PlatformCapabilityState>,
    pub materials: Vec<WindowMaterialSupport>,
    pub custom_regions: Option<PlatformCapabilityState>,
    pub live_reconfiguration: Option<PlatformCapabilityState>,
}
