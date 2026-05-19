// Tests extracted from crates/sturdy-engine-core/src/acceleration_structure.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn acceleration_structure_desc_rejects_zero_size() {
    let err = AccelerationStructureDesc {
        kind: AccelerationStructureKind::BottomLevel,
        size: 0,
    }
    .validate()
    .unwrap_err();

    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("non-zero"));
}
