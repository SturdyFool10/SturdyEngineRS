use crate::{ImageHandle, OpticalFlowSessionHandle};

#[derive(Copy, Clone, Debug)]
pub struct OpticalFlowSessionDesc {
    pub width: u32,
    pub height: u32,
    pub output_grid_size: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct OpticalFlowEstimateDesc {
    pub session: OpticalFlowSessionHandle,
    pub input_current: ImageHandle,
    pub input_previous: ImageHandle,
    pub output_motion_vectors: ImageHandle,
    pub input_hint: Option<ImageHandle>,
}
