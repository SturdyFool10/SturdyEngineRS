// Per-frame transient buffer pool (Track 11a).
//
// A single persistent HOST_VISIBLE + HOST_COHERENT VkBuffer sub-allocated with a
// bump pointer. The pointer resets to 0 at frame start (after the previous frame's
// fence fires). Eliminates per-frame vkAllocateMemory + vkCreateBuffer calls for
// uniform uploads and small staging transfers.
//
// # Usage
//
// ```
// let alloc = pool.alloc(size, alignment)?;
// unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), alloc.mapped_ptr, size) };
// // Use alloc.buffer at alloc.offset in a descriptor / copy command.
// ```
//
// # Thread safety
//
// `BufferPool` is NOT internally synchronized; access must be serialised by the
// caller (typically via the CommandContext mutex).

use ash::{Device, vk};

use crate::{Error, Result};

/// One bump-allocated region from the pool.
#[derive(Copy, Clone)]
pub struct PoolAllocation {
    /// The pool's persistent buffer.
    pub buffer: vk::Buffer,
    /// Byte offset within `buffer` where this allocation begins.
    pub offset: u64,
    /// Allocation size in bytes.
    pub size: u64,
    /// CPU-mapped pointer to the start of this allocation.
    pub mapped_ptr: *mut u8,
}

// SAFETY: the mapped pointer is valid for the lifetime of the pool and only accessed
// while the pool's owning mutex is held.
unsafe impl Send for PoolAllocation {}
unsafe impl Sync for PoolAllocation {}

// SAFETY: BufferPool is only accessed through the CommandContext mutex (part of VulkanBackend).
// The raw mapped pointer is valid for the pool's lifetime and not shared across threads.
unsafe impl Send for BufferPool {}
unsafe impl Sync for BufferPool {}

pub struct BufferPool {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    mapped_ptr: *mut u8,
    capacity: u64,
    /// Current write cursor; bumped on alloc, reset to 0 by `reset()`.
    cursor: u64,
    device: Device,
}

impl BufferPool {
    /// Create a pool with the given capacity in HOST_VISIBLE + HOST_COHERENT memory.
    pub fn create(
        device: &Device,
        memory_properties: vk::PhysicalDeviceMemoryProperties,
        capacity: u64,
    ) -> Result<Self> {
        let buf_info = vk::BufferCreateInfo::default()
            .size(capacity)
            .usage(
                vk::BufferUsageFlags::UNIFORM_BUFFER
                    | vk::BufferUsageFlags::STORAGE_BUFFER
                    | vk::BufferUsageFlags::TRANSFER_SRC
                    | vk::BufferUsageFlags::INDEX_BUFFER
                    | vk::BufferUsageFlags::VERTEX_BUFFER,
            )
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let buffer = unsafe {
            device
                .create_buffer(&buf_info, None)
                .map_err(|e| Error::Backend(format!("BufferPool buffer creation failed: {e:?}")))?
        };

        let req = unsafe { device.get_buffer_memory_requirements(buffer) };
        let memory_type = (0..memory_properties.memory_type_count)
            .find(|&i| {
                (req.memory_type_bits & (1 << i)) != 0
                    && memory_properties.memory_types[i as usize]
                        .property_flags
                        .contains(
                            vk::MemoryPropertyFlags::HOST_VISIBLE
                                | vk::MemoryPropertyFlags::HOST_COHERENT,
                        )
            })
            .ok_or_else(|| {
                unsafe { device.destroy_buffer(buffer, None) };
                Error::Backend("no HOST_VISIBLE memory type for BufferPool".into())
            })?;

        let alloc_info = vk::MemoryAllocateInfo::default()
            .allocation_size(req.size)
            .memory_type_index(memory_type);
        let memory = unsafe {
            device.allocate_memory(&alloc_info, None).map_err(|e| {
                device.destroy_buffer(buffer, None);
                Error::Backend(format!("BufferPool memory allocation failed: {e:?}"))
            })?
        };
        unsafe {
            device.bind_buffer_memory(buffer, memory, 0).map_err(|e| {
                device.free_memory(memory, None);
                device.destroy_buffer(buffer, None);
                Error::Backend(format!("BufferPool bind_buffer_memory failed: {e:?}"))
            })?;
        }
        let mapped_ptr = unsafe {
            device
                .map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
                .map_err(|e| {
                    device.free_memory(memory, None);
                    device.destroy_buffer(buffer, None);
                    Error::Backend(format!("BufferPool map_memory failed: {e:?}"))
                })? as *mut u8
        };

        Ok(Self {
            buffer,
            memory,
            mapped_ptr,
            capacity,
            cursor: 0,
            device: device.clone(),
        })
    }

    /// Bump-allocate `size` bytes with `alignment` bytes alignment.
    ///
    /// Returns `None` when the pool is full; callers should fall back to a dedicated allocation.
    pub fn alloc(&mut self, size: u64, alignment: u64) -> Option<PoolAllocation> {
        debug_assert!(alignment == 0 || alignment.is_power_of_two());
        let align = alignment.max(1);
        let offset = (self.cursor + align - 1) & !(align - 1);
        if offset + size > self.capacity {
            return None;
        }
        self.cursor = offset + size;
        let ptr = unsafe { self.mapped_ptr.add(offset as usize) };
        Some(PoolAllocation {
            buffer: self.buffer,
            offset,
            size,
            mapped_ptr: ptr,
        })
    }

    /// Reset the allocation cursor. Call at the start of each frame, after the
    /// previous frame's fence has fired (guaranteeing all GPU reads are complete).
    pub fn reset(&mut self) {
        self.cursor = 0;
    }

    /// Total pool capacity in bytes.
    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    /// Bytes consumed since the last `reset()`.
    pub fn used(&self) -> u64 {
        self.cursor
    }

    pub fn destroy(&self) {
        unsafe {
            self.device.unmap_memory(self.memory);
            self.device.destroy_buffer(self.buffer, None);
            self.device.free_memory(self.memory, None);
        }
    }
}
