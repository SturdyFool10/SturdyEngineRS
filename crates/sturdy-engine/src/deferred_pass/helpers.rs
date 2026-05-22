use std::path::PathBuf;

use glam::Mat4;

pub(super) fn engine_shader(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(name)
}

/// Extract camera near and far clip planes from a RH perspective projection matrix.
///
/// For glam `perspective_rh` (Vulkan depth 0..1):
///   col2.z (a) = far / (near - far)      [negative]
///   col3.z (b) = near * far / (near - far) [negative]
///
/// Solving: near = b / a,  far = b / (a + 1)
pub(super) fn extract_near_far(proj: Mat4) -> (f32, f32) {
    let a = proj.z_axis.z;
    let b = proj.w_axis.z;

    if a.abs() < 1e-7 {
        tracing::warn!(
            "extract_near_far: degenerate projection (a={a:.6}), using fallback near=0.1 far=1000"
        );
        return (0.1, 1000.0);
    }

    // near = b / a
    // far  = b / (a + 1)   ← correct derivation for Vulkan 0..1 depth
    let near = b / a;
    let denom = a + 1.0;
    let far = if denom.abs() < 1e-7 {
        // Degenerate (infinite far plane) — fall back to a large value.
        tracing::debug!(
            "extract_near_far: near-infinite far plane (a={a:.6}), clamping far to near*10000"
        );
        near * 10000.0
    } else {
        b / denom
    };

    let near = near.abs().max(0.001);
    let far = far.abs();
    if !near.is_finite() || !far.is_finite() || far <= near {
        tracing::error!(
            "extract_near_far: invalid result near={near:.3} far={far:.3} \
             (a={a:.6} b={b:.6}); using fallback"
        );
        return (0.1, 1000.0);
    }
    (near, far)
}
