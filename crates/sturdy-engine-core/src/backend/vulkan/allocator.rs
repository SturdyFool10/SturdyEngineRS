use std::collections::BTreeMap;

use ash::{Device, vk};

use crate::{Error, Result};

// Block sizes for new VkDeviceMemory allocations.
const DEVICE_LOCAL_BLOCK_SIZE: u64 = 256 * 1024 * 1024; // 256 MiB
const HOST_VISIBLE_BLOCK_SIZE: u64 = 64 * 1024 * 1024; // 64 MiB

pub struct Allocation {
    pub memory: vk::DeviceMemory,
    pub offset: u64,
    pub size: u64,
    pub mapped_ptr: Option<*mut u8>,
    memory_type: u32,
    block_id: u64,
}

unsafe impl Send for Allocation {}
unsafe impl Sync for Allocation {}

struct Block {
    id: u64,
    memory: vk::DeviceMemory,
    capacity: u64,
    /// free ranges: key = offset, value = size
    free: BTreeMap<u64, u64>,
    mapped_ptr: Option<*mut u8>,
}

impl Block {
    fn new(id: u64, memory: vk::DeviceMemory, capacity: u64, mapped_ptr: Option<*mut u8>) -> Self {
        let mut free = BTreeMap::new();
        free.insert(0, capacity);
        Self {
            id,
            memory,
            capacity,
            free,
            mapped_ptr,
        }
    }

    fn allocate(&mut self, size: u64, alignment: u64) -> Option<u64> {
        // Walk free ranges to find the first fit with proper alignment.
        let mut chosen = None;
        for (&offset, &free_size) in &self.free {
            let aligned = align_up(offset, alignment);
            let waste = aligned - offset;
            if free_size >= size + waste {
                chosen = Some((offset, aligned, free_size));
                break;
            }
        }
        let (raw_offset, aligned_offset, free_size) = chosen?;
        self.free.remove(&raw_offset);
        // Re-insert leading fragment before the aligned start.
        if aligned_offset > raw_offset {
            self.free.insert(raw_offset, aligned_offset - raw_offset);
        }
        // Re-insert trailing fragment after allocation.
        let end = aligned_offset + size;
        let free_end = raw_offset + free_size;
        if end < free_end {
            self.free.insert(end, free_end - end);
        }
        Some(aligned_offset)
    }

    fn free(&mut self, offset: u64, size: u64) {
        // Merge with adjacent free ranges.
        let mut start = offset;
        let mut end = offset + size;

        // Check if the range immediately before us is free and adjacent.
        if let Some((&prev_off, &prev_size)) = self.free.range(..start).next_back() {
            if prev_off + prev_size == start {
                self.free.remove(&prev_off);
                start = prev_off;
            }
        }
        // Check if the range immediately after us is free and adjacent.
        if let Some((&next_off, &next_size)) = self.free.range(end..).next() {
            if next_off == end {
                self.free.remove(&next_off);
                end = next_off + next_size;
            }
        }
        self.free.insert(start, end - start);
    }

    fn is_empty(&self) -> bool {
        self.free.len() == 1 && self.free.get(&0).copied() == Some(self.capacity)
    }
}

struct TypePool {
    memory_type: u32,
    host_visible: bool,
    blocks: Vec<Block>,
    next_block_id: u64,
}

impl TypePool {
    fn new(memory_type: u32, host_visible: bool) -> Self {
        Self {
            memory_type,
            host_visible,
            blocks: Vec::new(),
            next_block_id: 0,
        }
    }

    fn alloc(&mut self, device: &Device, size: u64, alignment: u64) -> Result<Allocation> {
        // Try existing blocks first.
        for block in &mut self.blocks {
            if let Some(offset) = block.allocate(size, alignment) {
                let mapped_ptr = block
                    .mapped_ptr
                    .map(|base| unsafe { base.add(offset as usize) });
                return Ok(Allocation {
                    memory: block.memory,
                    offset,
                    size,
                    mapped_ptr,
                    memory_type: self.memory_type,
                    block_id: block.id,
                });
            }
        }
        // No existing block had room — create a new one.
        let block_capacity = if self.host_visible {
            HOST_VISIBLE_BLOCK_SIZE.max(size)
        } else {
            DEVICE_LOCAL_BLOCK_SIZE.max(size)
        };
        let alloc_info = vk::MemoryAllocateInfo::default()
            .allocation_size(block_capacity)
            .memory_type_index(self.memory_type);
        let memory = unsafe {
            device
                .allocate_memory(&alloc_info, None)
                .map_err(|e| Error::Backend(format!("vkAllocateMemory failed: {e:?}")))?
        };
        let mapped_ptr = if self.host_visible {
            let ptr = unsafe {
                device
                    .map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
                    .map_err(|e| {
                        device.free_memory(memory, None);
                        Error::Backend(format!("vkMapMemory failed: {e:?}"))
                    })?
            };
            Some(ptr as *mut u8)
        } else {
            None
        };

        let id = self.next_block_id;
        self.next_block_id += 1;
        let mut block = Block::new(id, memory, block_capacity, mapped_ptr);
        let offset = block.allocate(size, alignment).ok_or_else(|| {
            Error::Backend(format!(
                "Vulkan allocator fresh block did not fit request: size={size} alignment={alignment} block_capacity={block_capacity} memory_type={}",
                self.memory_type
            ))
        })?;
        self.blocks.push(block);
        let mapped_ptr = mapped_ptr.map(|base| unsafe { base.add(offset as usize) });
        Ok(Allocation {
            memory,
            offset,
            size,
            mapped_ptr,
            memory_type: self.memory_type,
            block_id: id,
        })
    }

    fn dealloc(&mut self, device: &Device, alloc: Allocation) -> Result<()> {
        let Some(block) = self.blocks.iter_mut().find(|b| b.id == alloc.block_id) else {
            return Err(Error::Backend(format!(
                "Vulkan allocator invalid allocation handle: block_id={} not found in memory_type={} pool",
                alloc.block_id, self.memory_type
            )));
        };
        block.free(alloc.offset, alloc.size);
        // If the block is now fully free and we have more than one block, release it.
        if block.is_empty() && self.blocks.len() > 1 {
            if let Some(idx) = self.blocks.iter().position(|b| b.id == alloc.block_id) {
                let b = self.blocks.swap_remove(idx);
                unsafe {
                    if b.mapped_ptr.is_some() {
                        device.unmap_memory(b.memory);
                    }
                    device.free_memory(b.memory, None);
                }
            } else {
                return Err(Error::Backend(format!(
                    "Vulkan allocator corruption: block_id={} disappeared from memory_type={} pool during deallocation",
                    alloc.block_id, self.memory_type
                )));
            }
        }
        Ok(())
    }

    fn destroy_all(&mut self, device: &Device) {
        for block in self.blocks.drain(..) {
            unsafe {
                if block.mapped_ptr.is_some() {
                    device.unmap_memory(block.memory);
                }
                device.free_memory(block.memory, None);
            }
        }
    }
}

pub struct GpuAllocator {
    memory_properties: vk::PhysicalDeviceMemoryProperties,
    pools: Vec<TypePool>,
}

// Safety: GpuAllocator is only accessed through Mutex<ResourceRegistry> in VulkanBackend.
// The raw mapped pointers are valid for the lifetime of the allocator and only accessed
// while the mutex is held.
unsafe impl Send for GpuAllocator {}
unsafe impl Sync for GpuAllocator {}

impl GpuAllocator {
    pub fn new(memory_properties: vk::PhysicalDeviceMemoryProperties) -> Self {
        Self {
            memory_properties,
            pools: Vec::new(),
        }
    }

    pub fn alloc(
        &mut self,
        device: &Device,
        requirements: vk::MemoryRequirements,
        required_flags: vk::MemoryPropertyFlags,
    ) -> Result<Allocation> {
        let memory_type = self.find_memory_type(requirements.memory_type_bits, required_flags)?;
        let host_visible = self.memory_properties.memory_types[memory_type as usize]
            .property_flags
            .contains(vk::MemoryPropertyFlags::HOST_VISIBLE);

        let pool_index = match self.pools.iter().position(|p| p.memory_type == memory_type) {
            Some(index) => index,
            None => {
                self.pools.push(TypePool::new(memory_type, host_visible));
                self.pools.len() - 1
            }
        };
        let pool = &mut self.pools[pool_index];
        pool.alloc(device, requirements.size, requirements.alignment)
    }

    pub fn dealloc(&mut self, device: &Device, alloc: Allocation) -> Result<()> {
        let Some(pool) = self
            .pools
            .iter_mut()
            .find(|p| p.memory_type == alloc.memory_type)
        else {
            return Err(Error::Backend(format!(
                "Vulkan allocator invalid allocation handle: no pool for memory_type={}",
                alloc.memory_type
            )));
        };
        pool.dealloc(device, alloc)
    }

    pub fn destroy_all(&mut self, device: &Device) {
        for pool in &mut self.pools {
            pool.destroy_all(device);
        }
        self.pools.clear();
    }

    pub fn find_memory_type(
        &self,
        type_bits: u32,
        required: vk::MemoryPropertyFlags,
    ) -> Result<u32> {
        for index in 0..self.memory_properties.memory_type_count {
            let supported = (type_bits & (1 << index)) != 0;
            let mt = self.memory_properties.memory_types[index as usize];
            if supported && mt.property_flags.contains(required) {
                return Ok(index);
            }
        }
        Err(Error::Unsupported("no compatible Vulkan memory type found"))
    }
}

fn align_up(offset: u64, alignment: u64) -> u64 {
    if alignment == 0 {
        return offset;
    }
    (offset + alignment - 1) & !(alignment - 1)
}
