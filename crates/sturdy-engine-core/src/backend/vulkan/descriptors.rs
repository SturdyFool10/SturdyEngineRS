use std::collections::HashMap;

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

#[derive(Default)]
pub struct DescriptorRegistry {
    layouts: HashMap<PipelineLayoutHandle, VulkanPipelineLayout>,
    bind_groups: HashMap<BindGroupHandle, VulkanBindGroup>,
    /// Per-layout pool slabs. One slab per unique pipeline layout; each slab
    /// holds pages of VkDescriptorPool that are reused across many bind groups.
    pool_slabs: HashMap<PipelineLayoutHandle, LayoutPoolSlab>,
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
}

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
        device
            .create_descriptor_pool(&info, None)
            .map_err(|error| Error::Backend(format!("vkCreateDescriptorPool failed: {error:?}")))
    }
}

impl DescriptorRegistry {
    pub fn create_pipeline_layout(
        &mut self,
        device: &Device,
        handle: PipelineLayoutHandle,
        layout: &CanonicalPipelineLayout,
        bindless_set_layout: Option<vk::DescriptorSetLayout>,
        limits: &Limits,
    ) -> Result<()> {
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
        // Pre-register the pool slab so it's ready on first bind-group creation.
        //panic allowed, reason = "layout was inserted into layouts map immediately above"
        let layout = self.layouts.get(&handle).unwrap();
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

    pub fn destroy_all(&mut self, device: &Device) {
        for (_, mut slab) in self.pool_slabs.drain() {
            slab.destroy_all(device);
        }
        self.bind_groups.clear(); // pools already gone, drop handles
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

        self.bind_groups.insert(
            handle,
            VulkanBindGroup {
                layout: desc.layout,
                first_set: layout.bind_group_first_set,
                pool,
                sets,
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
    }
    Ok(())
}

fn image_descriptor_layout(descriptor_type: vk::DescriptorType) -> vk::ImageLayout {
    match descriptor_type {
        vk::DescriptorType::STORAGE_IMAGE => vk::ImageLayout::GENERAL,
        _ => vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CanonicalGroupLayout, UpdateRate};

    fn binding(kind: BindingKind, count: u32, stage_mask: StageMask) -> CanonicalBinding {
        CanonicalBinding {
            path: format!("{kind:?}"),
            kind,
            count,
            stage_mask,
            update_rate: UpdateRate::Draw,
            binding: 0,
        }
    }

    fn layout_with(bindings: Vec<CanonicalBinding>) -> CanonicalPipelineLayout {
        CanonicalPipelineLayout {
            groups: vec![CanonicalGroupLayout {
                name: "set0".into(),
                bindings,
            }],
            push_constants_bytes: 0,
            push_constants_stage_mask: StageMask::default(),
        }
    }

    #[test]
    fn validates_push_constant_limit() {
        let mut layout = CanonicalPipelineLayout::default();
        layout.push_constants_bytes = 256;
        let limits = Limits {
            max_push_constants_size: 128,
            ..Limits::default()
        };

        assert!(matches!(
            validate_pipeline_layout(&layout, false, &limits),
            Err(Error::InvalidInput(_))
        ));
    }

    #[test]
    fn validates_per_stage_descriptor_limit() {
        let layout = layout_with(vec![binding(
            BindingKind::SampledImage,
            3,
            StageMask::FRAGMENT,
        )]);
        let limits = Limits {
            max_per_stage_sampled_images: 2,
            max_descriptor_set_sampled_images: 16,
            ..Limits::default()
        };

        assert!(matches!(
            validate_pipeline_layout(&layout, false, &limits),
            Err(Error::InvalidInput(_))
        ));
    }

    #[test]
    fn bindless_set_requires_bindless_support() {
        let layout = layout_with(vec![binding(
            BindingKind::SampledImage,
            BINDLESS_COUNT,
            StageMask::FRAGMENT,
        )]);

        assert!(matches!(
            validate_pipeline_layout(&layout, false, &Limits::default()),
            Err(Error::Unsupported(_))
        ));
    }

    #[test]
    fn bindless_set_is_exempt_from_finite_descriptor_counts() {
        let mut bindless = binding(
            BindingKind::SampledImage,
            BINDLESS_COUNT,
            StageMask::FRAGMENT,
        );
        bindless.binding = BINDLESS_SAMPLED_IMAGE_BINDING;
        let layout = layout_with(vec![bindless]);
        let limits = Limits {
            max_per_stage_sampled_images: 1,
            max_descriptor_set_sampled_images: 1,
            ..Limits::default()
        };

        assert!(validate_pipeline_layout(&layout, true, &limits).is_ok());
    }

    #[test]
    fn bindless_set_validates_heap_binding_slots() {
        let mut bad_binding = binding(
            BindingKind::SampledImage,
            BINDLESS_COUNT,
            StageMask::FRAGMENT,
        );
        bad_binding.binding = 7;
        let layout = layout_with(vec![bad_binding]);

        assert!(matches!(
            validate_pipeline_layout(&layout, true, &Limits::default()),
            Err(Error::InvalidInput(_))
        ));
    }

    #[test]
    fn bindless_set_rejects_mixed_finite_bindings() {
        let mut finite = binding(BindingKind::Sampler, 1, StageMask::FRAGMENT);
        finite.binding = BINDLESS_SAMPLER_BINDING;
        let mut bindless = binding(
            BindingKind::SampledImage,
            BINDLESS_COUNT,
            StageMask::FRAGMENT,
        );
        bindless.binding = BINDLESS_SAMPLED_IMAGE_BINDING;
        let layout = layout_with(vec![finite, bindless]);

        assert!(matches!(
            validate_pipeline_layout(&layout, true, &Limits::default()),
            Err(Error::InvalidInput(_))
        ));
    }
}
