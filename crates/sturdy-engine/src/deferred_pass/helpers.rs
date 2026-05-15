use std::path::PathBuf;

use glam::Mat4;

pub(super) fn engine_shader(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(name)
}

/// Extract camera near and far clip planes from a RH perspective projection matrix.
pub(super) fn extract_near_far(proj: Mat4) -> (f32, f32) {
    // For glam perspective_rh: col3.z = near*far/(near-far), col2.z = far/(near-far).
    let a = proj.z_axis.z;
    let b = proj.w_axis.z;
    if a.abs() < 1e-7 {
        return (0.1, 1000.0);
    }

    let near = b / a;
    let far = near * a / (a - 1.0 + 1e-7);
    (near.abs().max(0.01), far.abs().max(near.abs() + 1.0))
}
