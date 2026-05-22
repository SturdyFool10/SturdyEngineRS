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

#[test]
fn allocator_device_local_capacity_uses_whole_device_memory() {
    let mut props = vk::PhysicalDeviceMemoryProperties::default();
    props.memory_heap_count = 2;
    props.memory_heaps[0] = vk::MemoryHeap {
        size: 4 * 1024 * 1024 * 1024,
        flags: vk::MemoryHeapFlags::DEVICE_LOCAL,
    };
    props.memory_heaps[1] = vk::MemoryHeap {
        size: 512 * 1024 * 1024,
        flags: vk::MemoryHeapFlags::empty(),
    };

    let allocator = GpuAllocator::new(props);

    assert_eq!(allocator.device_local_memory_bytes, 4 * 1024 * 1024 * 1024);
}

#[test]
fn allocator_block_size_pressure_ignores_transient_os_budget_dips() {
    let mut props = vk::PhysicalDeviceMemoryProperties::default();
    props.memory_heap_count = 1;
    props.memory_heaps[0] = vk::MemoryHeap {
        size: 4 * 1024 * 1024 * 1024,
        flags: vk::MemoryHeapFlags::DEVICE_LOCAL,
    };

    let mut allocator = GpuAllocator::new(props);
    allocator.device_local_budget = 128 * 1024 * 1024;
    let mut pool = TypePool::new(0, false, vk::MemoryAllocateFlags::empty());
    pool.blocks.push(Block::new(
        0,
        vk::DeviceMemory::default(),
        512 * 1024 * 1024,
        None,
    ));
    allocator.pools.push(pool);

    assert_eq!(
        allocator.device_local_new_block_size(),
        DEVICE_LOCAL_BLOCK_SIZE
    );
}
