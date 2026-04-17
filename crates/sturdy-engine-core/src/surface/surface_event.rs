use crate::{Format, SurfaceColorSpace, SurfaceInfo, SurfaceSize};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SurfaceEvent {
    Resized {
        old: SurfaceSize,
        new: SurfaceSize,
    },
    FormatChanged {
        old: Format,
        new: Format,
    },
    ColorSpaceChanged {
        old: SurfaceColorSpace,
        new: SurfaceColorSpace,
    },
    Recreated {
        old: SurfaceInfo,
        new: SurfaceInfo,
    },
}
