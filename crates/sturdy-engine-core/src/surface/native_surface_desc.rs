use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use crate::{Result, SurfaceHdrPreference, SurfacePresentMode, SurfaceSize};

#[derive(Clone, Debug)]
pub struct NativeSurfaceDesc {
    pub display_handle: RawDisplayHandle,
    pub window_handle: RawWindowHandle,
    pub size: SurfaceSize,
    pub transparent: bool,
    /// HDR output preference.  Backends use this to choose the swapchain format
    /// and color space.  Defaults to `Sdr`.
    pub hdr: SurfaceHdrPreference,
    /// Preferred present mode.  `None` lets the backend choose (Mailbox → FIFO).
    pub preferred_present_mode: Option<SurfacePresentMode>,
}

impl NativeSurfaceDesc {
    pub fn new(
        display_handle: RawDisplayHandle,
        window_handle: RawWindowHandle,
        size: SurfaceSize,
    ) -> Self {
        Self {
            display_handle,
            window_handle,
            size,
            transparent: false,
            hdr: SurfaceHdrPreference::Sdr,
            preferred_present_mode: None,
        }
    }

    pub fn validate(&self) -> Result<()> {
        self.size.validate()
    }
}
