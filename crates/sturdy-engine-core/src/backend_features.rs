#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct BackendFeatures {
    pub ray_tracing: bool,
    pub mesh_shading: bool,
    pub descriptor_indexing: bool,
    pub timeline_semaphores: bool,
    pub dynamic_rendering: bool,
    pub synchronization2: bool,
    pub hdr_output: bool,
    pub variable_rate_shading: bool,
}
