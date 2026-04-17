use std::collections::HashMap;

use ash::{Device, vk};

use crate::{
    BindGroupDesc, BindGroupHandle, BindingKind, CanonicalBinding, CanonicalPipelineLayout, Error,
    PipelineLayoutHandle, ResourceBinding, Result, StageMask,
};

use super::resources::ResourceRegistry;

#[derive(Default)]
pub struct DescriptorRegistry {
    layouts: HashMap<PipelineLayoutHandle, VulkanPipelineLayout>,
    bind_groups: HashMap<BindGroupHandle, VulkanBindGroup>,
}

struct VulkanPipelineLayout {
    pipeline_layout: vk::PipelineLayout,
    set_layouts: Vec<vk::DescriptorSetLayout>,
    bindings: HashMap<String, VulkanBinding>,
    pool_sizes: Vec<vk::DescriptorPoolSize>,
}

#[derive(Copy, Clone)]
struct VulkanBinding {
    set_index: usize,
    binding_index: u32,
    descriptor_type: vk::DescriptorType,
}

struct VulkanBindGroup {
    layout: PipelineLayoutHandle,
    pool: vk::DescriptorPool,
    sets: Vec<vk::DescriptorSet>,
}

impl DescriptorRegistry {
    pub fn create_pipeline_layout(
        &mut self,
        device: &Device,
        handle: PipelineLayoutHandle,
        layout: &CanonicalPipelineLayout,
    ) -> Result<()> {
        let mut set_layouts = Vec::with_capacity(layout.groups.len());
        let mut binding_map = HashMap::new();
        let mut pool_counts: HashMap<vk::DescriptorType, u32> = HashMap::new();

        for (set_index, group) in layout.groups.iter().enumerate() {
            let bindings = group
                .bindings
                .iter()
                .enumerate()
                .map(|(index, binding)| {
                    let binding_index = index as u32;
                    let descriptor_type = descriptor_type(binding.kind);
                    binding_map.insert(
                        binding.path.clone(),
                        VulkanBinding {
                            set_index,
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
                        destroy_set_layouts(device, &mut set_layouts);
                        return Err(Error::Backend(format!(
                            "vkCreateDescriptorSetLayout failed: {error:?}"
                        )));
                    }
                }
            };
            set_layouts.push(set_layout);
        }

        let push_ranges = if layout.push_constants_bytes == 0 {
            Vec::new()
        } else {
            vec![
                vk::PushConstantRange::default()
                    .stage_flags(vk::ShaderStageFlags::ALL)
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
                    destroy_set_layouts(device, &mut set_layouts);
                    return Err(Error::Backend(format!(
                        "vkCreatePipelineLayout failed: {error:?}"
                    )));
                }
            }
        };

        self.layouts.insert(
            handle,
            VulkanPipelineLayout {
                pipeline_layout,
                set_layouts,
                bindings: binding_map,
                pool_sizes: pool_counts
                    .into_iter()
                    .map(|(ty, descriptor_count)| vk::DescriptorPoolSize {
                        ty,
                        descriptor_count,
                    })
                    .collect(),
            },
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
        let mut set_layouts = layout.set_layouts;
        destroy_set_layouts(device, &mut set_layouts);
        Ok(())
    }

    pub fn destroy_all(&mut self, device: &Device) {
        for (_, bind_group) in self.bind_groups.drain() {
            unsafe {
                device.destroy_descriptor_pool(bind_group.pool, None);
            }
        }
        for (_, layout) in self.layouts.drain() {
            unsafe {
                device.destroy_pipeline_layout(layout.pipeline_layout, None);
            }
            let mut set_layouts = layout.set_layouts;
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
        let pool_sizes = if layout.pool_sizes.is_empty() {
            vec![vk::DescriptorPoolSize {
                ty: vk::DescriptorType::UNIFORM_BUFFER,
                descriptor_count: 1,
            }]
        } else {
            layout.pool_sizes.clone()
        };
        let pool_info = vk::DescriptorPoolCreateInfo::default()
            .max_sets(layout.set_layouts.len().max(1) as u32)
            .pool_sizes(&pool_sizes);
        let pool = unsafe {
            device
                .create_descriptor_pool(&pool_info, None)
                .map_err(|error| {
                    Error::Backend(format!("vkCreateDescriptorPool failed: {error:?}"))
                })?
        };

        let sets = if layout.set_layouts.is_empty() {
            Vec::new()
        } else {
            let allocate_info = vk::DescriptorSetAllocateInfo::default()
                .descriptor_pool(pool)
                .set_layouts(&layout.set_layouts);
            unsafe {
                match device.allocate_descriptor_sets(&allocate_info) {
                    Ok(sets) => sets,
                    Err(error) => {
                        device.destroy_descriptor_pool(pool, None);
                        return Err(Error::Backend(format!(
                            "vkAllocateDescriptorSets failed: {error:?}"
                        )));
                    }
                }
            }
        };

        for entry in &desc.entries {
            let binding = layout.bindings.get(&entry.path).ok_or_else(|| {
                Error::InvalidInput(format!(
                    "bind group entry path '{}' was not found in pipeline layout",
                    entry.path
                ))
            })?;
            let set = sets
                .get(binding.set_index)
                .copied()
                .ok_or(Error::InvalidHandle)?;
            write_descriptor(device, set, *binding, entry.resource, resources)?;
        }

        self.bind_groups.insert(
            handle,
            VulkanBindGroup {
                layout: desc.layout,
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
        unsafe {
            device.destroy_descriptor_pool(bind_group.pool, None);
        }
        Ok(())
    }

    pub fn pipeline_layout(&self, handle: PipelineLayoutHandle) -> Result<vk::PipelineLayout> {
        self.layouts
            .get(&handle)
            .map(|layout| layout.pipeline_layout)
            .ok_or(Error::InvalidHandle)
    }

    pub fn descriptor_sets(&self, handle: BindGroupHandle) -> Result<&[vk::DescriptorSet]> {
        self.bind_groups
            .get(&handle)
            .map(|group| group.sets.as_slice())
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
    }
    Ok(())
}

fn image_descriptor_layout(descriptor_type: vk::DescriptorType) -> vk::ImageLayout {
    match descriptor_type {
        vk::DescriptorType::STORAGE_IMAGE => vk::ImageLayout::GENERAL,
        _ => vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
    }
}
