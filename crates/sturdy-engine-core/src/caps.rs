#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Caps {
    pub supports_raytracing: bool,
    pub supports_mesh_shading: bool,
    pub supports_bindless: bool,
    pub max_mip_levels: u32,
    pub max_frames_in_flight: u32,
}

impl Default for Caps {
    fn default() -> Self {
        Self {
            supports_raytracing: false,
            supports_mesh_shading: false,
            supports_bindless: false,
            max_mip_levels: 1,
            max_frames_in_flight: 2,
        }
    }
}
