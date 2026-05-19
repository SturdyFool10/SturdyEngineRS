use crate::{
    Format, Result, SurfaceColorSpace, SurfaceHdrPreference, SurfacePresentMode, SurfaceSize,
};

#[derive(Clone, Debug, Default)]
pub struct SurfaceRecreateDesc {
    pub size: Option<SurfaceSize>,
    /// Surface transparency/compositing preference. `None` keeps the existing preference.
    pub transparent: Option<bool>,
    /// HDR preference override.  `None` keeps the surface's existing preference.
    pub hdr: Option<SurfaceHdrPreference>,
    /// Preferred swapchain format.  `None` lets the backend choose.
    pub preferred_format: Option<Format>,
    /// Preferred color space.  `None` lets the backend choose.
    pub preferred_color_space: Option<SurfaceColorSpace>,
    /// Preferred present mode.  `None` lets the backend choose (usually Mailbox → FIFO).
    pub preferred_present_mode: Option<SurfacePresentMode>,
}

impl SurfaceRecreateDesc {
    pub fn validate(&self) -> Result<()> {
        if let Some(size) = self.size {
            size.validate()?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "surface_recreate_desc_tests.rs"]
mod tests;
