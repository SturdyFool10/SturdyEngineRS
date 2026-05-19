// Tests extracted from crates/sturdy-engine-core/src/surface/surface_recreate_desc.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn default_recreate_desc_is_valid() {
    SurfaceRecreateDesc::default().validate().unwrap();
}

#[test]
fn recreate_desc_rejects_zero_size() {
    let err = SurfaceRecreateDesc {
        size: Some(SurfaceSize {
            width: 0,
            height: 720,
        }),
        ..Default::default()
    }
    .validate()
    .unwrap_err();
    assert!(matches!(err, crate::Error::InvalidInput(_)));
}
