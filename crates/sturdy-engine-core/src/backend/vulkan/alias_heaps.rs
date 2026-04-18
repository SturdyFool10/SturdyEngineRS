use std::collections::HashMap;

use ash::{Device, vk};

use crate::{Error, Result};

/// A single shared `VkDeviceMemory` backing one alias slot.
struct AliasHeap {
    memory: vk::DeviceMemory,
    size: u64,
    memory_type: u32,
}

/// Persistent registry of alias heap memories, one per (slot_id, memory_type) pair.
///
/// Heaps grow on demand (reallocation with device-idle) but are never returned
/// to the OS until the device is destroyed.
#[derive(Default)]
pub struct AliasHeapRegistry {
    heaps: HashMap<u32, AliasHeap>,
}

impl AliasHeapRegistry {
    /// Return the `VkDeviceMemory` for `slot_id`, allocating or growing it as needed.
    pub fn slot_memory(
        &mut self,
        device: &Device,
        slot_id: u32,
        required_size: u64,
        memory_type: u32,
    ) -> Result<vk::DeviceMemory> {
        if let Some(heap) = self.heaps.get(&slot_id) {
            if heap.memory_type == memory_type && heap.size >= required_size {
                return Ok(heap.memory);
            }
            // Need to reallocate (different type or grown).  Free old first.
            unsafe { device.free_memory(heap.memory, None) };
            self.heaps.remove(&slot_id);
        }
        let memory = alloc_raw(device, required_size, memory_type)?;
        self.heaps.insert(
            slot_id,
            AliasHeap {
                memory,
                size: required_size,
                memory_type,
            },
        );
        Ok(memory)
    }

    pub fn destroy_all(&mut self, device: &Device) {
        for (_, heap) in self.heaps.drain() {
            unsafe { device.free_memory(heap.memory, None) };
        }
    }
}

fn alloc_raw(device: &Device, size: u64, memory_type: u32) -> Result<vk::DeviceMemory> {
    let info = vk::MemoryAllocateInfo::default()
        .allocation_size(size)
        .memory_type_index(memory_type);
    unsafe {
        device
            .allocate_memory(&info, None)
            .map_err(|e| Error::Backend(format!("vkAllocateMemory (alias heap) failed: {e:?}")))
    }
}
