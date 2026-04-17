#[repr(C)]
#[derive(Copy, Clone)]
pub struct PushData {
    offset: [f32; 2],
    scale: [f32; 2],
    tint: [f32; 4],
}

pub const PUSH_CONSTANT_BYTES: u32 = std::mem::size_of::<PushData>() as u32;

pub fn animated_push_data(time_seconds: f32) -> PushData {
    let x = time_seconds.sin() * 0.35;
    let y = (time_seconds * 0.7).cos() * 0.18;
    let pulse = 0.65 + 0.25 * (time_seconds * 1.7).sin();
    PushData {
        offset: [x, y],
        scale: [0.75 + pulse * 0.2, 0.75 + pulse * 0.2],
        tint: [0.75 + pulse * 0.25, 0.9, 1.15 - pulse * 0.25, 1.0],
    }
}
