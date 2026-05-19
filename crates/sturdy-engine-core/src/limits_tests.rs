// Tests extracted from crates/sturdy-engine-core/src/limits.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn default_limits_are_nonzero() {
    let limits = Limits::default();

    assert!(limits.max_texture_2d_size > 0);
    assert!(limits.max_texture_array_layers > 0);
    assert!(limits.max_color_attachments > 0);
    assert!(limits.max_bound_descriptor_sets > 0);
    assert!(limits.max_per_stage_samplers > 0);
    assert!(limits.max_per_stage_sampled_images > 0);
    assert!(limits.max_per_stage_storage_images > 0);
    assert!(limits.max_per_stage_uniform_buffers > 0);
    assert!(limits.max_per_stage_storage_buffers > 0);
    assert!(limits.max_descriptor_set_samplers > 0);
    assert!(limits.max_descriptor_set_sampled_images > 0);
    assert!(limits.max_descriptor_set_storage_images > 0);
    assert!(limits.max_descriptor_set_uniform_buffers > 0);
    assert!(limits.max_descriptor_set_storage_buffers > 0);
    assert!(
        limits
            .max_compute_workgroup_size
            .iter()
            .all(|size| *size > 0)
    );
    assert!(limits.max_compute_invocations > 0);
    assert!(limits.max_push_constants_size > 0);
}
