use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use crate::{Result, SurfaceSize};

#[derive(Copy, Clone, Debug)]
pub struct NativeSurfaceDesc {
    pub display_handle: RawDisplayHandle,
    pub window_handle: RawWindowHandle,
    pub size: SurfaceSize,
}

impl NativeSurfaceDesc {
    pub fn validate(&self) -> Result<()> {
        self.size.validate()
    }
}
