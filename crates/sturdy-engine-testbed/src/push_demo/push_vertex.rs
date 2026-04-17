#[repr(C)]
#[derive(Copy, Clone)]
pub struct PushVertex {
    pub position: [f32; 2],
    pub color: [f32; 3],
}
