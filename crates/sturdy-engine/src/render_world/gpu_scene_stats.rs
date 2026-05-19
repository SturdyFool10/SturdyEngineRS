/// Summary returned after uploading render-world object data to the GPU scene buffer.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct RenderWorldGpuSceneStats {
    pub instance_count: usize,
    pub batch_count: usize,
    pub capacity: usize,
    pub reallocated: bool,
    pub indirect_reallocated: bool,
    pub full_rebuild: bool,
    pub uploaded_instances: usize,
}
