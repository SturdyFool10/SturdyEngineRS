use crate::{Buffer, BufferDesc, BufferUsage, Engine, Result};

/// Vertex layout for the fullscreen triangle buffer.
///
/// Three vertices at (-1,-3), (-1,1), (3,1) in clip space. The triangle covers
/// the whole screen with UV coordinates correctly mapped to [0,1]×[0,1].
#[repr(C)]
pub(crate) struct FullscreenVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
}

pub(crate) fn create_fullscreen_triangle(engine: &Engine) -> Result<Buffer> {
    let vertices = [
        FullscreenVertex {
            position: [-1.0, -3.0],
            uv: [0.0, -1.0],
        },
        FullscreenVertex {
            position: [-1.0, 1.0],
            uv: [0.0, 1.0],
        },
        FullscreenVertex {
            position: [3.0, 1.0],
            uv: [2.0, 1.0],
        },
    ];
    let buffer = engine.create_buffer(BufferDesc {
        size: std::mem::size_of_val(&vertices) as u64,
        usage: BufferUsage::VERTEX,
    })?;
    buffer.write(0, bytes_of_slice(&vertices))?;
    buffer.set_debug_name("shader-program-fullscreen-triangle")?;
    Ok(buffer)
}

fn bytes_of_slice<T>(values: &[T]) -> &[u8] {
    //panic allowed, reason = "reinterpreting T as bytes is always safe for POD vertex data"
    unsafe {
        std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), std::mem::size_of_val(values))
    }
}
