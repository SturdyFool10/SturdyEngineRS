use std::collections::BTreeMap;

use ash::vk::TaggedStructure;
use ash::{Device, vk};

use crate::{Error, Result};

use super::error_context::VkResultExt;

// Block sizes for new VkDeviceMemory allocations.
const DEVICE_LOCAL_BLOCK_SIZE: u64 = 256 * 1024 * 1024; // 256 MiB
/// Reduced retry block size used only after a preferred device-local block allocation fails.
const DEVICE_LOCAL_BLOCK_SIZE_REDUCED: u64 = 32 * 1024 * 1024; // 32 MiB
const HOST_VISIBLE_BLOCK_SIZE: u64 = 64 * 1024 * 1024; // 64 MiB

/// Sentinel block ID indicating a dedicated `VkDeviceMemory` allocation (not sub-allocated).
const DEDICATED_BLOCK_ID: u64 = u64::MAX;

pub struct Allocation {
    pub memory: vk::DeviceMemory,
    pub offset: u64,
    pub size: u64,
    pub mapped_ptr: Option<*mut u8>,
    memory_type: u32,
    block_id: u64,
}

impl Allocation {
    /// Create a sentinel allocation for a resource with its own dedicated `VkDeviceMemory`.
    ///
    /// The memory is already bound — the allocator just tracks it for dealloc.
    pub fn dedicated(memory: vk::DeviceMemory, size: u64, memory_type: u32) -> Self {
        Self {
            memory,
            offset: 0,
            size,
            mapped_ptr: None,
            memory_type,
            block_id: DEDICATED_BLOCK_ID,
        }
    }

    /// Returns `true` when this allocation owns its own dedicated `VkDeviceMemory`.
    pub fn is_dedicated(&self) -> bool {
        self.block_id == DEDICATED_BLOCK_ID
    }
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

    fn allocate(&mut self, size: u64, alignment: u64, memory_type: u32) -> Result<Option<u64>> {
        validate_allocation_request(size, alignment, self.capacity, memory_type)?;

        // Walk free ranges to find the first fit with proper alignment.
        let mut chosen = None;
        for (&offset, &free_size) in &self.free {
            let aligned = align_up_checked(offset, alignment, memory_type)?;
            let waste = aligned - offset;
            let required = size.checked_add(waste).ok_or_else(|| {
                Error::Backend(format!(
                    "Vulkan allocator request overflow: size={size} alignment={alignment} offset={offset} memory_type={memory_type}"
                ))
            })?;
            if free_size >= required {
                chosen = Some((offset, aligned, free_size));
                break;
            }
        }
        let Some((raw_offset, aligned_offset, free_size)) = chosen else {
            return Ok(None);
        };
        self.free.remove(&raw_offset);
        // Re-insert leading fragment before the aligned start.
        if aligned_offset > raw_offset {
            self.free.insert(raw_offset, aligned_offset - raw_offset);
        }
        // Re-insert trailing fragment after allocation.
        let end = aligned_offset.checked_add(size).ok_or_else(|| {
            Error::Backend(format!(
                "Vulkan allocator range overflow: offset={aligned_offset} size={size} memory_type={memory_type}"
            ))
        })?;
        let free_end = raw_offset.checked_add(free_size).ok_or_else(|| {
            Error::Backend(format!(
                "Vulkan allocator free range overflow: offset={raw_offset} size={free_size} memory_type={memory_type}"
            ))
        })?;
        if end < free_end {
            self.free.insert(end, free_end - end);
        }
        Ok(Some(aligned_offset))
    }

    fn free(&mut self, offset: u64, size: u64, memory_type: u32) -> Result<()> {
        debug_assert!(size > 0, "Vulkan allocator cannot free a zero-sized range");
        let mut end = offset.checked_add(size).ok_or_else(|| {
            Error::Backend(format!(
                "Vulkan allocator deallocation range overflow: offset={offset} size={size} memory_type={memory_type}"
            ))
        })?;
        debug_assert!(
            end <= self.capacity,
            "Vulkan allocator deallocation range exceeds block capacity"
        );
        if size == 0 || end > self.capacity {
            return Err(Error::Backend(format!(
                "Vulkan allocator invalid deallocation range: offset={offset} size={size} block_capacity={} memory_type={memory_type}",
                self.capacity
            )));
        }

        // Merge with adjacent free ranges.
        let mut start = offset;

        // Check if the range immediately before us is free and adjacent.
        if let Some((&prev_off, &prev_size)) = self.free.range(..start).next_back() {
            let prev_end = prev_off.checked_add(prev_size).ok_or_else(|| {
                Error::Backend(format!(
                    "Vulkan allocator free-list range overflow: offset={prev_off} size={prev_size} memory_type={memory_type}"
                ))
            })?;
            if prev_end > start {
                debug_assert!(false, "Vulkan allocator deallocation overlaps a free range");
                return Err(Error::Backend(format!(
                    "Vulkan allocator invalid deallocation range overlaps previous free range: offset={offset} size={size} memory_type={memory_type}"
                )));
            }
            if prev_end == start {
                self.free.remove(&prev_off);
                start = prev_off;
            }
        }
        // Check if the range immediately after us is free and adjacent.
        if let Some((&next_off, &next_size)) = self.free.range(end..).next() {
            if next_off == end {
                self.free.remove(&next_off);
                end = next_off.checked_add(next_size).ok_or_else(|| {
                    Error::Backend(format!(
                        "Vulkan allocator free-list range overflow: offset={next_off} size={next_size} memory_type={memory_type}"
                    ))
                })?;
            }
        }
        self.free.insert(start, end - start);
        Ok(())
    }

    fn is_empty(&self) -> bool {
        self.free.len() == 1 && self.free.get(&0).copied() == Some(self.capacity)
    }
}

/// Per-pool memory usage snapshot.
#[derive(Clone, Debug, Default)]
pub struct PoolStats {
    /// Total bytes in all live VkDeviceMemory blocks for this pool.
    pub capacity_bytes: u64,
    /// Bytes currently sub-allocated (capacity minus free list totals).
    pub used_bytes: u64,
    /// Number of live VkDeviceMemory blocks.
    pub block_count: u32,
    /// Whether the pool is host-visible.
    pub host_visible: bool,
}

struct TypePool {
    memory_type: u32,
    host_visible: bool,
    allocate_flags: vk::MemoryAllocateFlags,
    blocks: Vec<Block>,
    next_block_id: u64,
}

impl TypePool {
    fn new(memory_type: u32, host_visible: bool, allocate_flags: vk::MemoryAllocateFlags) -> Self {
        Self {
            memory_type,
            host_visible,
            allocate_flags,
            blocks: Vec::new(),
            next_block_id: 0,
        }
    }

    fn alloc(
        &mut self,
        device: &Device,
        size: u64,
        alignment: u64,
        priority: Option<f32>,
        new_block_size: u64,
    ) -> Result<Allocation> {
        // Try existing blocks first.
        for block in &mut self.blocks {
            if let Some(offset) = block.allocate(size, alignment, self.memory_type)? {
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
        // No existing block had room — create a new one. Prefer a large block so the
        // allocator can keep growing under load, but retry with a request-sized block
        // when the driver rejects the preferred block under memory pressure.
        let preferred_capacity = new_block_size.max(size);
        let retry_capacity = size
            .max(DEVICE_LOCAL_BLOCK_SIZE_REDUCED)
            .min(preferred_capacity);
        let (memory, block_capacity) = match allocate_memory_block(
            device,
            self.memory_type,
            preferred_capacity,
            priority,
            self.allocate_flags,
        ) {
            Ok(memory) => (memory, preferred_capacity),
            Err(preferred_error) if retry_capacity < preferred_capacity => {
                #[cfg(debug_assertions)]
                tracing::error!(
                    "vkAllocateMemory failed for preferred {} MiB block; retrying {} MiB: {preferred_error}",
                    preferred_capacity / (1024 * 1024),
                    retry_capacity / (1024 * 1024),
                );
                #[cfg(not(debug_assertions))]
                let _ = &preferred_error;
                (
                    allocate_memory_block(
                        device,
                        self.memory_type,
                        retry_capacity,
                        priority,
                        self.allocate_flags,
                    )?,
                    retry_capacity,
                )
            }
            Err(error) => return Err(error),
        };
        let mapped_ptr = if self.host_visible {
            let ptr = unsafe {
                device
                    .map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
                    .trace_vk_with("vkMapMemory", || {
                        format!(
                            "block_capacity={block_capacity} memory_type={} host_visible={}",
                            self.memory_type, self.host_visible
                        )
                    })
                    .inspect_err(|_| device.free_memory(memory, None))?
            };
            Some(ptr as *mut u8)
        } else {
            None
        };

        let id = self.next_block_id;
        self.next_block_id += 1;
        let mut block = Block::new(id, memory, block_capacity, mapped_ptr);
        let offset = match block.allocate(size, alignment, self.memory_type)? {
            Some(offset) => offset,
            None => {
                return Err(Error::Backend(format!(
                    "Vulkan allocator fresh block did not fit request: size={size} alignment={alignment} block_capacity={block_capacity} memory_type={}",
                    self.memory_type
                )));
            }
        };
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
        block.free(alloc.offset, alloc.size, self.memory_type)?;
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

    fn stats(&self) -> PoolStats {
        let capacity_bytes: u64 = self.blocks.iter().map(|b| b.capacity).sum();
        let free_bytes: u64 = self.blocks.iter().flat_map(|b| b.free.values()).sum();
        PoolStats {
            capacity_bytes,
            used_bytes: capacity_bytes.saturating_sub(free_bytes),
            block_count: self.blocks.len() as u32,
            host_visible: self.host_visible,
        }
    }
}

/// Aggregate memory usage snapshot across all sub-allocator pools.
#[derive(Clone, Debug, Default)]
pub struct AllocatorStats {
    pub device_local_used_bytes: u64,
    pub device_local_capacity_bytes: u64,
    pub host_visible_used_bytes: u64,
    pub host_visible_capacity_bytes: u64,
    pub block_count: u32,
}

pub struct GpuAllocator {
    memory_properties: vk::PhysicalDeviceMemoryProperties,
    pools: Vec<TypePool>,
    /// `VK_EXT_memory_priority` is available; chained into every allocation.
    pub memory_priority_enabled: bool,
    /// Total OS-reported device-local memory budget in bytes from `VK_EXT_memory_budget`;
    /// 0 means unavailable. Used for diagnostics/reporting, not as a hard block-size cap.
    pub device_local_budget: u64,
    /// Total physical device-local heap size in bytes. Allocator pressure decisions are
    /// relative to this whole-device capacity so a transient OS budget dip does not
    /// artificially prevent the allocator from growing.
    pub device_local_memory_bytes: u64,
}

// Safety: GpuAllocator is only accessed through Mutex<ResourceRegistry> in VulkanBackend.
// The raw mapped pointers are valid for the lifetime of the allocator and only accessed
// while the mutex is held.
unsafe impl Send for GpuAllocator {}
unsafe impl Sync for GpuAllocator {}

impl GpuAllocator {
    pub fn new(memory_properties: vk::PhysicalDeviceMemoryProperties) -> Self {
        let device_local_memory_bytes = memory_properties.memory_heaps
            [..memory_properties.memory_heap_count as usize]
            .iter()
            .filter(|heap| heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL))
            .map(|heap| heap.size)
            .sum();
        Self {
            memory_properties,
            pools: Vec::new(),
            memory_priority_enabled: false,
            device_local_budget: 0,
            device_local_memory_bytes,
        }
    }

    pub fn alloc(
        &mut self,
        device: &Device,
        requirements: vk::MemoryRequirements,
        required_flags: vk::MemoryPropertyFlags,
    ) -> Result<Allocation> {
        self.alloc_with_flags(
            device,
            requirements,
            required_flags,
            vk::MemoryAllocateFlags::empty(),
        )
    }

    pub fn alloc_with_flags(
        &mut self,
        device: &Device,
        requirements: vk::MemoryRequirements,
        required_flags: vk::MemoryPropertyFlags,
        allocate_flags: vk::MemoryAllocateFlags,
    ) -> Result<Allocation> {
        let memory_type = self.find_memory_type(requirements.memory_type_bits, required_flags)?;
        let host_visible = self.memory_properties.memory_types[memory_type as usize]
            .property_flags
            .contains(vk::MemoryPropertyFlags::HOST_VISIBLE);

        // GFX-1e: assign memory priority when the extension is available.
        // Device-local (GPU-only) memory gets 0.7 (scene geometry priority).
        // Host-visible (staging) memory gets 0.1 so it is evicted first under pressure.
        let priority = if self.memory_priority_enabled {
            Some(if host_visible { 0.1 } else { 0.7 })
        } else {
            None
        };

        // Compute block size before taking the mutable pool borrow to satisfy the borrow checker.
        let new_block_size = if host_visible {
            HOST_VISIBLE_BLOCK_SIZE
        } else {
            self.device_local_new_block_size()
        };
        let pool_index = match self
            .pools
            .iter()
            .position(|p| p.memory_type == memory_type && p.allocate_flags == allocate_flags)
        {
            Some(index) => index,
            None => {
                self.pools
                    .push(TypePool::new(memory_type, host_visible, allocate_flags));
                self.pools.len() - 1
            }
        };
        let pool = &mut self.pools[pool_index];
        pool.alloc(
            device,
            requirements.size,
            requirements.alignment,
            priority,
            new_block_size,
        )
    }

    /// Choose the preferred block size for a new device-local block.
    ///
    /// The allocator keeps requesting normal-sized blocks while below whole-device
    /// pressure. Near total physical device-local capacity, it asks for smaller blocks
    /// to reduce allocation failures. If the driver still rejects a preferred block,
    /// `TypePool::alloc` retries with a request-sized block.
    fn device_local_new_block_size(&self) -> u64 {
        if self.device_local_memory_bytes > 0 {
            let current = self.device_local_capacity_bytes();
            if current >= self.device_local_memory_bytes * 9 / 10 {
                #[cfg(debug_assertions)]
                tracing::warn!(
                    "VRAM pressure: allocator capacity is {} / {} MiB; preferring {} MiB blocks",
                    current / (1024 * 1024),
                    self.device_local_memory_bytes / (1024 * 1024),
                    DEVICE_LOCAL_BLOCK_SIZE_REDUCED / (1024 * 1024)
                );
                return DEVICE_LOCAL_BLOCK_SIZE_REDUCED;
            }
        }
        DEVICE_LOCAL_BLOCK_SIZE
    }

    fn device_local_capacity_bytes(&self) -> u64 {
        self.pools
            .iter()
            .filter(|p| !p.host_visible)
            .map(|p| p.stats().capacity_bytes)
            .sum()
    }

    pub fn dealloc(&mut self, device: &Device, alloc: Allocation) -> Result<()> {
        // GFX-1e: dedicated allocations own their own VkDeviceMemory — free it directly.
        if alloc.is_dedicated() {
            unsafe { device.free_memory(alloc.memory, None) };
            return Ok(());
        }
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

    /// Aggregate memory usage across all pools.
    pub fn stats(&self) -> AllocatorStats {
        let mut stats = AllocatorStats::default();
        for pool in &self.pools {
            let ps = pool.stats();
            if ps.host_visible {
                stats.host_visible_capacity_bytes += ps.capacity_bytes;
                stats.host_visible_used_bytes += ps.used_bytes;
            } else {
                stats.device_local_capacity_bytes += ps.capacity_bytes;
                stats.device_local_used_bytes += ps.used_bytes;
            }
            stats.block_count += ps.block_count;
        }
        stats
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

fn allocate_memory_block(
    device: &Device,
    memory_type: u32,
    capacity: u64,
    priority: Option<f32>,
    allocate_flags: vk::MemoryAllocateFlags,
) -> Result<vk::DeviceMemory> {
    let mut alloc_info = vk::MemoryAllocateInfo::default()
        .allocation_size(capacity)
        .memory_type_index(memory_type);
    let mut flags_info;
    if !allocate_flags.is_empty() {
        flags_info = vk::MemoryAllocateFlagsInfo::default().flags(allocate_flags);
        alloc_info = alloc_info.push(&mut flags_info);
    }
    let mut priority_info;
    if let Some(p) = priority {
        priority_info = vk::MemoryPriorityAllocateInfoEXT::default().priority(p);
        alloc_info = alloc_info.push(&mut priority_info);
    }
    unsafe {
        device
            .allocate_memory(&alloc_info, None)
            .trace_vk_with("vkAllocateMemory", || {
                format!(
                    "capacity={capacity} memory_type={memory_type} priority={priority:?} flags=0x{:x}",
                    allocate_flags.as_raw()
                )
            })
    }
}

fn validate_allocation_request(
    size: u64,
    alignment: u64,
    block_capacity: u64,
    memory_type: u32,
) -> Result<()> {
    debug_assert!(size > 0, "Vulkan allocator cannot allocate zero bytes");
    debug_assert!(
        alignment == 0 || alignment.is_power_of_two(),
        "Vulkan memory alignment must be zero or a power of two"
    );
    if size == 0 {
        return Err(Error::Backend(format!(
            "Vulkan allocator invalid zero-sized allocation request: alignment={alignment} block_capacity={block_capacity} memory_type={memory_type}"
        )));
    }
    if alignment != 0 && !alignment.is_power_of_two() {
        return Err(Error::Backend(format!(
            "Vulkan allocator invalid alignment: size={size} alignment={alignment} block_capacity={block_capacity} memory_type={memory_type}"
        )));
    }
    Ok(())
}

fn align_up_checked(offset: u64, alignment: u64, memory_type: u32) -> Result<u64> {
    if alignment == 0 {
        return Ok(offset);
    }
    debug_assert!(
        alignment.is_power_of_two(),
        "Vulkan memory alignment must be a power of two"
    );
    if !alignment.is_power_of_two() {
        return Err(Error::Backend(format!(
            "Vulkan allocator invalid alignment: offset={offset} alignment={alignment} memory_type={memory_type}"
        )));
    }
    offset
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
        .ok_or_else(|| {
            Error::Backend(format!(
                "Vulkan allocator alignment overflow: offset={offset} alignment={alignment} memory_type={memory_type}"
            ))
        })
}

#[cfg(test)]
#[path = "allocator_tests.rs"]
mod tests;
