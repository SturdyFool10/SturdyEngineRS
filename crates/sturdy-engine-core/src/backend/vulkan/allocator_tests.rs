// Tests extracted from crates/sturdy-engine-core/src/backend/vulkan/allocator.rs
// See scripts/extract_tests.py for the extraction logic.

use super::*;

#[test]
fn block_allocate_and_free_round_trip_restores_capacity() {
    let mut block = Block::new(0, vk::DeviceMemory::default(), 64, None);

    let offset = block.allocate(16, 8, 0).expect("valid request");
    assert_eq!(offset, Some(0));
    assert!(!block.is_empty());

    block.free(0, 16, 0).expect("valid free");
    assert!(block.is_empty());
}

#[test]
fn block_allocate_rejects_non_power_of_two_alignment() {
    let mut block = Block::new(0, vk::DeviceMemory::default(), 64, None);

    let error = block
        .allocate(16, 3, 0)
        .expect_err("invalid alignment should return an error");

    assert!(format!("{error}").contains("invalid alignment"));
}

#[test]
fn block_free_rejects_out_of_range_deallocation() {
    let mut block = Block::new(0, vk::DeviceMemory::default(), 64, None);

    let error = block
        .free(60, 8, 0)
        .expect_err("out-of-range free should return an error");

    assert!(format!("{error}").contains("invalid deallocation range"));
}
