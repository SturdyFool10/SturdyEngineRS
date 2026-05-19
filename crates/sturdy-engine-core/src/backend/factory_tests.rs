// Tests extracted from crates/sturdy-engine-core/src/backend/factory.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn null_backend_factory_creates_null_backend() {
    let backend = create_backend(&DeviceDesc {
        backend: BackendKind::Null,
        ..DeviceDesc::default()
    })
    .unwrap();

    assert_eq!(backend.kind(), BackendKind::Null);
}

#[test]
fn null_adapter_enumeration_is_empty() {
    assert!(enumerate_adapters(BackendKind::Null).unwrap().is_empty());
}
