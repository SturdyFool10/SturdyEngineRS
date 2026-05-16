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
            .push_next(&mut binding_flags_info);

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
