#[cfg(not(target_arch = "wasm32"))]
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use crate::{Error, Result};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SurfaceSize {
    pub width: u32,
    pub height: u32,
}

impl SurfaceSize {
    pub fn validate(self) -> Result<()> {
        if self.width == 0 || self.height == 0 {
            return Err(Error::InvalidInput("surface size must be non-zero".into()));
        }
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Copy, Clone, Debug)]
pub struct NativeSurfaceDesc {
    pub display_handle: RawDisplayHandle,
    pub window_handle: RawWindowHandle,
    pub size: SurfaceSize,
}

#[cfg(not(target_arch = "wasm32"))]
impl NativeSurfaceDesc {
    pub fn validate(&self) -> Result<()> {
        self.size.validate()
    }
}
