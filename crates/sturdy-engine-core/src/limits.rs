#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Limits {
    pub max_image_dimension_2d: u32,
    pub max_image_dimension_3d: u32,
    pub max_texture_2d_size: u32,
    pub max_texture_array_layers: u32,
    pub max_mip_levels: u32,
    pub max_push_constants_size: u32,
    pub max_bound_descriptor_sets: u32,
    pub max_color_attachments: u32,
    pub max_compute_workgroup_size: [u32; 3],
    pub max_compute_invocations: u32,
    pub max_frames_in_flight: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_image_dimension_2d: 1,
            max_image_dimension_3d: 1,
            max_texture_2d_size: 1,
            max_texture_array_layers: 1,
            max_mip_levels: 1,
            max_push_constants_size: 128,
            max_bound_descriptor_sets: 4,
            max_color_attachments: 4,
            max_compute_workgroup_size: [1, 1, 1],
            max_compute_invocations: 1,
            max_frames_in_flight: 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_limits_are_nonzero() {
        let limits = Limits::default();

        assert!(limits.max_texture_2d_size > 0);
        assert!(limits.max_texture_array_layers > 0);
        assert!(limits.max_color_attachments > 0);
        assert!(
            limits
                .max_compute_workgroup_size
                .iter()
                .all(|size| *size > 0)
        );
        assert!(limits.max_compute_invocations > 0);
        assert!(limits.max_push_constants_size > 0);
    }
}
