#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Limits {
    pub max_image_dimension_2d: u32,
    pub max_image_dimension_3d: u32,
    pub max_mip_levels: u32,
    pub max_push_constants_size: u32,
    pub max_bound_descriptor_sets: u32,
    pub max_color_attachments: u32,
    pub max_frames_in_flight: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_image_dimension_2d: 1,
            max_image_dimension_3d: 1,
            max_mip_levels: 1,
            max_push_constants_size: 128,
            max_bound_descriptor_sets: 4,
            max_color_attachments: 4,
            max_frames_in_flight: 2,
        }
    }
}
