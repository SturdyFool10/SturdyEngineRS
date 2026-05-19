// Bindless descriptor heap (Track 8a).
//
// A single, persistent descriptor set that holds ALL textures, samplers, and
// storage buffers used during a frame. Each resource is assigned a stable u32
// index at registration time. Shaders sample via `g_bindless_textures[index]`
// instead of per-draw descriptor set updates.
//
// # Vulkan requirements
//
// The heap requires these descriptor_indexing features (detected and enabled by

use ash::vk::TaggedStructure;
// the device creation code):
//   - runtimeDescriptorArray         — unbounded [] arrays in shaders
//   - descriptorBindingPartiallyBound — allocated but unwritten slots are ok
//   - descriptorBindingUpdateAfterBind — update the set while the GPU is using it
//
// All three are checked via `Caps::supports_bindless` before creating the heap.
//
// # Index allocation
//
// Indices are allocated with an AtomicU32 fetch-add. They are NEVER freed —
// this is intentional for the initial implementation. The capacity constants
// are generous enough that games won't exhaust them at runtime. A free-list
// can be added later without changing the API.
//
// # Descriptor bindings (set 0)
//
//   binding 0 — SAMPLER                  (up to BINDLESS_SAMPLER_CAPACITY)
//   binding 1 — SAMPLED_IMAGE            (up to BINDLESS_SAMPLED_IMAGE_CAPACITY)
//   binding 2 — STORAGE_IMAGE            (up to BINDLESS_STORAGE_IMAGE_CAPACITY)
//   binding 3 — STORAGE_BUFFER           (up to BINDLESS_STORAGE_BUFFER_CAPACITY)
//
// The corresponding Slang declarations live in `shaders/bindless.slang`.

use std::sync::atomic::{AtomicU32, Ordering};

use ash::{Device, vk};

use crate::{Error, Result};

// ── Capacities ────────────────────────────────────────────────────────────────

/// Maximum number of simultaneously-registered samplers.
pub const BINDLESS_SAMPLER_CAPACITY: u32 = 128;
/// Maximum number of simultaneously-registered sampled (read-only) images.
pub const BINDLESS_SAMPLED_IMAGE_CAPACITY: u32 = 16_384;
/// Maximum number of simultaneously-registered storage (read-write) images.
pub const BINDLESS_STORAGE_IMAGE_CAPACITY: u32 = 1_024;
/// Maximum number of simultaneously-registered storage buffers.
pub const BINDLESS_STORAGE_BUFFER_CAPACITY: u32 = 4_096;

// ── Binding slot assignments ──────────────────────────────────────────────────

pub const BINDLESS_SAMPLER_BINDING: u32 = 0;
pub const BINDLESS_SAMPLED_IMAGE_BINDING: u32 = 1;
pub const BINDLESS_STORAGE_IMAGE_BINDING: u32 = 2;
pub const BINDLESS_STORAGE_BUFFER_BINDING: u32 = 3;

// ── BindlessVkInfo ────────────────────────────────────────────────────────────

/// Vulkan-level handles needed to bind the bindless set before draw calls.
///
/// Returned by `VulkanBackend::bindless_vk_info()`. Used by command recording
/// to bind the bindless descriptor set at set index 0 for pipelines that
/// include `bindless.slang`.
#[derive(Copy, Clone)]
pub struct BindlessVkInfo {
    pub set: vk::DescriptorSet,
    pub set_layout: vk::DescriptorSetLayout,
}

// ── BindlessHeap ──────────────────────────────────────────────────────────────

/// The global bindless descriptor heap.
///
/// One instance lives inside `VulkanBackend` when `supports_bindless` is true.
/// All Vulkan objects are destroyed in `Drop`.
///
// Logical two-heap model: resource descriptors (bindings 1-3) and sampler
// descriptors (binding 0) occupy separate ranges. When VK_EXT_descriptor_heap
// is available, these will map directly to a VkDescriptorHeapEXT.
pub struct BindlessHeap {
    /// The descriptor set layout for the bindless set (set 0).
    pub set_layout: vk::DescriptorSetLayout,
    /// The persistent pool backing the single bindless set.
    pool: vk::DescriptorPool,
    /// The one large descriptor set containing all bindless resources.
    pub set: vk::DescriptorSet,
    /// Next available index for each resource type.
    next_sampler: AtomicU32,
    next_sampled_image: AtomicU32,
    next_storage_image: AtomicU32,
    next_storage_buffer: AtomicU32,
    /// Copy of the device handle for Drop.
    device: Device,
}

impl BindlessHeap {
    /// Create the bindless descriptor heap.
    ///
    /// # Panics
    ///
    /// Panics (via `Error`) if descriptor_indexing features are not available.
    /// The caller must check `Caps::supports_bindless` before calling this.
    pub fn create(device: &Device) -> Result<Self> {
        let binding_flags = [
            // All four bindings use the same flags:
            //   - UPDATE_AFTER_BIND: write descriptors while a frame is in flight
            //   - PARTIALLY_BOUND:   unwritten slots don't trigger validation errors
            vk::DescriptorBindingFlags::UPDATE_AFTER_BIND
                | vk::DescriptorBindingFlags::PARTIALLY_BOUND,
            vk::DescriptorBindingFlags::UPDATE_AFTER_BIND
                | vk::DescriptorBindingFlags::PARTIALLY_BOUND,
            vk::DescriptorBindingFlags::UPDATE_AFTER_BIND
                | vk::DescriptorBindingFlags::PARTIALLY_BOUND,
            vk::DescriptorBindingFlags::UPDATE_AFTER_BIND
                | vk::DescriptorBindingFlags::PARTIALLY_BOUND,
        ];

        let mut binding_flags_info =
            vk::DescriptorSetLayoutBindingFlagsCreateInfo::default().binding_flags(&binding_flags);

        let bindings = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDLESS_SAMPLER_BINDING)
                .descriptor_type(vk::DescriptorType::SAMPLER)
                .descriptor_count(BINDLESS_SAMPLER_CAPACITY)
                .stage_flags(vk::ShaderStageFlags::ALL),
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDLESS_SAMPLED_IMAGE_BINDING)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(BINDLESS_SAMPLED_IMAGE_CAPACITY)
                .stage_flags(vk::ShaderStageFlags::ALL),
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDLESS_STORAGE_IMAGE_BINDING)
                .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                .descriptor_count(BINDLESS_STORAGE_IMAGE_CAPACITY)
                .stage_flags(vk::ShaderStageFlags::ALL),
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDLESS_STORAGE_BUFFER_BINDING)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(BINDLESS_STORAGE_BUFFER_CAPACITY)
                .stage_flags(vk::ShaderStageFlags::ALL),
        ];

        let layout_info = vk::DescriptorSetLayoutCreateInfo::default()
            // UPDATE_AFTER_BIND_POOL: the pool must have the matching flag
            .flags(vk::DescriptorSetLayoutCreateFlags::UPDATE_AFTER_BIND_POOL)
            .bindings(&bindings)
            .push(&mut binding_flags_info);

        let set_layout = unsafe {
            device
                .create_descriptor_set_layout(&layout_info, None)
                .map_err(|e| {
                    Error::Backend(format!("bindless set layout creation failed: {e:?}"))
                })?
        };

        // Pool sized for exactly one descriptor set.
        let pool_sizes = [
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::SAMPLER,
                descriptor_count: BINDLESS_SAMPLER_CAPACITY,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::SAMPLED_IMAGE,
                descriptor_count: BINDLESS_SAMPLED_IMAGE_CAPACITY,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::STORAGE_IMAGE,
                descriptor_count: BINDLESS_STORAGE_IMAGE_CAPACITY,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::STORAGE_BUFFER,
                descriptor_count: BINDLESS_STORAGE_BUFFER_CAPACITY,
            },
        ];

        let pool_info = vk::DescriptorPoolCreateInfo::default()
            // UPDATE_AFTER_BIND: matches the set layout flag
            .flags(vk::DescriptorPoolCreateFlags::UPDATE_AFTER_BIND)
            .max_sets(1)
            .pool_sizes(&pool_sizes);

        let pool = unsafe {
            device
                .create_descriptor_pool(&pool_info, None)
                .map_err(|e| {
                    device.destroy_descriptor_set_layout(set_layout, None);
                    Error::Backend(format!("bindless descriptor pool creation failed: {e:?}"))
                })?
        };

        let alloc_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(pool)
            .set_layouts(std::slice::from_ref(&set_layout));

        let set = unsafe {
            device
                .allocate_descriptor_sets(&alloc_info)
                .map_err(|e| {
                    device.destroy_descriptor_pool(pool, None);
                    device.destroy_descriptor_set_layout(set_layout, None);
                    Error::Backend(format!("bindless descriptor set allocation failed: {e:?}"))
                })?
                .into_iter()
                .next()
                .ok_or_else(|| Error::Backend("bindless: no descriptor set returned".into()))?
        };

        Ok(Self {
            set_layout,
            pool,
            set,
            next_sampler: AtomicU32::new(0),
            next_sampled_image: AtomicU32::new(0),
            next_storage_image: AtomicU32::new(0),
            next_storage_buffer: AtomicU32::new(0),
            device: device.clone(),
        })
    }

    // ── Registration ──────────────────────────────────────────────────────────

    /// Register a sampler and return its stable bindless index.
    ///
    /// The index is valid for the lifetime of this heap. Never returns the
    /// same index twice. Returns `None` when capacity is exhausted.
    pub fn register_sampler(&self, sampler: vk::Sampler) -> Option<u32> {
        let idx = self.next_sampler.fetch_add(1, Ordering::Relaxed);
        if idx >= BINDLESS_SAMPLER_CAPACITY {
            return None;
        }
        let sampler_info = [vk::DescriptorImageInfo {
            sampler,
            image_view: vk::ImageView::null(),
            image_layout: vk::ImageLayout::UNDEFINED,
        }];
        let write = vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(BINDLESS_SAMPLER_BINDING)
            .dst_array_element(idx)
            .descriptor_type(vk::DescriptorType::SAMPLER)
            .image_info(&sampler_info);
        unsafe { self.device.update_descriptor_sets(&[write], &[]) };
        Some(idx)
    }

    /// Register a sampled (read-only) image view and return its stable bindless index.
    ///
    /// The image must be in `SHADER_READ_ONLY_OPTIMAL` layout when sampled.
    pub fn register_sampled_image(&self, image_view: vk::ImageView) -> Option<u32> {
        let idx = self.next_sampled_image.fetch_add(1, Ordering::Relaxed);
        if idx >= BINDLESS_SAMPLED_IMAGE_CAPACITY {
            return None;
        }
        let image_info = [vk::DescriptorImageInfo {
            sampler: vk::Sampler::null(),
            image_view,
            image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        }];
        let write = vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(BINDLESS_SAMPLED_IMAGE_BINDING)
            .dst_array_element(idx)
            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
            .image_info(&image_info);
        unsafe { self.device.update_descriptor_sets(&[write], &[]) };
        Some(idx)
    }

    /// Register a storage (read-write) image view and return its stable bindless index.
    ///
    /// The image must be in `GENERAL` layout when accessed.
    pub fn register_storage_image(&self, image_view: vk::ImageView) -> Option<u32> {
        let idx = self.next_storage_image.fetch_add(1, Ordering::Relaxed);
        if idx >= BINDLESS_STORAGE_IMAGE_CAPACITY {
            return None;
        }
        let image_info = [vk::DescriptorImageInfo {
            sampler: vk::Sampler::null(),
            image_view,
            image_layout: vk::ImageLayout::GENERAL,
        }];
        let write = vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(BINDLESS_STORAGE_IMAGE_BINDING)
            .dst_array_element(idx)
            .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
            .image_info(&image_info);
        unsafe { self.device.update_descriptor_sets(&[write], &[]) };
        Some(idx)
    }

    /// Register a storage buffer and return its stable bindless index.
    pub fn register_storage_buffer(
        &self,
        buffer: vk::Buffer,
        offset: u64,
        size: u64,
    ) -> Option<u32> {
        let idx = self.next_storage_buffer.fetch_add(1, Ordering::Relaxed);
        if idx >= BINDLESS_STORAGE_BUFFER_CAPACITY {
            return None;
        }
        let buf_info = [vk::DescriptorBufferInfo {
            buffer,
            offset,
            range: size,
        }];
        let write = vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(BINDLESS_STORAGE_BUFFER_BINDING)
            .dst_array_element(idx)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&buf_info);
        unsafe { self.device.update_descriptor_sets(&[write], &[]) };
        Some(idx)
    }

    // ── Diagnostics ──────────────────────────────────────────────────────────

    /// Number of registered samplers.
    #[allow(dead_code)]
    pub fn sampler_count(&self) -> u32 {
        self.next_sampler
            .load(Ordering::Relaxed)
            .min(BINDLESS_SAMPLER_CAPACITY)
    }

    /// Number of registered sampled images.
    #[allow(dead_code)]
    pub fn sampled_image_count(&self) -> u32 {
        self.next_sampled_image
            .load(Ordering::Relaxed)
            .min(BINDLESS_SAMPLED_IMAGE_CAPACITY)
    }

    /// Validate an index is within the registered range (debug builds only).
    ///
    /// In release builds this is a no-op.
    #[allow(dead_code)]
    pub fn validate_sampled_image_index(&self, index: u32) {
        if cfg!(debug_assertions) {
            let count = self.next_sampled_image.load(Ordering::Relaxed);
            assert!(
                index < count,
                "bindless sampled-image index {index} is out of range (registered: {count})"
            );
        }
    }

    #[allow(dead_code)]
    pub fn validate_sampler_index(&self, index: u32) {
        if cfg!(debug_assertions) {
            let count = self.next_sampler.load(Ordering::Relaxed);
            assert!(
                index < count,
                "bindless sampler index {index} is out of range (registered: {count})"
            );
        }
    }
}

impl Drop for BindlessHeap {
    fn drop(&mut self) {
        unsafe {
            // Descriptor sets are implicitly freed when the pool is destroyed.
            self.device.destroy_descriptor_pool(self.pool, None);
            self.device
                .destroy_descriptor_set_layout(self.set_layout, None);
        }
    }
}

// SAFETY: BindlessHeap is Send+Sync because:
// - AtomicU32 is Send+Sync
// - vk::DescriptorSet/Pool/Layout are u64 handles — no pointer types
// - Descriptor updates via update_descriptor_sets are internally synchronized
//   by the Vulkan implementation (spec sec. 5.1: commands on different VkDevices are not
//   synchronized, but we have exactly one VkDevice per VulkanBackend)
unsafe impl Send for BindlessHeap {}
unsafe impl Sync for BindlessHeap {}

// ── GFX-7b: VK_EXT_descriptor_heap alternative ──────────────────────────────
//
// `DescriptorHeapBindlessHeap` replaces the pool+set model with two GPU-visible,
// CPU-writable buffers — one sampler heap and one resource heap.  Descriptors are
// written directly into mapped host memory via `vkWriteSamplerDescriptorsEXT` and
// `vkWriteResourceDescriptorsEXT` instead of through the pool allocation path.
//
// Index space is identical to `BindlessHeap` so handles are interchangeable.
//
// Shader-side:  shaders must be recompiled with heap-access decorations
// (`[[vk::heap]]` in Slang / `HeapEXT` SPIR-V string decoration) and pipelines
// must carry `VK_PIPELINE_CREATE_2_DESCRIPTOR_HEAP_BIT_EXT`.  Until those changes
// land the pool heap continues to drive rendering while this heap is maintained
// in parallel for validation and future switch-over.

/// Per-type descriptor sizes queried from the driver.
#[derive(Clone, Copy, Debug, Default)]
pub struct DescriptorHeapSizes {
    pub sampler: u64,
    pub sampled_image: u64,
    pub storage_image: u64,
    pub storage_buffer: u64,
}

impl DescriptorHeapSizes {
    /// Total resource heap size for the bindless capacities.
    pub fn resource_heap_bytes(&self) -> u64 {
        self.sampler * BINDLESS_SAMPLER_CAPACITY as u64
            + self.sampled_image * BINDLESS_SAMPLED_IMAGE_CAPACITY as u64
            + self.storage_image * BINDLESS_STORAGE_IMAGE_CAPACITY as u64
            + self.storage_buffer * BINDLESS_STORAGE_BUFFER_CAPACITY as u64
    }

    pub fn sampler_offset(&self) -> u64 {
        0
    }
    pub fn sampled_image_offset(&self) -> u64 {
        self.sampler * BINDLESS_SAMPLER_CAPACITY as u64
    }
    pub fn storage_image_offset(&self) -> u64 {
        self.sampled_image_offset() + self.sampled_image * BINDLESS_SAMPLED_IMAGE_CAPACITY as u64
    }
    pub fn storage_buffer_offset(&self) -> u64 {
        self.storage_image_offset() + self.storage_image * BINDLESS_STORAGE_IMAGE_CAPACITY as u64
    }
}

/// GPU-virtual address range of a heap buffer — passed to `cmd_bind_resource_heap` /
/// `cmd_bind_sampler_heap`.
#[derive(Clone, Copy, Debug)]
pub struct DescriptorHeapRange {
    /// GPU virtual address of the heap buffer base.
    pub device_address: u64,
    /// Total byte size of the heap.
    pub size: u64,
}

/// `VK_EXT_descriptor_heap`-backed bindless resource storage.
///
/// All descriptors live in one CPU-writable, GPU-visible buffer.  The sampler
/// heap is a separate buffer for embedded-sampler descriptors.
pub struct DescriptorHeapBindlessHeap {
    /// Resource heap: sampled images + storage images + storage buffers (+ embedded samplers).
    resource_heap_buffer: vk::Buffer,
    resource_heap_memory: vk::DeviceMemory,
    /// CPU-mapped base pointer into the resource heap.
    resource_heap_ptr: *mut u8,
    resource_heap_device_address: u64,

    /// Per-type descriptor byte sizes from the driver.
    pub sizes: DescriptorHeapSizes,

    next_sampler: AtomicU32,
    next_sampled_image: AtomicU32,
    next_storage_image: AtomicU32,
    next_storage_buffer: AtomicU32,

    device: Device,
    heap_ext: ash::ext::descriptor_heap::Device,
}

impl DescriptorHeapBindlessHeap {
    /// Create the heap buffer and map it for CPU writes.
    ///
    /// `sizes` must be pre-queried via `Device::descriptor_heap_type_size`.
    pub fn create(
        device: &Device,
        _instance: &ash::Instance,
        _physical_device: vk::PhysicalDevice,
        sizes: DescriptorHeapSizes,
        heap_ext: ash::ext::descriptor_heap::Device,
        memory_properties: vk::PhysicalDeviceMemoryProperties,
    ) -> Result<Self> {
        let heap_bytes = sizes.resource_heap_bytes();
        if heap_bytes == 0 {
            return Err(Error::Backend(
                "descriptor heap sizes are zero — driver did not report valid sizes".into(),
            ));
        }

        // Buffer needs: DESCRIPTOR_HEAP_BIT_EXT (heap storage) +
        //               SHADER_DEVICE_ADDRESS (for BindHeapInfoEXT device address) +
        //               HOST_VISIBLE for CPU writes via vkWriteResourceDescriptorsEXT.
        let buf_info = vk::BufferCreateInfo::default()
            .size(heap_bytes)
            .usage(
                vk::BufferUsageFlags::DESCRIPTOR_HEAP_EXT
                    | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            )
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let buffer = unsafe {
            device.create_buffer(&buf_info, None).map_err(|e| {
                Error::Backend(format!("descriptor heap buffer creation failed: {e:?}"))
            })?
        };

        let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };

        // Find a host-visible + host-coherent memory type.
        let memory_type = (0..memory_properties.memory_type_count)
            .find(|&i| {
                let supported = (requirements.memory_type_bits & (1 << i)) != 0;
                let flags = memory_properties.memory_types[i as usize].property_flags;
                supported
                    && flags.contains(
                        vk::MemoryPropertyFlags::HOST_VISIBLE
                            | vk::MemoryPropertyFlags::HOST_COHERENT,
                    )
            })
            .ok_or_else(|| {
                Error::Backend("no host-visible memory type for descriptor heap".into())
            })?;

        let mut alloc_info = vk::MemoryAllocateInfo::default()
            .allocation_size(requirements.size)
            .memory_type_index(memory_type);
        // Chain SHADER_DEVICE_ADDRESS flag to get a device-addressable allocation.
        let mut addr_info =
            vk::MemoryAllocateFlagsInfo::default().flags(vk::MemoryAllocateFlags::DEVICE_ADDRESS);
        alloc_info = alloc_info.push(&mut addr_info);

        let memory = unsafe {
            device.allocate_memory(&alloc_info, None).map_err(|e| {
                device.destroy_buffer(buffer, None);
                Error::Backend(format!("descriptor heap memory allocation failed: {e:?}"))
            })?
        };

        unsafe {
            device.bind_buffer_memory(buffer, memory, 0).map_err(|e| {
                device.free_memory(memory, None);
                device.destroy_buffer(buffer, None);
                Error::Backend(format!("descriptor heap bind_buffer_memory failed: {e:?}"))
            })?;
        }

        let mapped_ptr = unsafe {
            device
                .map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
                .map_err(|e| {
                    device.free_memory(memory, None);
                    device.destroy_buffer(buffer, None);
                    Error::Backend(format!("descriptor heap map_memory failed: {e:?}"))
                })? as *mut u8
        };

        let device_address = unsafe {
            device.get_buffer_device_address(&vk::BufferDeviceAddressInfo::default().buffer(buffer))
        };

        Ok(Self {
            resource_heap_buffer: buffer,
            resource_heap_memory: memory,
            resource_heap_ptr: mapped_ptr,
            resource_heap_device_address: device_address,
            sizes,
            next_sampler: AtomicU32::new(0),
            next_sampled_image: AtomicU32::new(0),
            next_storage_image: AtomicU32::new(0),
            next_storage_buffer: AtomicU32::new(0),
            device: device.clone(),
            heap_ext,
        })
    }

    // ── GPU address range for bind commands ───────────────────────────────────

    /// Device address range covering the full resource heap — pass to `cmd_bind_resource_heap`.
    pub fn resource_heap_range(&self) -> DescriptorHeapRange {
        DescriptorHeapRange {
            device_address: self.resource_heap_device_address,
            size: self.sizes.resource_heap_bytes(),
        }
    }

    // ── Registration ──────────────────────────────────────────────────────────

    /// Write an embedded sampler into the sampler slot and return its index.
    ///
    /// `sampler_create_info` must be fully populated (no pNext chains are forwarded).
    pub fn register_sampler_from_desc(
        &self,
        sampler_create_info: vk::SamplerCreateInfo<'_>,
    ) -> Option<u32> {
        let idx = self.next_sampler.fetch_add(1, Ordering::Relaxed);
        if idx >= BINDLESS_SAMPLER_CAPACITY {
            return None;
        }
        let offset = self.sizes.sampler_offset() + idx as u64 * self.sizes.sampler;
        let slot_ptr = unsafe { self.resource_heap_ptr.add(offset as usize) };
        let dest = [vk::HostAddressRangeEXT::default().address(unsafe {
            std::slice::from_raw_parts_mut(slot_ptr, self.sizes.sampler as usize)
        })];
        let _ = unsafe {
            self.heap_ext
                .write_sampler_descriptors(&[sampler_create_info], &dest)
        };
        Some(idx)
    }

    /// Write a sampled-image descriptor and return its bindless index.
    pub fn register_sampled_image(
        &self,
        image: vk::Image,
        format: vk::Format,
        view_type: vk::ImageViewType,
        subresource: vk::ImageSubresourceRange,
    ) -> Option<u32> {
        let idx = self.next_sampled_image.fetch_add(1, Ordering::Relaxed);
        if idx >= BINDLESS_SAMPLED_IMAGE_CAPACITY {
            return None;
        }
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(view_type)
            .format(format)
            .subresource_range(subresource);
        let img_desc_info = vk::ImageDescriptorInfoEXT {
            p_view: &view_info,
            layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            ..Default::default()
        };
        let res_info = [vk::ResourceDescriptorInfoEXT::default()
            .ty(vk::DescriptorType::SAMPLED_IMAGE)
            .data(vk::ResourceDescriptorDataEXT {
                p_image: &img_desc_info,
            })];
        let offset = self.sizes.sampled_image_offset() + idx as u64 * self.sizes.sampled_image;
        let slot_ptr = unsafe { self.resource_heap_ptr.add(offset as usize) };
        let dest = [vk::HostAddressRangeEXT::default().address(unsafe {
            std::slice::from_raw_parts_mut(slot_ptr, self.sizes.sampled_image as usize)
        })];
        let _ = unsafe { self.heap_ext.write_resource_descriptors(&res_info, &dest) };
        Some(idx)
    }

    /// Write a storage-image descriptor and return its bindless index.
    pub fn register_storage_image(
        &self,
        image: vk::Image,
        format: vk::Format,
        view_type: vk::ImageViewType,
        subresource: vk::ImageSubresourceRange,
    ) -> Option<u32> {
        let idx = self.next_storage_image.fetch_add(1, Ordering::Relaxed);
        if idx >= BINDLESS_STORAGE_IMAGE_CAPACITY {
            return None;
        }
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(view_type)
            .format(format)
            .subresource_range(subresource);
        let img_desc_info = vk::ImageDescriptorInfoEXT {
            p_view: &view_info,
            layout: vk::ImageLayout::GENERAL,
            ..Default::default()
        };
        let res_info = [vk::ResourceDescriptorInfoEXT::default()
            .ty(vk::DescriptorType::STORAGE_IMAGE)
            .data(vk::ResourceDescriptorDataEXT {
                p_image: &img_desc_info,
            })];
        let offset = self.sizes.storage_image_offset() + idx as u64 * self.sizes.storage_image;
        let slot_ptr = unsafe { self.resource_heap_ptr.add(offset as usize) };
        let dest = [vk::HostAddressRangeEXT::default().address(unsafe {
            std::slice::from_raw_parts_mut(slot_ptr, self.sizes.storage_image as usize)
        })];
        let _ = unsafe { self.heap_ext.write_resource_descriptors(&res_info, &dest) };
        Some(idx)
    }

    /// Write a storage-buffer descriptor using the buffer's device address.
    pub fn register_storage_buffer(&self, device_address: u64, range: u64) -> Option<u32> {
        let idx = self.next_storage_buffer.fetch_add(1, Ordering::Relaxed);
        if idx >= BINDLESS_STORAGE_BUFFER_CAPACITY {
            return None;
        }
        let addr_range = vk::DeviceAddressRangeEXT {
            address: device_address,
            size: range,
        };
        let res_info = [vk::ResourceDescriptorInfoEXT::default()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .data(vk::ResourceDescriptorDataEXT {
                p_address_range: &addr_range,
            })];
        let offset = self.sizes.storage_buffer_offset() + idx as u64 * self.sizes.storage_buffer;
        let slot_ptr = unsafe { self.resource_heap_ptr.add(offset as usize) };
        let dest = [vk::HostAddressRangeEXT::default().address(unsafe {
            std::slice::from_raw_parts_mut(slot_ptr, self.sizes.storage_buffer as usize)
        })];
        let _ = unsafe { self.heap_ext.write_resource_descriptors(&res_info, &dest) };
        Some(idx)
    }
}

impl Drop for DescriptorHeapBindlessHeap {
    fn drop(&mut self) {
        unsafe {
            self.device.unmap_memory(self.resource_heap_memory);
            self.device.destroy_buffer(self.resource_heap_buffer, None);
            self.device.free_memory(self.resource_heap_memory, None);
        }
    }
}

unsafe impl Send for DescriptorHeapBindlessHeap {}
unsafe impl Sync for DescriptorHeapBindlessHeap {}
