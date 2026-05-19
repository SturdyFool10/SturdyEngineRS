// Tests extracted from crates/sturdy-engine/src/hiz_pass.rs
// Runtime code should stay separate from test code.

use super::full_mip_count;

#[test]
fn hiz_full_mip_count_handles_non_power_of_two_extents() {
    assert_eq!(full_mip_count(1, 1), 1);
    assert_eq!(full_mip_count(2, 1), 2);
    assert_eq!(full_mip_count(3, 2), 2);
    assert_eq!(full_mip_count(4, 4), 3);
    assert_eq!(full_mip_count(1920, 1080), 11);
}
