use crate::{Format, SurfaceColorSpace, SurfacePresentMode};

/// One (format, color-space) pair a surface supports.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SurfaceFormatInfo {
    pub format: Format,
    pub color_space: SurfaceColorSpace,
}

/// All surface properties returned by a capabilities query.
#[derive(Clone, Debug)]
pub struct SurfaceCapabilities {
    pub formats: Vec<SurfaceFormatInfo>,
    pub present_modes: Vec<SurfacePresentMode>,
    pub min_image_count: u32,
    pub max_image_count: u32,
    pub current_width: u32,
    pub current_height: u32,
}

impl Default for SurfaceCapabilities {
    fn default() -> Self {
        Self {
            formats: Vec::new(),
            present_modes: vec![SurfacePresentMode::Fifo],
            min_image_count: 2,
            max_image_count: 0,
            current_width: 0,
            current_height: 0,
        }
    }
}
