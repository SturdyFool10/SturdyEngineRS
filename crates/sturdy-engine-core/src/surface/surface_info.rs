use crate::{Format, SurfaceColorSpace, SurfaceSize};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SurfaceInfo {
    pub size: SurfaceSize,
    pub format: Format,
    pub color_space: SurfaceColorSpace,
}
