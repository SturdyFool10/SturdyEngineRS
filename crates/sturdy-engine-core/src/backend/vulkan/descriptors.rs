use std::collections::HashMap;

use ash::vk::TaggedStructure;
use ash::{Device, vk};

use crate::{
    BINDLESS_COUNT, BindGroupDesc, BindGroupHandle, BindingKind, CanonicalBinding,
    CanonicalGroupLayout, CanonicalPipelineLayout, Error, Limits, PipelineLayoutHandle,
    ResourceBinding, Result, StageMask,
};

use super::bindless::{
    BINDLESS_SAMPLED_IMAGE_BINDING, BINDLESS_SAMPLER_BINDING, BINDLESS_STORAGE_BUFFER_BINDING,
    BINDLESS_STORAGE_IMAGE_BINDING,
};
use super::resources::ResourceRegistry;

/// How many bind groups each pool page can hold before a new page is appended.
const POOL_PAGE_CAPACITY: u32 = 64;

pub struct DescriptorRegistry {
    layouts: HashMap<PipelineLayoutHandle, VulkanPipelineLayout>,
    bind_groups: HashMap<BindGroupHandle, VulkanBindGroup>,
    /// Per-layout pool slabs. One slab per unique pipeline layout; each slab
    /// holds pages of VkDescriptorPool that are reused across many bind groups.
    pool_slabs: HashMap<PipelineLayoutHandle, LayoutPoolSlab>,
    /// GFX-7a: VK_EXT_descriptor_buffer commands. When Some, bind groups also
    /// create CPU-writable buffer-backed descriptor storage.
    descriptor_buffer_ext: Option<ash::ext::descriptor_buffer::Device>,
    /// GFX-7a: buffer device address feature must be on for buffer-backed descriptors.
    pub buffer_device_address_enabled: bool,
}

struct VulkanPipelineLayout {
    pipeline_layout: vk::PipelineLayout,
    set_layouts: Vec<vk::DescriptorSetLayout>,
    owned_set_layouts: Vec<vk::DescriptorSetLayout>,
    bind_group_set_layouts: Vec<vk::DescriptorSetLayout>,
    bind_group_first_set: u32,
    uses_bindless: bool,
    bindings: HashMap<String, VulkanBinding>,
    /// Per-bind-group pool sizes (counts for one allocation, not for a full page).
    pool_sizes_per_bg: Vec<vk::DescriptorPoolSize>,
    push_constants_bytes: u32,
    push_constant_stages: vk::ShaderStageFlags,
}

#[derive(Copy, Clone)]
struct VulkanBinding {
    allocated_set_index: usize,
    binding_index: u32,
    descriptor_type: vk::DescriptorType,
}

struct VulkanBindGroup {
    layout: PipelineLayoutHandle,
    first_set: u32,
    /// Pool this bind group's sets were allocated from — needed for freeing.
    pool: vk::DescriptorPool,
    sets: Vec<vk::DescriptorSet>,
    /// GFX-7a: CPU-writable buffer backing for descriptor buffer mode.
    /// Present when `VK_EXT_descriptor_buffer` is active and the layout is not bindless.
    db_backing: Option<DescriptorBufferBacking>,
}

/// GFX-7a: A single HOST_VISIBLE buffer holding all descriptors for one bind group.
struct DescriptorBufferBacking {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    /// CPU-mapped pointer into the buffer's memory.
    mapped_ptr: *mut u8,
    /// GPU virtual address of the buffer — passed to `cmd_bind_descriptor_buffers_ext`.
    device_address: vk::DeviceAddress,
    /// Total byte size of the buffer.
    size: vk::DeviceSize,
}

// SAFETY: DescriptorBufferBacking is only accessed through the descriptor registry's Mutex.
unsafe impl Send for DescriptorBufferBacking {}
unsafe impl Sync for DescriptorBufferBacking {}

/// A growable list of `VkDescriptorPool` pages for one pipeline layout.
///
/// Each page has capacity for `POOL_PAGE_CAPACITY` bind groups.  Sets are freed
/// individually (the pools carry `FREE_DESCRIPTOR_SET`), so pages are never
/// destroyed until the layout itself is destroyed.
struct LayoutPoolSlab {
    pages: Vec<PoolPage>,
    /// Pool sizes scaled for a single bind-group allocation.
    pool_sizes_per_bg: Vec<vk::DescriptorPoolSize>,
    /// Number of descriptor sets in one bind group (= number of set_layouts).
    sets_per_bg: u32,
}

struct PoolPage {
    pool: vk::DescriptorPool,
    /// How many more bind-group allocations this page can satisfy.
    remaining: u32,
}

impl LayoutPoolSlab {
    fn new(pool_sizes_per_bg: Vec<vk::DescriptorPoolSize>, sets_per_bg: u32) -> Self {
        Self {
            pages: Vec::new(),
            pool_sizes_per_bg,
            sets_per_bg,
        }
    }

    fn allocate(
        &mut self,
        device: &Device,
        set_layouts: &[vk::DescriptorSetLayout],
    ) -> Result<(vk::DescriptorPool, Vec<vk::DescriptorSet>)> {
        // Find a page with enough remaining capacity.
        let page_idx = self
            .pages
            .iter()
            .position(|p| p.remaining >= self.sets_per_bg.max(1));

        let pool = if let Some(idx) = page_idx {
            self.pages[idx].remaining -= self.sets_per_bg.max(1);
            self.pages[idx].pool
        } else {
            tracing::debug!(
                pages_now = self.pages.len() + 1,
                capacity = POOL_PAGE_CAPACITY,
                "allocating new descriptor pool page"
            );
            let pool = create_pool(device, &self.pool_sizes_per_bg, POOL_PAGE_CAPACITY)?;
            let remaining = POOL_PAGE_CAPACITY - self.sets_per_bg.max(1);
            self.pages.push(PoolPage { pool, remaining });
            pool
        };

        if set_layouts.is_empty() {
            return Ok((pool, Vec::new()));
        }

        let allocate_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(pool)
            .set_layouts(set_layouts);
        let sets = unsafe {
            device
                .allocate_descriptor_sets(&allocate_info)
                .map_err(|error| {
                    tracing::error!(
                        set_layouts = set_layouts.len(),
                        pages = self.pages.len(),
                        "vkAllocateDescriptorSets failed — pool may be exhausted or fragmented; \
                         check POOL_PAGE_CAPACITY or descriptor type counts: {error:?}"
                    );
                    Error::Backend(format!("vkAllocateDescriptorSets failed: {error:?}"))
                })?
        };
        Ok((pool, sets))
    }

    fn free(&mut self, device: &Device, pool: vk::DescriptorPool, sets: &[vk::DescriptorSet]) {
        if !sets.is_empty() {
            unsafe {
                // FREE_DESCRIPTOR_SET flag was set at pool creation — individual frees are valid.
                let _ = device.free_descriptor_sets(pool, sets);
            }
        }
        // Return capacity to the page.
        if let Some(page) = self.pages.iter_mut().find(|p| p.pool == pool) {
            page.remaining += self.sets_per_bg.max(1);
        }
    }

    fn destroy_all(&mut self, device: &Device) {
        for page in self.pages.drain(..) {
            unsafe {
                device.destroy_descriptor_pool(page.pool, None);
            }
        }
    }
}

/// Create a descriptor pool that can hold `capacity` bind-group allocations.
///
/// The `FREE_DESCRIPTOR_SET` flag is always set so individual sets can be freed
/// without destroying the whole pool.
fn create_pool(
    device: &Device,
    sizes_per_bg: &[vk::DescriptorPoolSize],
    capacity: u32,
) -> Result<vk::DescriptorPool> {
    let scaled: Vec<vk::DescriptorPoolSize> = sizes_per_bg
        .iter()
        .map(|s| vk::DescriptorPoolSize {
            ty: s.ty,
            descriptor_count: s.descriptor_count * capacity,
        })
        .collect();
    let fallback;
    let pool_sizes: &[vk::DescriptorPoolSize] = if scaled.is_empty() {
        fallback = [vk::DescriptorPoolSize {
            ty: vk::DescriptorType::UNIFORM_BUFFER,
            descriptor_count: capacity,
        }];
        &fallback
    } else {
        &scaled
    };
    let info = vk::DescriptorPoolCreateInfo::default()
        .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
        .max_sets(capacity)
        .pool_sizes(pool_sizes);
    unsafe {
        device.create_descriptor_pool(&info, None).map_err(|error| {
            tracing::error!(
                capacity,
                pool_size_types = pool_sizes.len(),
                "vkCreateDescriptorPool failed — device may have exhausted descriptor pool \
                     resources or the pool size type list is empty: {error:?}"
            );
            Error::Backend(format!("vkCreateDescriptorPool failed: {error:?}"))
        })
    }
}

impl Default for DescriptorRegistry {
    fn default() -> Self {
        Self {
            layouts: HashMap::new(),
            bind_groups: HashMap::new(),
            pool_slabs: HashMap::new(),
            descriptor_buffer_ext: None,
            buffer_device_address_enabled: false,
        }
    }
}

impl DescriptorRegistry {
    /// GFX-7a: enable descriptor buffer backing when the extension is available.
    pub fn set_descriptor_buffer(&mut self, device: ash::ext::descriptor_buffer::Device) {
        self.descriptor_buffer_ext = Some(device);
    }

    pub fn create_pipeline_layout(
        &mut self,
        device: &Device,
        handle: PipelineLayoutHandle,
        layout: &CanonicalPipelineLayout,
        bindless_set_layout: Option<vk::DescriptorSetLayout>,
        limits: &Limits,
    ) -> Result<()> {
        tracing::debug!(
            ?handle,
            groups = layout.groups.len(),
            bindless = bindless_set_layout.is_some(),
            push_constants_bytes = layout.push_constants_bytes,
            "creating pipeline layout"
        );
        validate_pipeline_layout(layout, bindless_set_layout.is_some(), limits)?;

        let mut set_layouts = Vec::with_capacity(layout.groups.len());
        let mut owned_set_layouts = Vec::with_capacity(layout.groups.len());
        let mut bind_group_set_layouts = Vec::with_capacity(layout.groups.len());
        let mut bind_group_first_set = None;
        let mut uses_bindless = false;
        let mut binding_map = HashMap::new();
        let mut pool_counts: HashMap<vk::DescriptorType, u32> = HashMap::new();

        for (set_index, group) in layout.groups.iter().enumerate() {
            let is_bindless = group_is_bindless(group);
            if is_bindless {
                if set_index != 0 {
                    destroy_set_layouts(device, &mut owned_set_layouts);
                    return Err(Error::InvalidInput(
                        "bindless descriptor group must be reflected at set 0".into(),
                    ));
                }
                let set_layout = bindless_set_layout.ok_or_else(|| {
                    Error::Unsupported(
                        "pipeline layout uses bindless descriptors, but bindless is not supported by this device",
                    )
                })?;
                set_layouts.push(set_layout);
                uses_bindless = true;
                continue;
            }

            if bind_group_first_set.is_none() {
                bind_group_first_set = Some(set_index as u32);
            }
            let allocated_set_index = bind_group_set_layouts.len();
            let bindings = group
                .bindings
                .iter()
                .map(|binding| {
                    let binding_index = binding.binding;
                    let descriptor_type = descriptor_type(binding.kind);
                    binding_map.insert(
                        binding.path.clone(),
                        VulkanBinding {
                            allocated_set_index,
                            binding_index,
                            descriptor_type,
                        },
                    );
                    *pool_counts.entry(descriptor_type).or_default() += binding.count.max(1);
                    descriptor_set_layout_binding(binding_index, binding)
                })
                .collect::<Vec<_>>();
            let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
            let set_layout = unsafe {
                match device.create_descriptor_set_layout(&info, None) {
                    Ok(layout) => layout,
                    Err(error) => {
                        tracing::error!(
                            ?handle,
                            set_index,
                            bindings = bindings.len(),
                            "vkCreateDescriptorSetLayout failed — check binding count limits \
                             or unsupported descriptor types in this set: {error:?}"
                        );
                        destroy_set_layouts(device, &mut owned_set_layouts);
                        return Err(Error::Backend(format!(
                            "vkCreateDescriptorSetLayout failed: {error:?}"
                        )));
                    }
                }
            };
            set_layouts.push(set_layout);
            owned_set_layouts.push(set_layout);
            bind_group_set_layouts.push(set_layout);
        }

        let push_constant_stages = if layout.push_constants_bytes == 0 {
            vk::ShaderStageFlags::empty()
        } else {
            shader_stage_flags(layout.push_constants_stage_mask)
        };
        let push_ranges = if layout.push_constants_bytes == 0 {
            Vec::new()
        } else {
            vec![
                vk::PushConstantRange::default()
                    .stage_flags(push_constant_stages)
                    .offset(0)
                    .size(layout.push_constants_bytes),
            ]
        };
        let info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&set_layouts)
            .push_constant_ranges(&push_ranges);
        let pipeline_layout = unsafe {
            match device.create_pipeline_layout(&info, None) {
                Ok(layout) => layout,
                Err(error) => {
                    tracing::error!(
                        ?handle,
                        set_count = set_layouts.len(),
                        push_constants_bytes = layout.push_constants_bytes,
                        "vkCreatePipelineLayout failed — push constant size may exceed \
                         device limit, or too many descriptor sets: {error:?}"
                    );
                    destroy_set_layouts(device, &mut owned_set_layouts);
                    return Err(Error::Backend(format!(
                        "vkCreatePipelineLayout failed: {error:?}"
                    )));
                }
            }
        };

        let pool_sizes_per_bg = pool_counts
            .into_iter()
            .map(|(ty, descriptor_count)| vk::DescriptorPoolSize {
                ty,
                descriptor_count,
            })
            .collect::<Vec<_>>();
        self.layouts.insert(
            handle,
            VulkanPipelineLayout {
                pipeline_layout,
                set_layouts,
                owned_set_layouts,
                bind_group_set_layouts,
                bind_group_first_set: bind_group_first_set.unwrap_or(0),
                uses_bindless,
                bindings: binding_map,
                pool_sizes_per_bg,
                push_constants_bytes: layout.push_constants_bytes,
                push_constant_stages,
            },
        );
        tracing::debug!(?handle, uses_bindless, "pipeline layout created");
        // Pre-register the pool slab so it's ready on first bind-group creation.
        let layout = self.layouts.get(&handle).ok_or_else(|| {
            Error::ResourceStateCorruption(
                "pipeline layout registry insert did not persist the new layout".into(),
            )
        })?;
        self.pool_slabs.insert(
            handle,
            LayoutPoolSlab::new(
                layout.pool_sizes_per_bg.clone(),
                layout.bind_group_set_layouts.len() as u32,
            ),
        );
        Ok(())
    }

    pub fn destroy_pipeline_layout(
        &mut self,
        device: &Device,
        handle: PipelineLayoutHandle,
    ) -> Result<()> {
        if self
            .bind_groups
            .values()
            .any(|group| group.references_layout(handle, &self.layouts))
        {
            return Err(Error::InvalidInput(
                "cannot destroy pipeline layout while bind groups still reference it".into(),
            ));
        }
        let layout = self.layouts.remove(&handle).ok_or(Error::InvalidHandle)?;
        unsafe {
            device.destroy_pipeline_layout(layout.pipeline_layout, None);
        }
        let mut set_layouts = layout.owned_set_layouts;
        destroy_set_layouts(device, &mut set_layouts);
        // Destroy all pool pages for this layout.
        if let Some(mut slab) = self.pool_slabs.remove(&handle) {
            slab.destroy_all(device);
        }
        Ok(())
    }

    /// GFX-7a: Bind descriptor buffer resources OR descriptor sets, depending on availability.
    ///
    /// When all bind groups have descriptor buffer backing, uses
    /// `cmd_bind_descriptor_buffers_ext` + `cmd_set_descriptor_buffer_offsets_ext`.
    /// Otherwise falls back to `cmd_bind_descriptor_sets`.
    pub fn bind_descriptor_buffer_or_sets(
        &self,
        device: &Device,
        command_buffer: vk::CommandBuffer,
        bind_point: vk::PipelineBindPoint,
        layout: vk::PipelineLayout,
        bind_groups: &[crate::BindGroupHandle],
        uses_bindless: bool,
    ) -> crate::Result<()> {
        if bind_groups.is_empty() {
            return Ok(());
        }
        // Try the descriptor buffer path when available and all groups have backing.
        if let Some(db_ext) = &self.descriptor_buffer_ext {
            if !uses_bindless && self.all_have_descriptor_buffer(bind_groups) {
                let mut binding_infos = Vec::new();
                let mut buffer_indices: Vec<u32> = Vec::new();
                let mut offsets: Vec<vk::DeviceSize> = Vec::new();
                let mut first_set = 0u32;
                for (buf_idx, handle) in bind_groups.iter().enumerate() {
                    if let Some((addr, size, fs)) = self.descriptor_buffer_binding(*handle) {
                        let info = vk::DescriptorBufferBindingInfoEXT::default()
                            .address(addr)
                            .usage(
                                vk::BufferUsageFlags::RESOURCE_DESCRIPTOR_BUFFER_EXT
                                    | vk::BufferUsageFlags::SAMPLER_DESCRIPTOR_BUFFER_EXT,
                            );
                        binding_infos.push(info);
                        buffer_indices.push(buf_idx as u32);
                        offsets.push(0); // each bind group owns its full buffer from offset 0
                        first_set = fs;
                        let _ = size;
                    }
                }
                unsafe {
                    db_ext.cmd_bind_descriptor_buffers(command_buffer, &binding_infos);
                    db_ext.cmd_set_descriptor_buffer_offsets(
                        command_buffer,
                        bind_point,
                        layout,
                        first_set,
                        &buffer_indices,
                        &offsets,
                    );
                }
                return Ok(());
            }
        }
        // Fall back to pool-based descriptor sets.
        let mut sets = Vec::new();
        let mut first_set = None;
        for handle in bind_groups {
            let group_first_set = self.bind_group_first_set(*handle)?;
            if let Some(first) = first_set {
                if first != group_first_set {
                    return Err(crate::Error::InvalidInput(
                        "a pass cannot bind groups from layouts with different first set indices"
                            .into(),
                    ));
                }
            } else {
                first_set = Some(group_first_set);
            }
            sets.extend_from_slice(self.descriptor_sets(*handle)?);
        }
        if !sets.is_empty() {
            unsafe {
                device.cmd_bind_descriptor_sets(
                    command_buffer,
                    bind_point,
                    layout,
                    first_set.unwrap_or(0),
                    &sets,
                    &[],
                );
            }
        }
        Ok(())
    }

    /// GFX-7a: return the `DescriptorBufferBindingInfoEXT` for a bind group if it has
    /// descriptor buffer backing. Used by command recording to choose the bind path.
    pub fn descriptor_buffer_binding(
        &self,
        handle: BindGroupHandle,
    ) -> Option<(vk::DeviceAddress, vk::DeviceSize, u32)> {
        let bg = self.bind_groups.get(&handle)?;
        let db = bg.db_backing.as_ref()?;
        Some((db.device_address, db.size, bg.first_set))
    }

    /// GFX-7a: true when all bind groups in the list have descriptor buffer backing.
    pub fn all_have_descriptor_buffer(&self, handles: &[BindGroupHandle]) -> bool {
        !handles.is_empty()
            && handles.iter().all(|h| {
                self.bind_groups
                    .get(h)
                    .and_then(|bg| bg.db_backing.as_ref())
                    .is_some()
            })
    }

    pub fn destroy_all(&mut self, device: &Device) {
        // GFX-7a: free descriptor buffer backings before dropping bind groups.
        for (_, bg) in self.bind_groups.drain() {
            if let Some(db) = bg.db_backing {
                unsafe {
                    device.unmap_memory(db.memory);
                    device.destroy_buffer(db.buffer, None);
                    device.free_memory(db.memory, None);
                }
            }
        }
        for (_, mut slab) in self.pool_slabs.drain() {
            slab.destroy_all(device);
        }
        for (_, layout) in self.layouts.drain() {
            unsafe {
                device.destroy_pipeline_layout(layout.pipeline_layout, None);
            }
            let mut set_layouts = layout.owned_set_layouts;
            destroy_set_layouts(device, &mut set_layouts);
        }
    }

    pub fn create_bind_group(
        &mut self,
        device: &Device,
        handle: BindGroupHandle,
        desc: &BindGroupDesc,
        resources: &ResourceRegistry,
    ) -> Result<()> {
        let layout = self.layouts.get(&desc.layout).ok_or(Error::InvalidHandle)?;
        let set_layouts = layout.bind_group_set_layouts.clone();

        let slab = self
            .pool_slabs
            .get_mut(&desc.layout)
            .ok_or(Error::InvalidHandle)?;
        let (pool, sets) = slab.allocate(device, &set_layouts)?;

        for entry in &desc.entries {
            let binding = layout.bindings.get(&entry.path).ok_or_else(|| {
                Error::InvalidInput(format!(
                    "bind group entry path '{}' was not found in pipeline layout",
                    entry.path
                ))
            })?;
            let set = sets
                .get(binding.allocated_set_index)
                .copied()
                .ok_or(Error::InvalidHandle)?;
            write_descriptor(device, set, *binding, entry.resource, resources)?;
        }

        // GFX-7a: when descriptor_buffer is available and the layout is not bindless,
        // also create a buffer-backed copy of the descriptors.
        let db_backing = if let Some(db_ext) = &self.descriptor_buffer_ext {
            if !layout.uses_bindless && self.buffer_device_address_enabled {
                let set_layout = layout
                    .bind_group_set_layouts
                    .first()
                    .copied()
                    .unwrap_or(vk::DescriptorSetLayout::null());
                if set_layout != vk::DescriptorSetLayout::null() {
                    match create_descriptor_buffer_backing(
                        device,
                        db_ext,
                        set_layout,
                        desc,
                        &layout.bindings,
                        resources,
                    ) {
                        Ok(backing) => Some(backing),
                        Err(e) => {
                            // Non-fatal: fall back to pool-based path silently.
                            tracing::error!(
                                "descriptor buffer backing creation failed (using pool fallback): {e}"
                            );
                            None
                        }
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        self.bind_groups.insert(
            handle,
            VulkanBindGroup {
                layout: desc.layout,
                first_set: layout.bind_group_first_set,
                pool,
                sets,
                db_backing,
            },
        );
        Ok(())
    }

    pub fn destroy_bind_group(&mut self, device: &Device, handle: BindGroupHandle) -> Result<()> {
        let bind_group = self
            .bind_groups
            .remove(&handle)
            .ok_or(Error::InvalidHandle)?;
        // Return the sets to the layout's pool slab rather than destroying the pool.
        if let Some(slab) = self.pool_slabs.get_mut(&bind_group.layout) {
            slab.free(device, bind_group.pool, &bind_group.sets);
        }
        // GFX-7a: free descriptor buffer backing if present.
        if let Some(db) = bind_group.db_backing {
            unsafe {
                device.unmap_memory(db.memory);
                device.destroy_buffer(db.buffer, None);
                device.free_memory(db.memory, None);
            }
        }
        Ok(())
    }

    pub fn pipeline_layout(&self, handle: PipelineLayoutHandle) -> Result<vk::PipelineLayout> {
        self.layouts
            .get(&handle)
            .map(|layout| layout.pipeline_layout)
            .ok_or(Error::InvalidHandle)
    }

    pub fn push_constants_bytes(&self, handle: PipelineLayoutHandle) -> Result<u32> {
        self.layouts
            .get(&handle)
            .map(|layout| layout.push_constants_bytes)
            .ok_or(Error::InvalidHandle)
    }

    pub fn push_constant_stages(
        &self,
        handle: PipelineLayoutHandle,
    ) -> Result<vk::ShaderStageFlags> {
        self.layouts
            .get(&handle)
            .map(|layout| layout.push_constant_stages)
            .ok_or(Error::InvalidHandle)
    }

    pub fn shader_object_layout_info(
        &self,
        handle: PipelineLayoutHandle,
    ) -> Result<(Vec<vk::DescriptorSetLayout>, Vec<vk::PushConstantRange>)> {
        let layout = self.layouts.get(&handle).ok_or(Error::InvalidHandle)?;
        let push_ranges = if layout.push_constants_bytes == 0 {
            Vec::new()
        } else {
            vec![
                vk::PushConstantRange::default()
                    .stage_flags(layout.push_constant_stages)
                    .offset(0)
                    .size(layout.push_constants_bytes),
            ]
        };
        Ok((layout.set_layouts.clone(), push_ranges))
    }

    pub fn descriptor_sets(&self, handle: BindGroupHandle) -> Result<&[vk::DescriptorSet]> {
        self.bind_groups
            .get(&handle)
            .map(|group| group.sets.as_slice())
            .ok_or(Error::InvalidHandle)
    }

    pub fn bind_group_first_set(&self, handle: BindGroupHandle) -> Result<u32> {
        self.bind_groups
            .get(&handle)
            .map(|group| group.first_set)
            .ok_or(Error::InvalidHandle)
    }

    pub fn pipeline_uses_bindless(&self, handle: PipelineLayoutHandle) -> Result<bool> {
        self.layouts
            .get(&handle)
            .map(|layout| layout.uses_bindless)
            .ok_or(Error::InvalidHandle)
    }
}

impl VulkanBindGroup {
    fn references_layout(
        &self,
        layout: PipelineLayoutHandle,
        _layouts: &HashMap<PipelineLayoutHandle, VulkanPipelineLayout>,
    ) -> bool {
        self.layout == layout
    }
}

#[derive(Default)]
struct DescriptorCounts {
    samplers: u32,
    sampled_images: u32,
    storage_images: u32,
    uniform_buffers: u32,
    storage_buffers: u32,
}

impl DescriptorCounts {
    fn add(&mut self, kind: BindingKind, count: u32) {
        match kind {
            BindingKind::Sampler => self.samplers = self.samplers.saturating_add(count),
            BindingKind::SampledImage => {
                self.sampled_images = self.sampled_images.saturating_add(count)
            }
            BindingKind::StorageImage => {
                self.storage_images = self.storage_images.saturating_add(count)
            }
            BindingKind::UniformBuffer => {
                self.uniform_buffers = self.uniform_buffers.saturating_add(count)
            }
            BindingKind::StorageBuffer => {
                self.storage_buffers = self.storage_buffers.saturating_add(count)
            }
            BindingKind::AccelerationStructure => {}
        }
    }
}

fn validate_pipeline_layout(
    layout: &CanonicalPipelineLayout,
    bindless_supported: bool,
    limits: &Limits,
) -> Result<()> {
    if layout.push_constants_bytes > limits.max_push_constants_size {
        return Err(Error::InvalidInput(format!(
            "pipeline layout push constants use {} bytes, exceeding backend limit {}",
            layout.push_constants_bytes, limits.max_push_constants_size
        )));
    }
    if layout.groups.len() as u32 > limits.max_bound_descriptor_sets {
        return Err(Error::InvalidInput(format!(
            "pipeline layout uses {} descriptor sets, exceeding backend limit {}",
            layout.groups.len(),
            limits.max_bound_descriptor_sets
        )));
    }

    let mut set_totals = DescriptorCounts::default();
    let mut stage_totals = [
        DescriptorCounts::default(),
        DescriptorCounts::default(),
        DescriptorCounts::default(),
        DescriptorCounts::default(),
        DescriptorCounts::default(),
        DescriptorCounts::default(),
    ];

    for (set_index, group) in layout.groups.iter().enumerate() {
        let is_bindless = group_is_bindless(group);
        if is_bindless {
            if set_index != 0 {
                return Err(Error::InvalidInput(
                    "bindless descriptor group must be reflected at set 0".into(),
                ));
            }
            if !bindless_supported {
                return Err(Error::Unsupported(
                    "pipeline layout uses bindless descriptors, but bindless is not supported by this device",
                ));
            }
            validate_bindless_group(group)?;
            continue;
        }

        for binding in &group.bindings {
            let count = binding.count.max(1);
            set_totals.add(binding.kind, count);
            for stage_index in stage_indices(binding.stage_mask) {
                stage_totals[stage_index].add(binding.kind, count);
            }
        }
    }

    validate_descriptor_counts("descriptor set", &set_totals, limits, false)?;
    for (stage_index, counts) in stage_totals.iter().enumerate() {
        validate_descriptor_counts(stage_label(stage_index), counts, limits, true)?;
    }
    Ok(())
}

fn validate_descriptor_counts(
    scope: &str,
    counts: &DescriptorCounts,
    limits: &Limits,
    per_stage: bool,
) -> Result<()> {
    let limit = |set_limit: u32, stage_limit: u32| {
        if per_stage { stage_limit } else { set_limit }
    };
    validate_descriptor_count(
        scope,
        "samplers",
        counts.samplers,
        limit(
            limits.max_descriptor_set_samplers,
            limits.max_per_stage_samplers,
        ),
    )?;
    validate_descriptor_count(
        scope,
        "sampled images",
        counts.sampled_images,
        limit(
            limits.max_descriptor_set_sampled_images,
            limits.max_per_stage_sampled_images,
        ),
    )?;
    validate_descriptor_count(
        scope,
        "storage images",
        counts.storage_images,
        limit(
            limits.max_descriptor_set_storage_images,
            limits.max_per_stage_storage_images,
        ),
    )?;
    validate_descriptor_count(
        scope,
        "uniform buffers",
        counts.uniform_buffers,
        limit(
            limits.max_descriptor_set_uniform_buffers,
            limits.max_per_stage_uniform_buffers,
        ),
    )?;
    validate_descriptor_count(
        scope,
        "storage buffers",
        counts.storage_buffers,
        limit(
            limits.max_descriptor_set_storage_buffers,
            limits.max_per_stage_storage_buffers,
        ),
    )
}

fn validate_descriptor_count(scope: &str, label: &str, count: u32, limit: u32) -> Result<()> {
    if count > limit {
        return Err(Error::InvalidInput(format!(
            "pipeline layout uses {count} {label} in {scope}, exceeding backend limit {limit}"
        )));
    }
    Ok(())
}

fn stage_indices(mask: StageMask) -> impl Iterator<Item = usize> {
    [
        StageMask::VERTEX,
        StageMask::FRAGMENT,
        StageMask::COMPUTE,
        StageMask::MESH,
        StageMask::TASK,
        StageMask::RAY_TRACING,
    ]
    .into_iter()
    .enumerate()
    .filter_map(move |(index, stage)| {
        if mask.0 & stage.0 != 0 {
            Some(index)
        } else {
            None
        }
    })
}

fn stage_label(index: usize) -> &'static str {
    match index {
        0 => "vertex stage",
        1 => "fragment stage",
        2 => "compute stage",
        3 => "mesh stage",
        4 => "task stage",
        5 => "ray tracing stage",
        _ => "unknown stage",
    }
}

fn group_is_bindless(group: &CanonicalGroupLayout) -> bool {
    group
        .bindings
        .iter()
        .any(|binding| binding.count == BINDLESS_COUNT)
}

fn validate_bindless_group(group: &CanonicalGroupLayout) -> Result<()> {
    for binding in &group.bindings {
        if binding.count != BINDLESS_COUNT {
            return Err(Error::InvalidInput(format!(
                "bindless descriptor set cannot mix finite binding '{}' with unbounded arrays",
                binding.path
            )));
        }
        let expected_binding = match binding.kind {
            BindingKind::Sampler => BINDLESS_SAMPLER_BINDING,
            BindingKind::SampledImage => BINDLESS_SAMPLED_IMAGE_BINDING,
            BindingKind::StorageImage => BINDLESS_STORAGE_IMAGE_BINDING,
            BindingKind::StorageBuffer => BINDLESS_STORAGE_BUFFER_BINDING,
            BindingKind::UniformBuffer | BindingKind::AccelerationStructure => {
                return Err(Error::InvalidInput(format!(
                    "binding '{}' uses {:?}, which is not supported by the Vulkan bindless heap",
                    binding.path, binding.kind
                )));
            }
        };
        if binding.binding != expected_binding {
            return Err(Error::InvalidInput(format!(
                "bindless {:?} '{}' must use Vulkan binding {}, got {}",
                binding.kind, binding.path, expected_binding, binding.binding
            )));
        }
    }
    Ok(())
}

fn descriptor_set_layout_binding(
    binding_index: u32,
    binding: &CanonicalBinding,
) -> vk::DescriptorSetLayoutBinding<'static> {
    vk::DescriptorSetLayoutBinding::default()
        .binding(binding_index)
        .descriptor_type(descriptor_type(binding.kind))
        .descriptor_count(binding.count.max(1))
        .stage_flags(shader_stage_flags(binding.stage_mask))
}

fn descriptor_type(kind: BindingKind) -> vk::DescriptorType {
    match kind {
        BindingKind::SampledImage => vk::DescriptorType::SAMPLED_IMAGE,
        BindingKind::StorageImage => vk::DescriptorType::STORAGE_IMAGE,
        BindingKind::UniformBuffer => vk::DescriptorType::UNIFORM_BUFFER,
        BindingKind::StorageBuffer => vk::DescriptorType::STORAGE_BUFFER,
        BindingKind::Sampler => vk::DescriptorType::SAMPLER,
        BindingKind::AccelerationStructure => vk::DescriptorType::ACCELERATION_STRUCTURE_KHR,
    }
}

fn shader_stage_flags(mask: StageMask) -> vk::ShaderStageFlags {
    if mask == StageMask::ALL {
        return vk::ShaderStageFlags::ALL;
    }

    let mut flags = vk::ShaderStageFlags::empty();
    if mask.0 & StageMask::VERTEX.0 != 0 {
        flags |= vk::ShaderStageFlags::VERTEX;
    }
    if mask.0 & StageMask::FRAGMENT.0 != 0 {
        flags |= vk::ShaderStageFlags::FRAGMENT;
    }
    if mask.0 & StageMask::COMPUTE.0 != 0 {
        flags |= vk::ShaderStageFlags::COMPUTE;
    }
    if mask.0 & StageMask::MESH.0 != 0 {
        flags |= vk::ShaderStageFlags::MESH_EXT;
    }
    if mask.0 & StageMask::TASK.0 != 0 {
        flags |= vk::ShaderStageFlags::TASK_EXT;
    }
    if mask.0 & StageMask::RAY_TRACING.0 != 0 {
        flags |= vk::ShaderStageFlags::RAYGEN_KHR
            | vk::ShaderStageFlags::MISS_KHR
            | vk::ShaderStageFlags::CLOSEST_HIT_KHR;
    }
    if flags.is_empty() {
        vk::ShaderStageFlags::ALL
    } else {
        flags
    }
}

fn destroy_set_layouts(device: &Device, set_layouts: &mut Vec<vk::DescriptorSetLayout>) {
    for layout in set_layouts.drain(..) {
        unsafe {
            device.destroy_descriptor_set_layout(layout, None);
        }
    }
}

fn write_descriptor(
    device: &Device,
    set: vk::DescriptorSet,
    binding: VulkanBinding,
    resource: ResourceBinding,
    resources: &ResourceRegistry,
) -> Result<()> {
    match resource {
        ResourceBinding::Buffer(buffer) => {
            if !matches!(
                binding.descriptor_type,
                vk::DescriptorType::UNIFORM_BUFFER | vk::DescriptorType::STORAGE_BUFFER
            ) {
                return Err(Error::InvalidInput(
                    "buffer resource can only be bound to buffer descriptors".into(),
                ));
            }
            let info = [vk::DescriptorBufferInfo::default()
                .buffer(resources.buffer(buffer)?)
                .offset(0)
                .range(vk::WHOLE_SIZE)];
            let write = [vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(binding.binding_index)
                .descriptor_type(binding.descriptor_type)
                .buffer_info(&info)];
            unsafe {
                device.update_descriptor_sets(&write, &[]);
            }
        }
        ResourceBinding::Image(image) => {
            if !matches!(
                binding.descriptor_type,
                vk::DescriptorType::SAMPLED_IMAGE | vk::DescriptorType::STORAGE_IMAGE
            ) {
                return Err(Error::InvalidInput(
                    "image resource can only be bound to image descriptors".into(),
                ));
            }
            let info = [vk::DescriptorImageInfo::default()
                .image_view(resources.image_view(image)?)
                .image_layout(image_descriptor_layout(binding.descriptor_type))];
            let write = [vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(binding.binding_index)
                .descriptor_type(binding.descriptor_type)
                .image_info(&info)];
            unsafe {
                device.update_descriptor_sets(&write, &[]);
            }
        }
        ResourceBinding::ImageView { image, subresource } => {
            if !matches!(
                binding.descriptor_type,
                vk::DescriptorType::SAMPLED_IMAGE | vk::DescriptorType::STORAGE_IMAGE
            ) {
                return Err(Error::InvalidInput(
                    "image resource can only be bound to image descriptors".into(),
                ));
            }
            let info = [vk::DescriptorImageInfo::default()
                .image_view(resources.image_view_for_subresource(device, image, subresource)?)
                .image_layout(image_descriptor_layout(binding.descriptor_type))];
            let write = [vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(binding.binding_index)
                .descriptor_type(binding.descriptor_type)
                .image_info(&info)];
            unsafe {
                device.update_descriptor_sets(&write, &[]);
            }
        }
        ResourceBinding::Sampler(sampler) => {
            if binding.descriptor_type != vk::DescriptorType::SAMPLER {
                return Err(Error::InvalidInput(
                    "sampler resource can only be bound to sampler descriptors".into(),
                ));
            }
            let info = [vk::DescriptorImageInfo::default().sampler(resources.sampler(sampler)?)];
            let write = [vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(binding.binding_index)
                .descriptor_type(binding.descriptor_type)
                .image_info(&info)];
            unsafe {
                device.update_descriptor_sets(&write, &[]);
            }
        }
        ResourceBinding::AccelerationStructure(acceleration_structure) => {
            if binding.descriptor_type != vk::DescriptorType::ACCELERATION_STRUCTURE_KHR {
                return Err(Error::InvalidInput(
                    "acceleration structure resource can only be bound to acceleration structure descriptors".into(),
                ));
            }
            let as_handle = resources.acceleration_structure(acceleration_structure)?;
            let as_handles = [as_handle];
            let as_info = vk::WriteDescriptorSetAccelerationStructureKHR::default()
                .acceleration_structures(&as_handles);
            let mut write = vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(binding.binding_index)
                .descriptor_type(binding.descriptor_type)
                .descriptor_count(1);
            write.p_next = (&as_info as *const vk::WriteDescriptorSetAccelerationStructureKHR<'_>)
                .cast::<std::ffi::c_void>();
            let write = [write];
            unsafe {
                device.update_descriptor_sets(&write, &[]);
            }
        }
    }
    Ok(())
}

fn image_descriptor_layout(descriptor_type: vk::DescriptorType) -> vk::ImageLayout {
    match descriptor_type {
        vk::DescriptorType::STORAGE_IMAGE => vk::ImageLayout::GENERAL,
        _ => vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
    }
}

/// GFX-7a: Allocate and fill a HOST_VISIBLE descriptor buffer for one bind group.
fn create_descriptor_buffer_backing(
    device: &Device,
    db_ext: &ash::ext::descriptor_buffer::Device,
    set_layout: vk::DescriptorSetLayout,
    desc: &crate::BindGroupDesc,
    bindings: &std::collections::HashMap<String, VulkanBinding>,
    resources: &ResourceRegistry,
) -> crate::Result<DescriptorBufferBacking> {
    let size = unsafe { db_ext.get_descriptor_set_layout_size(set_layout) };
    if size == 0 {
        return Err(crate::Error::Backend(
            "descriptor set layout size is 0".into(),
        ));
    }

    // Create a HOST_VISIBLE + HOST_COHERENT buffer with RESOURCE_DESCRIPTOR_BUFFER + SHADER_DEVICE_ADDRESS.
    let buf_info = vk::BufferCreateInfo::default()
        .size(size)
        .usage(
            vk::BufferUsageFlags::RESOURCE_DESCRIPTOR_BUFFER_EXT
                | vk::BufferUsageFlags::SAMPLER_DESCRIPTOR_BUFFER_EXT
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
        )
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    let buffer = unsafe {
        device.create_buffer(&buf_info, None).map_err(|e| {
            crate::Error::Backend(format!("descriptor buffer creation failed: {e:?}"))
        })?
    };

    let req = unsafe { device.get_buffer_memory_requirements(buffer) };
    // Attempt any compatible memory type (HOST_VISIBLE is guaranteed for descriptor buffers
    // when the buffer usage includes SAMPLER_DESCRIPTOR_BUFFER or RESOURCE_DESCRIPTOR_BUFFER).
    let mut memory_type = u32::MAX;
    for i in 0..32u32 {
        if (req.memory_type_bits & (1 << i)) != 0 {
            memory_type = i;
            break;
        }
    }
    if memory_type == u32::MAX {
        unsafe { device.destroy_buffer(buffer, None) };
        return Err(crate::Error::Backend(
            "no compatible memory type for descriptor buffer".into(),
        ));
    }

    let mut alloc_flags_info =
        vk::MemoryAllocateFlagsInfo::default().flags(vk::MemoryAllocateFlags::DEVICE_ADDRESS);
    let alloc_info = vk::MemoryAllocateInfo::default()
        .allocation_size(req.size)
        .memory_type_index(memory_type)
        .push(&mut alloc_flags_info);
    let memory = unsafe {
        device.allocate_memory(&alloc_info, None).map_err(|e| {
            device.destroy_buffer(buffer, None);
            crate::Error::Backend(format!("descriptor buffer memory allocation failed: {e:?}"))
        })?
    };
    unsafe {
        device.bind_buffer_memory(buffer, memory, 0).map_err(|e| {
            device.free_memory(memory, None);
            device.destroy_buffer(buffer, None);
            crate::Error::Backend(format!("descriptor buffer bind_memory failed: {e:?}"))
        })?;
    }
    let mapped_ptr = unsafe {
        device
            .map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
            .map_err(|e| {
                device.free_memory(memory, None);
                device.destroy_buffer(buffer, None);
                crate::Error::Backend(format!("descriptor buffer map_memory failed: {e:?}"))
            })? as *mut u8
    };
    let device_address = unsafe {
        device.get_buffer_device_address(&vk::BufferDeviceAddressInfo::default().buffer(buffer))
    };

    // Zero the entire buffer (unbound descriptors are safe to read when zeroed).
    unsafe { std::ptr::write_bytes(mapped_ptr, 0u8, size as usize) };

    // Write each descriptor entry into the buffer at its layout-computed offset.
    for entry in &desc.entries {
        let binding = match bindings.get(&entry.path) {
            Some(b) => b,
            None => continue,
        };
        let binding_offset = unsafe {
            db_ext.get_descriptor_set_layout_binding_offset(set_layout, binding.binding_index)
        };
        let dst = unsafe { mapped_ptr.add(binding_offset as usize) };
        write_descriptor_to_buffer(device, db_ext, dst, *binding, entry.resource, resources);
    }

    Ok(DescriptorBufferBacking {
        buffer,
        memory,
        mapped_ptr,
        device_address,
        size,
    })
}

/// GFX-7a: Write a single resource descriptor into mapped host memory via `vkGetDescriptorEXT`.
fn write_descriptor_to_buffer(
    device: &Device,
    db_ext: &ash::ext::descriptor_buffer::Device,
    dst: *mut u8,
    binding: VulkanBinding,
    resource: crate::ResourceBinding,
    resources: &ResourceRegistry,
) {
    // Query per-type descriptor size to know how many bytes to write.
    // Use a 256-byte buffer — no descriptor type exceeds this in practice.
    const MAX_DESC_SIZE: usize = 256;
    let mut desc_bytes = [0u8; MAX_DESC_SIZE];

    match resource {
        crate::ResourceBinding::Buffer(buf_handle) => {
            let buf = match resources.buffer(buf_handle) {
                Ok(b) => b,
                Err(_) => return,
            };
            let addr_info = vk::DescriptorAddressInfoEXT::default()
                .address(unsafe {
                    device.get_buffer_device_address(
                        &vk::BufferDeviceAddressInfo::default().buffer(buf),
                    )
                })
                .range(vk::WHOLE_SIZE);
            let data = vk::DescriptorDataEXT {
                p_storage_buffer: &addr_info,
            };
            let info = vk::DescriptorGetInfoEXT::default()
                .ty(binding.descriptor_type)
                .data(data);
            unsafe { db_ext.get_descriptor(&info, &mut desc_bytes) };
        }
        crate::ResourceBinding::Image(img_handle) => {
            let view = match resources.image_view(img_handle) {
                Ok(v) => v,
                Err(_) => return,
            };
            let image_info = vk::DescriptorImageInfo::default()
                .image_view(view)
                .image_layout(image_descriptor_layout(binding.descriptor_type));
            let data = if binding.descriptor_type == vk::DescriptorType::STORAGE_IMAGE {
                vk::DescriptorDataEXT {
                    p_storage_image: &image_info,
                }
            } else {
                vk::DescriptorDataEXT {
                    p_sampled_image: &image_info,
                }
            };
            let info = vk::DescriptorGetInfoEXT::default()
                .ty(binding.descriptor_type)
                .data(data);
            unsafe { db_ext.get_descriptor(&info, &mut desc_bytes) };
        }
        crate::ResourceBinding::Sampler(sampler_handle) => {
            let sampler = match resources.sampler(sampler_handle) {
                Ok(s) => s,
                Err(_) => return,
            };
            let data = vk::DescriptorDataEXT {
                p_sampler: &sampler,
            };
            let info = vk::DescriptorGetInfoEXT::default()
                .ty(vk::DescriptorType::SAMPLER)
                .data(data);
            unsafe { db_ext.get_descriptor(&info, &mut desc_bytes) };
        }
        _ => return,
    }
    // Copy the written bytes to the mapped buffer. The size used by get_descriptor
    // depends on the type; since we zeroed the buffer, unused trailing bytes are safe.
    unsafe { std::ptr::copy_nonoverlapping(desc_bytes.as_ptr(), dst, MAX_DESC_SIZE) };
}

#[cfg(test)]
#[path = "descriptors_tests.rs"]
mod tests;
