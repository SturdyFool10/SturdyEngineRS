// Tests extracted from crates/sturdy-engine-core/src/backend/vulkan/device.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn global_queue_priority_prefers_graphics_for_unified_family() {
    let families = QueueFamilyMap::unified(2);

    assert!(queue_global_priority(families, 2) == vk::QueueGlobalPriorityKHR::HIGH);
}

#[test]
fn global_queue_priority_assigns_split_queue_tiers() {
    let families = QueueFamilyMap {
        graphics: 0,
        compute: 1,
        transfer: 2,
        video_decode: 0,
        video_encode: 0,
        async_compute: 1,
        dma: 2,
    };

    assert!(
        queue_global_priority(families, families.graphics) == vk::QueueGlobalPriorityKHR::HIGH
    );
    assert!(
        queue_global_priority(families, families.compute) == vk::QueueGlobalPriorityKHR::MEDIUM
    );
    assert!(
        queue_global_priority(families, families.transfer) == vk::QueueGlobalPriorityKHR::LOW
    );
}
