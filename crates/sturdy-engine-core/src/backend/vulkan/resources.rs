use std::{collections::HashMap, ffi::c_void, sync::Mutex};

use ash::vk::TaggedStructure;
use ash::{Device, vk};
use vk::Handle;

use crate::shader_object::ShaderObjectHandle;
use crate::{
    AccelerationStructureHandle, AccelerationStructureKind, AddressMode, BorderColor, BufferDesc,
    BufferHandle, BufferUsage, CompareOp, Error, FilterMode, Format, ImageCompression, ImageDesc,
    ImageHandle, ImageUsage, MipmapMode, PipelineLayoutHandle, Result, SamplerDesc, SamplerHandle,
    SamplerReductionMode, SubresourceRange, VulkanExternalBuffer, VulkanExternalImage,
};

use super::allocator::{Allocation, AllocatorStats, GpuAllocator};

pub struct VulkanAccelerationStructure {
    pub acceleration_structure: vk::AccelerationStructureKHR,
    pub buffer: vk::Buffer,
    pub allocation: Allocation,
    pub size: u64,
    pub kind: AccelerationStructureKind,
}

pub struct VulkanScratchBuffer {
    pub buffer: vk::Buffer,
    pub allocation: Allocation,
}

pub struct ResourceRegistry {
    allocator: GpuAllocator,
    images: HashMap<ImageHandle, VulkanImage>,
    image_views: Mutex<HashMap<ImageViewKey, vk::ImageView>>,
    buffers: HashMap<BufferHandle, VulkanBuffer>,
    /// GFX-7b: stores `SamplerDesc` alongside the handle so heap writes can reconstruct `SamplerCreateInfo`.
    samplers: HashMap<SamplerHandle, (vk::Sampler, crate::SamplerDesc)>,
    acceleration_structures: HashMap<AccelerationStructureHandle, VulkanAccelerationStructure>,
    /// VK_EXT_shader_object handles, keyed by engine handle.
    shader_objects: HashMap<ShaderObjectHandle, VulkanShaderObject>,
    /// VK_EXT_image_view_min_lod available; enables min_lod clamping on image views.
    pub image_view_min_lod_enabled: bool,
    /// VK_EXT_image_compression_control available; enables explicit compression hints.
    pub image_compression_control_enabled: bool,
    /// VK_NV_optical_flow available; enables optical-flow image usage pNext metadata.
    pub optical_flow_enabled: bool,
    /// VK_EXT_pageable_device_local_memory extension device for dynamic priority setting.
    pub pageable_memory_ext: Option<ash::ext::pageable_device_local_memory::Device>,
    /// VK_KHR_dedicated_allocation (Vulkan 1.1 core) — use dedicated VkDeviceMemory
    /// for resources where `prefersDedicatedAllocation` is true or size > 64 MiB.
    pub dedicated_allocation_enabled: bool,
}

#[derive(Copy, Clone)]
pub struct VulkanShaderObject {
    pub shader: vk::ShaderEXT,
    pub stage: vk::ShaderStageFlags,
    pub layout: Option<PipelineLayoutHandle>,
}

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
struct ImageViewKey {
    image: ImageHandle,
    subresource: SubresourceRange,
}

struct VulkanImage {
    image: vk::Image,
    view: vk::ImageView,
    allocation: Option<Allocation>,
    desc: ImageDesc,
    imported: bool,
}

struct VulkanBuffer {
    buffer: vk::Buffer,
    allocation: Option<Allocation>,
    imported: bool,
}

impl ResourceRegistry {
    pub fn new(memory_properties: vk::PhysicalDeviceMemoryProperties) -> Self {
        Self {
            allocator: GpuAllocator::new(memory_properties),
            images: HashMap::new(),
            image_views: Mutex::new(HashMap::new()),
            buffers: HashMap::new(),
            samplers: HashMap::new(),
            acceleration_structures: HashMap::new(),
            shader_objects: HashMap::new(),
            image_view_min_lod_enabled: false,
            image_compression_control_enabled: false,
            optical_flow_enabled: false,
            pageable_memory_ext: None,
            dedicated_allocation_enabled: false,
        }
    }

    pub fn allocator_stats(&self) -> AllocatorStats {
        self.allocator.stats()
    }

    pub fn create_image(
        &mut self,
        device: &Device,
        handle: ImageHandle,
        desc: ImageDesc,
    ) -> Result<()> {
        tracing::trace!(
            name = desc.debug_name.unwrap_or("(unnamed)"),
            format = ?desc.format,
            width = desc.extent.width,
            height = desc.extent.height,
            depth = desc.extent.depth,
            mips = desc.mip_levels,
            layers = desc.layers,
            samples = desc.samples,
            usage = ?desc.usage,
            "allocating image"
        );
        let mut info = vk::ImageCreateInfo::default()
            .image_type(image_type(desc))
            .format(vk_format(desc.format)?)
            .extent(vk::Extent3D {
                width: desc.extent.width,
                height: desc.extent.height,
                depth: desc.extent.depth,
            })
            .mip_levels(desc.mip_levels as u32)
            .array_layers(desc.layers as u32)
            .samples(vk_samples(desc.samples)?)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk_image_usage(desc.usage))
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        let mut fixed_rate_flags = vk_image_compression_fixed_rate_flags(desc.compression)?;
        let mut compression_control = vk_image_compression_control(
            desc.compression,
            self.image_compression_control_enabled,
            &mut fixed_rate_flags,
        );
        if let Some(control) = compression_control.as_mut() {
            info = info.push(control);
        }
        let optical_flow_usage = vk_optical_flow_image_usage(desc.usage)?;
        let mut optical_flow_info =
            vk_optical_flow_image_info(optical_flow_usage, self.optical_flow_enabled, desc.usage)?;
        if let Some(info_ext) = optical_flow_info.as_mut() {
            info = info.push(info_ext);
        }
        // GFX-5a: DRM format modifier for display-pipeline-compatible tiling.
        let drm_plane_layout = vk::SubresourceLayout::default();
        let mut drm_modifier_info;
        if let Some(modifier) = desc.drm_format_modifier {
            drm_modifier_info = vk::ImageDrmFormatModifierExplicitCreateInfoEXT::default()
                .drm_format_modifier(modifier)
                .plane_layouts(std::slice::from_ref(&drm_plane_layout));
            info = info.push(&mut drm_modifier_info);
        }

        let image = unsafe {
            device.create_image(&info, None).map_err(|error| {
                tracing::error!(
                    name = desc.debug_name.unwrap_or("(unnamed)"),
                    format = ?desc.format,
                    width = desc.extent.width,
                    height = desc.extent.height,
                    usage = ?desc.usage,
                    "vkCreateImage failed — check for unsupported format/usage combination, \
                     exceeding maxImageDimension2D, or invalid mip/layer count: {error:?}"
                );
                Error::Backend(format!("vkCreateImage failed: {error:?}"))
            })?
        };

        // GFX-1e: query dedicated allocation requirements (Vulkan 1.1 core).
        let (requirements, prefers_dedicated) = if self.dedicated_allocation_enabled {
            let mut dedicated_req = vk::MemoryDedicatedRequirementsKHR::default();
            let (mem_reqs, pref, req);
            {
                let mut req2 = vk::MemoryRequirements2::default().push(&mut dedicated_req);
                let info2 = vk::ImageMemoryRequirementsInfo2::default().image(image);
                unsafe { device.get_image_memory_requirements2(&info2, &mut req2) };
                mem_reqs = req2.memory_requirements;
            }
            pref = dedicated_req.prefers_dedicated_allocation == vk::TRUE;
            req = dedicated_req.requires_dedicated_allocation == vk::TRUE;
            let dedicated = pref || req || mem_reqs.size > 64 * 1024 * 1024;
            (mem_reqs, dedicated)
        } else {
            (
                unsafe { device.get_image_memory_requirements(image) },
                false,
            )
        };

        let allocation = if prefers_dedicated {
            // Allocate a dedicated VkDeviceMemory for this image (skip the sub-allocator pool).
            let memory_type = self
                .allocator
                .find_memory_type(
                    requirements.memory_type_bits,
                    vk::MemoryPropertyFlags::DEVICE_LOCAL,
                )
                .unwrap_or(0);
            let mut dedicated_info = vk::MemoryDedicatedAllocateInfoKHR::default().image(image);
            let mut alloc_info = vk::MemoryAllocateInfo::default()
                .allocation_size(requirements.size)
                .memory_type_index(memory_type)
                .push(&mut dedicated_info);
            // GFX-1e: chain priority for pageable memory if available.
            let mut priority_info;
            if self.allocator.memory_priority_enabled {
                priority_info = vk::MemoryPriorityAllocateInfoEXT::default().priority(0.9);
                alloc_info = alloc_info.push(&mut priority_info);
            }
            let memory = unsafe {
                device.allocate_memory(&alloc_info, None).map_err(|e| {
                    tracing::error!(
                        name = desc.debug_name.unwrap_or("(unnamed)"),
                        size = requirements.size,
                        "dedicated image vkAllocateMemory failed — \
                         device may be out of memory or memory type exhausted: {e:?}"
                    );
                    device.destroy_image(image, None);
                    Error::Backend(format!("dedicated vkAllocateMemory failed: {e:?}"))
                })?
            };
            unsafe {
                if let Err(e) = device.bind_image_memory(image, memory, 0) {
                    device.free_memory(memory, None);
                    device.destroy_image(image, None);
                    return Err(Error::Backend(format!(
                        "dedicated vkBindImageMemory failed: {e:?}"
                    )));
                }
            }
            // GFX-1e: set pageable memory priority after binding.
            if let Some(ref pm) = self.pageable_memory_ext {
                unsafe { (pm.fp().set_device_memory_priority_ext)(device.handle(), memory, 0.9) };
            }
            super::allocator::Allocation::dedicated(memory, requirements.size, memory_type)
        } else {
            match self
                .allocator
                .alloc(device, requirements, vk::MemoryPropertyFlags::DEVICE_LOCAL)
            {
                Ok(a) => a,
                Err(error) => {
                    unsafe { device.destroy_image(image, None) };
                    return Err(error);
                }
            }
        };
        // Non-dedicated path: bind memory from the sub-allocator pool.
        if !prefers_dedicated {
            unsafe {
                if let Err(error) =
                    device.bind_image_memory(image, allocation.memory, allocation.offset)
                {
                    let cleanup = self.allocator.dealloc(device, allocation);
                    device.destroy_image(image, None);
                    if let Err(cleanup_error) = cleanup {
                        return Err(Error::Backend(format!(
                            "vkBindImageMemory failed: {error:?}; additionally failed to release allocation: {cleanup_error}"
                        )));
                    }
                    return Err(Error::Backend(format!(
                        "vkBindImageMemory failed: {error:?}"
                    )));
                }
            }
        }
        let mut view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk_image_view_type(&desc))
            .format(vk_format(desc.format)?)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: vk_aspect_mask(desc.format),
                base_mip_level: 0,
                level_count: desc.mip_levels as u32,
                base_array_layer: 0,
                layer_count: desc.layers as u32,
            });
        // GFX-2k: min_lod clamping for mipmap streaming.
        let mut min_lod_info = vk::ImageViewMinLodCreateInfoEXT::default();
        if self.image_view_min_lod_enabled {
            if let Some(lod) = desc.min_lod() {
                min_lod_info = min_lod_info.min_lod(lod);
                view_info = view_info.push(&mut min_lod_info);
            }
        }
        let view = unsafe {
            match device.create_image_view(&view_info, None) {
                Ok(view) => view,
                Err(error) => {
                    tracing::error!(
                        name = desc.debug_name.unwrap_or("(unnamed)"),
                        format = ?desc.format,
                        mips = desc.mip_levels,
                        layers = desc.layers,
                        "vkCreateImageView failed — format may not support the requested \
                         view type, aspect mask may be wrong, or mip/layer range is invalid: {error:?}"
                    );
                    let cleanup = self.allocator.dealloc(device, allocation);
                    device.destroy_image(image, None);
                    if let Err(cleanup_error) = cleanup {
                        return Err(Error::Backend(format!(
                            "vkCreateImageView failed: {error:?}; additionally failed to release allocation: {cleanup_error}"
                        )));
                    }
                    return Err(Error::Backend(format!(
                        "vkCreateImageView failed: {error:?}"
                    )));
                }
            }
        };

        self.images.insert(
            handle,
            VulkanImage {
                image,
                view,
                allocation: Some(allocation),
                desc,
                imported: false,
            },
        );
        Ok(())
    }

    /// Create a `VkImage` and view but do NOT allocate or bind memory.
    ///
    /// The image must later be bound via `bind_image_to_memory_if_unbound` before
    /// the GPU accesses it.  Sets `VK_IMAGE_CREATE_ALIAS_BIT` so that multiple
    /// unbound images can share the same `VkDeviceMemory` simultaneously.
    pub fn create_image_unbound(
        &mut self,
        device: &Device,
        handle: ImageHandle,
        desc: ImageDesc,
    ) -> Result<()> {
        let mut info = vk::ImageCreateInfo::default()
            .flags(vk::ImageCreateFlags::ALIAS)
            .image_type(image_type(desc))
            .format(vk_format(desc.format)?)
            .extent(vk::Extent3D {
                width: desc.extent.width,
                height: desc.extent.height,
                depth: desc.extent.depth,
            })
            .mip_levels(desc.mip_levels as u32)
            .array_layers(desc.layers as u32)
            .samples(vk_samples(desc.samples)?)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk_image_usage(desc.usage))
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        let mut fixed_rate_flags = vk_image_compression_fixed_rate_flags(desc.compression)?;
        let mut compression_control = vk_image_compression_control(
            desc.compression,
            self.image_compression_control_enabled,
            &mut fixed_rate_flags,
        );
        if let Some(control) = compression_control.as_mut() {
            info = info.push(control);
        }
        let optical_flow_usage = vk_optical_flow_image_usage(desc.usage)?;
        let mut optical_flow_info =
            vk_optical_flow_image_info(optical_flow_usage, self.optical_flow_enabled, desc.usage)?;
        if let Some(info_ext) = optical_flow_info.as_mut() {
            info = info.push(info_ext);
        }
        let drm_plane_layout2 = vk::SubresourceLayout::default();
        let mut drm_modifier_info2;
        if let Some(modifier) = desc.drm_format_modifier {
            drm_modifier_info2 = vk::ImageDrmFormatModifierExplicitCreateInfoEXT::default()
                .drm_format_modifier(modifier)
                .plane_layouts(std::slice::from_ref(&drm_plane_layout2));
            info = info.push(&mut drm_modifier_info2);
        }

        let image = unsafe {
            device
                .create_image(&info, None)
                .map_err(|e| Error::Backend(format!("vkCreateImage (unbound) failed: {e:?}")))?
        };
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk_image_view_type(&desc))
            .format(vk_format(desc.format)?)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: vk_aspect_mask(desc.format),
                base_mip_level: 0,
                level_count: desc.mip_levels as u32,
                base_array_layer: 0,
                layer_count: desc.layers as u32,
            });
        let view = unsafe {
            match device.create_image_view(&view_info, None) {
                Ok(v) => v,
                Err(e) => {
                    device.destroy_image(image, None);
                    return Err(Error::Backend(format!(
                        "vkCreateImageView (unbound) failed: {e:?}"
                    )));
                }
            }
        };
        self.images.insert(
            handle,
            VulkanImage {
                image,
                view,
                allocation: None,
                desc,
                imported: false,
            },
        );
        Ok(())
    }

    /// Query the memory requirements for an existing (possibly unbound) image.
    pub fn image_memory_requirements(
        &self,
        device: &Device,
        handle: ImageHandle,
    ) -> Result<vk::MemoryRequirements> {
        let vk_image = self.images.get(&handle).ok_or(Error::InvalidHandle)?;
        Ok(unsafe { device.get_image_memory_requirements(vk_image.image) })
    }

    /// Bind `handle` to `memory` at `offset`, but only if the image is currently unbound.
    ///
    /// Skips images that already have their own allocation (non-transient or
    /// already bound in a previous call).
    pub fn bind_image_to_memory_if_unbound(
        &self,
        device: &Device,
        handle: ImageHandle,
        memory: vk::DeviceMemory,
        offset: u64,
    ) -> Result<()> {
        let vk_image = self.images.get(&handle).ok_or(Error::InvalidHandle)?;
        if vk_image.allocation.is_some() {
            return Ok(()); // already has its own allocation
        }
        unsafe {
            device
                .bind_image_memory(vk_image.image, memory, offset)
                .map_err(|e| Error::Backend(format!("vkBindImageMemory failed: {e:?}")))?;
        }
        Ok(())
    }

    /// Expose the allocator so the flush path can query memory types.
    pub fn allocator(&self) -> &super::allocator::GpuAllocator {
        &self.allocator
    }

    /// Expose the allocator mutably for configuration at startup.
    pub fn allocator_mut(&mut self) -> &mut super::allocator::GpuAllocator {
        &mut self.allocator
    }

    pub fn destroy_image(&mut self, device: &Device, handle: ImageHandle) -> Result<()> {
        let image = self.images.remove(&handle).ok_or(Error::InvalidHandle)?;
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        destroy_cached_image_views(
            device,
            &mut self
                .image_views
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            handle,
        );
        if !image.imported {
            unsafe {
                device.destroy_image_view(image.view, None);
                device.destroy_image(image.image, None);
            }
            if let Some(allocation) = image.allocation {
                self.allocator.dealloc(device, allocation)?;
            }
        }
        Ok(())
    }

    pub fn import_image(
        &mut self,
        handle: ImageHandle,
        image: vk::Image,
        view: vk::ImageView,
        desc: ImageDesc,
    ) -> Result<()> {
        if self.images.contains_key(&handle) {
            return Err(Error::InvalidHandle);
        }
        self.images.insert(
            handle,
            VulkanImage {
                image,
                view,
                allocation: None,
                desc,
                imported: true,
            },
        );
        Ok(())
    }

    pub fn import_external_image(
        &mut self,
        handle: ImageHandle,
        external: VulkanExternalImage,
        desc: ImageDesc,
    ) -> Result<()> {
        self.import_image(
            handle,
            vk::Image::from_raw(external.image),
            vk::ImageView::from_raw(external.image_view),
            desc,
        )
    }

    pub fn create_buffer(
        &mut self,
        device: &Device,
        handle: BufferHandle,
        desc: BufferDesc,
    ) -> Result<()> {
        tracing::trace!(
            size = desc.size,
            usage = ?desc.usage,
            "allocating buffer"
        );
        let info = vk::BufferCreateInfo::default()
            .size(desc.size)
            .usage(vk_buffer_usage(desc.usage))
            .sharing_mode(vk::SharingMode::EXCLUSIVE);

        let buffer = unsafe {
            device.create_buffer(&info, None).map_err(|error| {
                tracing::error!(
                    size = desc.size,
                    usage = ?desc.usage,
                    "vkCreateBuffer failed — check usage flag support or device memory limits: {error:?}"
                );
                Error::Backend(format!("vkCreateBuffer failed: {error:?}"))
            })?
        };
        let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
        let allocate_flags = if desc.usage.contains(BufferUsage::SHADER_DEVICE_ADDRESS) {
            vk::MemoryAllocateFlags::DEVICE_ADDRESS
        } else {
            vk::MemoryAllocateFlags::empty()
        };
        let allocation = match self.allocator.alloc_with_flags(
            device,
            requirements,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            allocate_flags,
        ) {
            Ok(a) => a,
            Err(error) => {
                unsafe { device.destroy_buffer(buffer, None) };
                return Err(error);
            }
        };
        unsafe {
            if let Err(error) =
                device.bind_buffer_memory(buffer, allocation.memory, allocation.offset)
            {
                tracing::error!(
                    size = desc.size,
                    usage = ?desc.usage,
                    "vkBindBufferMemory failed — memory type may not support the requested \
                     usage flags or the allocation offset is misaligned: {error:?}"
                );
                let cleanup = self.allocator.dealloc(device, allocation);
                device.destroy_buffer(buffer, None);
                if let Err(cleanup_error) = cleanup {
                    return Err(Error::Backend(format!(
                        "vkBindBufferMemory failed: {error:?}; additionally failed to release allocation: {cleanup_error}"
                    )));
                }
                return Err(Error::Backend(format!(
                    "vkBindBufferMemory failed: {error:?}"
                )));
            }
        }

        self.buffers.insert(
            handle,
            VulkanBuffer {
                buffer,
                allocation: Some(allocation),
                imported: false,
            },
        );
        Ok(())
    }

    pub fn destroy_buffer(&mut self, device: &Device, handle: BufferHandle) -> Result<()> {
        let buf = self.buffers.remove(&handle).ok_or(Error::InvalidHandle)?;
        if !buf.imported {
            unsafe { device.destroy_buffer(buf.buffer, None) };
            if let Some(allocation) = buf.allocation {
                self.allocator.dealloc(device, allocation)?;
            }
        }
        Ok(())
    }

    pub fn import_external_buffer(
        &mut self,
        handle: BufferHandle,
        external: VulkanExternalBuffer,
    ) -> Result<()> {
        if self.buffers.contains_key(&handle) {
            return Err(Error::InvalidHandle);
        }
        self.buffers.insert(
            handle,
            VulkanBuffer {
                buffer: vk::Buffer::from_raw(external.buffer),
                allocation: None,
                imported: true,
            },
        );
        Ok(())
    }

    pub fn create_sampler(
        &mut self,
        device: &Device,
        handle: SamplerHandle,
        desc: SamplerDesc,
    ) -> Result<()> {
        if self.samplers.contains_key(&handle) {
            return Err(Error::InvalidHandle);
        }
        let mut reduction_info = vk::SamplerReductionModeCreateInfo::default()
            .reduction_mode(vk_sampler_reduction_mode(desc.reduction_mode));
        let mut custom_border_info = vk_custom_border_color_info(desc.border_color);
        let mut info = vk::SamplerCreateInfo::default()
            .mag_filter(vk_filter(desc.mag_filter))
            .min_filter(vk_filter(desc.min_filter))
            .mipmap_mode(vk_mipmap_mode(desc.mipmap_mode))
            .address_mode_u(vk_address_mode(desc.address_u))
            .address_mode_v(vk_address_mode(desc.address_v))
            .address_mode_w(vk_address_mode(desc.address_w))
            .mip_lod_bias(desc.mip_lod_bias)
            .anisotropy_enable(desc.max_anisotropy.is_some())
            .max_anisotropy(desc.max_anisotropy.unwrap_or(1.0))
            .compare_enable(desc.compare.is_some())
            .compare_op(
                desc.compare
                    .map(vk_compare_op)
                    .unwrap_or(vk::CompareOp::ALWAYS),
            )
            .min_lod(desc.min_lod)
            .max_lod(desc.max_lod)
            .border_color(vk_border_color(desc.border_color))
            .unnormalized_coordinates(desc.unnormalized_coordinates);

        let mut next: *const c_void = std::ptr::null();
        if desc.reduction_mode != SamplerReductionMode::WeightedAverage {
            reduction_info.p_next = next;
            next = (&reduction_info as *const vk::SamplerReductionModeCreateInfo<'_>).cast();
        }
        if let Some(custom_border_info) = custom_border_info.as_mut() {
            custom_border_info.p_next = next;
            next =
                (custom_border_info as *const vk::SamplerCustomBorderColorCreateInfoEXT<'_>).cast();
        }
        if !next.is_null() {
            info.p_next = next;
        }
        let sampler = unsafe {
            device
                .create_sampler(&info, None)
                .map_err(|error| Error::Backend(format!("vkCreateSampler failed: {error:?}")))?
        };
        self.samplers.insert(handle, (sampler, desc));
        Ok(())
    }

    pub fn destroy_sampler(&mut self, device: &Device, handle: SamplerHandle) -> Result<()> {
        let (sampler, _) = self.samplers.remove(&handle).ok_or(Error::InvalidHandle)?;
        unsafe {
            device.destroy_sampler(sampler, None);
        }
        Ok(())
    }

    pub fn write_buffer(&self, handle: BufferHandle, offset: u64, data: &[u8]) -> Result<()> {
        let buf = self.buffers.get(&handle).ok_or(Error::InvalidHandle)?;
        if data.is_empty() {
            return Ok(());
        }
        let allocation = buf
            .allocation
            .as_ref()
            .ok_or(Error::Unsupported("cannot CPU-write an imported buffer"))?;
        let base = allocation
            .mapped_ptr
            .ok_or_else(|| Error::Backend("buffer allocation is not host-visible".into()))?;
        let start = offset as usize;
        let end = start + data.len();
        if end > allocation.size as usize {
            return Err(Error::InvalidInput(format!(
                "write_buffer out of range: offset={offset} len={} capacity={}",
                data.len(),
                allocation.size
            )));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), base.add(start), data.len());
        }
        Ok(())
    }

    pub fn read_buffer(&self, handle: BufferHandle, offset: u64, out: &mut [u8]) -> Result<()> {
        let buf = self.buffers.get(&handle).ok_or(Error::InvalidHandle)?;
        if out.is_empty() {
            return Ok(());
        }
        let allocation = buf
            .allocation
            .as_ref()
            .ok_or(Error::Unsupported("cannot CPU-read an imported buffer"))?;
        let base = allocation
            .mapped_ptr
            .ok_or_else(|| Error::Backend("buffer allocation is not host-visible".into()))?;
        let start = offset as usize;
        let end = start + out.len();
        if end > allocation.size as usize {
            return Err(Error::InvalidInput(format!(
                "read_buffer out of range: offset={offset} len={} capacity={}",
                out.len(),
                allocation.size
            )));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(base.add(start), out.as_mut_ptr(), out.len());
        }
        Ok(())
    }

    pub fn destroy_all(
        &mut self,
        device: &Device,
        as_ext: Option<&ash::khr::acceleration_structure::Device>,
        shader_object_ext: Option<&ash::ext::shader_object::Device>,
    ) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        for (_, view) in self
            .image_views
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .drain()
        {
            unsafe {
                device.destroy_image_view(view, None);
            }
        }
        for (_, image) in self.images.drain() {
            if !image.imported {
                unsafe {
                    device.destroy_image_view(image.view, None);
                    device.destroy_image(image.image, None);
                }
                if let Some(allocation) = image.allocation {
                    let _ = self.allocator.dealloc(device, allocation);
                }
            }
        }
        for (_, buf) in self.buffers.drain() {
            if !buf.imported {
                unsafe { device.destroy_buffer(buf.buffer, None) };
                if let Some(allocation) = buf.allocation {
                    let _ = self.allocator.dealloc(device, allocation);
                }
            }
        }
        for (_, (sampler, _)) in self.samplers.drain() {
            unsafe {
                device.destroy_sampler(sampler, None);
            }
        }
        for (_, entry) in self.acceleration_structures.drain() {
            if let Some(as_ext) = as_ext {
                unsafe {
                    as_ext.destroy_acceleration_structure(entry.acceleration_structure, None);
                }
            }
            unsafe {
                device.destroy_buffer(entry.buffer, None);
            }
            let _ = self.allocator.dealloc(device, entry.allocation);
        }
        for (_, shader_object) in self.shader_objects.drain() {
            if let Some(shader_object_ext) = shader_object_ext {
                unsafe {
                    shader_object_ext.destroy_shader(shader_object.shader, None);
                }
            }
        }
        self.allocator.destroy_all(device);
    }

    /// Register a VK_EXT_shader_object handle for an engine ShaderObjectHandle.
    pub fn register_shader_object(&mut self, handle: ShaderObjectHandle, obj: VulkanShaderObject) {
        self.shader_objects.insert(handle, obj);
    }

    /// Look up a VkShaderEXT by engine handle.
    pub fn shader_object(&self, handle: ShaderObjectHandle) -> Result<VulkanShaderObject> {
        self.shader_objects
            .get(&handle)
            .copied()
            .ok_or(Error::InvalidHandle)
    }

    /// Destroy a VkShaderEXT and remove it from the registry.
    pub fn destroy_shader_object(
        &mut self,
        ext: &ash::ext::shader_object::Device,
        handle: ShaderObjectHandle,
    ) -> Result<()> {
        let obj = self
            .shader_objects
            .remove(&handle)
            .ok_or(Error::InvalidHandle)?;
        unsafe {
            ext.destroy_shader(obj.shader, None);
        }
        Ok(())
    }

    pub fn image(&self, handle: ImageHandle) -> Result<vk::Image> {
        self.images
            .get(&handle)
            .map(|image| image.image)
            .ok_or(Error::InvalidHandle)
    }

    pub fn image_view(&self, handle: ImageHandle) -> Result<vk::ImageView> {
        self.images
            .get(&handle)
            .map(|image| image.view)
            .ok_or(Error::InvalidHandle)
    }

    pub fn image_view_for_subresource(
        &self,
        device: &Device,
        handle: ImageHandle,
        subresource: SubresourceRange,
    ) -> Result<vk::ImageView> {
        let vk_image = self.images.get(&handle).ok_or(Error::InvalidHandle)?;
        let desc = vk_image.desc;
        let normalized = normalize_subresource(desc, subresource)?;
        if normalized.base_mip == 0
            && normalized.mip_count == desc.mip_levels
            && normalized.base_layer == 0
            && normalized.layer_count == desc.layers
        {
            return Ok(vk_image.view);
        }

        let key = ImageViewKey {
            image: handle,
            subresource: normalized,
        };
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        if let Some(view) = self
            .image_views
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&key)
            .copied()
        {
            return Ok(view);
        }

        let view_info = vk::ImageViewCreateInfo::default()
            .image(vk_image.image)
            .view_type(vk_image_view_type_for_subresource(desc, normalized))
            .format(vk_format(desc.format)?)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: vk_aspect_mask(desc.format),
                base_mip_level: normalized.base_mip as u32,
                level_count: normalized.mip_count as u32,
                base_array_layer: normalized.base_layer as u32,
                layer_count: normalized.layer_count as u32,
            });
        let view = unsafe {
            device
                .create_image_view(&view_info, None)
                .map_err(|error| Error::Backend(format!("vkCreateImageView failed: {error:?}")))?
        };
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let _prev = self
            .image_views
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key, view);
        Ok(view)
    }

    pub fn image_desc(&self, handle: ImageHandle) -> Result<ImageDesc> {
        self.images
            .get(&handle)
            .map(|image| image.desc)
            .ok_or(Error::InvalidHandle)
    }

    pub fn buffer(&self, handle: BufferHandle) -> Result<vk::Buffer> {
        self.buffers
            .get(&handle)
            .map(|buffer| buffer.buffer)
            .ok_or(Error::InvalidHandle)
    }

    /// Register an existing `vk::Buffer` as a `BufferHandle` without allocating memory.
    ///
    /// Used to expose the transient buffer pool to the descriptor system. The buffer's
    /// lifetime must exceed the handle's use; destroy is NOT called on this entry
    /// because `imported = true` skips cleanup in `destroy_buffer`.
    pub fn register_raw_buffer(&mut self, handle: BufferHandle, buffer: vk::Buffer) {
        self.buffers.insert(handle, VulkanBuffer {
            buffer,
            allocation: None,
            imported: true,
        });
    }

    pub fn sampler(&self, handle: SamplerHandle) -> Result<vk::Sampler> {
        self.samplers
            .get(&handle)
            .map(|(s, _)| *s)
            .ok_or(Error::InvalidHandle)
    }

    /// GFX-7b: Return the engine-level `SamplerDesc` stored alongside the sampler handle.
    pub fn sampler_desc(&self, handle: SamplerHandle) -> Option<&crate::SamplerDesc> {
        self.samplers.get(&handle).map(|(_, d)| d)
    }

    /// GFX-7b: Raw image + ImageDesc for a handle — used to reconstruct `ImageViewCreateInfo`
    /// when writing sampled/storage image descriptors to a `VkDescriptorHeapEXT` resource heap.
    pub fn image_and_desc(&self, handle: ImageHandle) -> Option<(vk::Image, &crate::ImageDesc)> {
        let img = self.images.get(&handle)?;
        Some((img.image, &img.desc))
    }

    /// Return the `VkDeviceAddress` of a buffer (base address, offset = 0).
    pub fn buffer_device_address_raw(&self, device: &Device, handle: BufferHandle) -> Result<u64> {
        let buf = self.buffers.get(&handle).ok_or(Error::InvalidHandle)?;
        let info = vk::BufferDeviceAddressInfo::default().buffer(buf.buffer);
        let base = unsafe { device.get_buffer_device_address(&info) };
        if base == 0 {
            return Err(Error::Backend(
                "vkGetBufferDeviceAddress returned 0; buffer may not have SHADER_DEVICE_ADDRESS usage".into(),
            ));
        }
        Ok(base)
    }

    /// Return the `VkDeviceAddress` of a buffer + byte offset.
    pub fn buffer_device_address_at(
        &self,
        device: &Device,
        handle: BufferHandle,
        offset: u64,
    ) -> Result<u64> {
        Ok(self.buffer_device_address_raw(device, handle)? + offset)
    }

    pub fn create_scratch_buffer(
        &mut self,
        device: &Device,
        size: u64,
    ) -> Result<VulkanScratchBuffer> {
        if size == 0 {
            return Err(Error::InvalidInput(
                "scratch buffer size must be non-zero".into(),
            ));
        }
        let alignment = 256;
        let info = vk::BufferCreateInfo::default()
            .size(size.saturating_add(alignment - 1))
            .usage(
                vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            )
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let buffer = unsafe {
            device
                .create_buffer(&info, None)
                .map_err(|e| Error::Backend(format!("vkCreateBuffer (AS scratch) failed: {e:?}")))?
        };
        let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
        let allocation = match self.allocator.alloc_with_flags(
            device,
            requirements,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
            vk::MemoryAllocateFlags::DEVICE_ADDRESS,
        ) {
            Ok(allocation) => allocation,
            Err(error) => {
                unsafe { device.destroy_buffer(buffer, None) };
                return Err(error);
            }
        };
        if let Err(error) =
            unsafe { device.bind_buffer_memory(buffer, allocation.memory, allocation.offset) }
        {
            let _ = self.allocator.dealloc(device, allocation);
            unsafe { device.destroy_buffer(buffer, None) };
            return Err(Error::Backend(format!(
                "vkBindBufferMemory (AS scratch) failed: {error:?}"
            )));
        }
        Ok(VulkanScratchBuffer { buffer, allocation })
    }

    pub fn destroy_scratch_buffer(
        &mut self,
        device: &Device,
        scratch: VulkanScratchBuffer,
    ) -> Result<()> {
        unsafe {
            device.destroy_buffer(scratch.buffer, None);
        }
        self.allocator.dealloc(device, scratch.allocation)
    }

    pub fn scratch_buffer_device_address(
        device: &Device,
        scratch: &VulkanScratchBuffer,
    ) -> Result<u64> {
        let info = vk::BufferDeviceAddressInfo::default().buffer(scratch.buffer);
        let base = unsafe { device.get_buffer_device_address(&info) };
        if base == 0 {
            return Err(Error::Backend(
                "vkGetBufferDeviceAddress returned 0 for AS scratch buffer".into(),
            ));
        }
        Ok((base + 255) & !255)
    }

    /// Look up a `VkAccelerationStructureKHR` by handle.
    pub fn acceleration_structure(
        &self,
        handle: AccelerationStructureHandle,
    ) -> Result<vk::AccelerationStructureKHR> {
        self.acceleration_structures
            .get(&handle)
            .map(|as_| as_.acceleration_structure)
            .ok_or(Error::InvalidHandle)
    }

    /// Return the logical kind and allocation size recorded at creation time.
    pub fn acceleration_structure_metadata(
        &self,
        handle: AccelerationStructureHandle,
    ) -> Result<(AccelerationStructureKind, u64)> {
        self.acceleration_structures
            .get(&handle)
            .map(|as_| (as_.kind, as_.size))
            .ok_or(Error::InvalidHandle)
    }

    /// Create and register a new acceleration structure buffer + VkAccelerationStructureKHR.
    pub fn create_acceleration_structure(
        &mut self,
        device: &Device,
        handle: AccelerationStructureHandle,
        size: u64,
        as_ext: &ash::khr::acceleration_structure::Device,
        ty: vk::AccelerationStructureTypeKHR,
    ) -> Result<()> {
        let buf_info = vk::BufferCreateInfo::default()
            .size(size)
            .usage(
                vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR
                    | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            )
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let buffer = unsafe {
            device
                .create_buffer(&buf_info, None)
                .map_err(|e| Error::Backend(format!("vkCreateBuffer (AS) failed: {e:?}")))?
        };
        let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
        let allocation = match self.allocator.alloc_with_flags(
            device,
            requirements,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
            vk::MemoryAllocateFlags::DEVICE_ADDRESS,
        ) {
            Ok(a) => a,
            Err(e) => {
                unsafe { device.destroy_buffer(buffer, None) };
                return Err(e);
            }
        };
        if let Err(e) =
            unsafe { device.bind_buffer_memory(buffer, allocation.memory, allocation.offset) }
        {
            let _ = self.allocator.dealloc(device, allocation);
            unsafe { device.destroy_buffer(buffer, None) };
            return Err(Error::Backend(format!(
                "vkBindBufferMemory (AS) failed: {e:?}"
            )));
        }
        let as_info = vk::AccelerationStructureCreateInfoKHR::default()
            .buffer(buffer)
            .offset(0)
            .size(size)
            .ty(ty);
        let acceleration_structure =
            match unsafe { as_ext.create_acceleration_structure(&as_info, None) } {
                Ok(r) => r,
                Err(e) => {
                    let _ = self.allocator.dealloc(device, allocation);
                    unsafe { device.destroy_buffer(buffer, None) };
                    return Err(Error::Backend(format!(
                        "vkCreateAccelerationStructureKHR failed: {e:?}"
                    )));
                }
            };
        self.acceleration_structures.insert(
            handle,
            VulkanAccelerationStructure {
                acceleration_structure,
                buffer,
                allocation,
                size,
                kind: acceleration_structure_kind(ty),
            },
        );
        Ok(())
    }

    /// Destroy an acceleration structure by handle.
    pub fn destroy_acceleration_structure(
        &mut self,
        device: &Device,
        handle: AccelerationStructureHandle,
        as_ext: &ash::khr::acceleration_structure::Device,
    ) -> Result<()> {
        let entry = self
            .acceleration_structures
            .remove(&handle)
            .ok_or(Error::InvalidHandle)?;
        unsafe {
            as_ext.destroy_acceleration_structure(entry.acceleration_structure, None);
            device.destroy_buffer(entry.buffer, None);
        }
        self.allocator.dealloc(device, entry.allocation)?;
        Ok(())
    }

    /// Return the device address of an acceleration structure's underlying buffer.
    pub fn acceleration_structure_device_address(
        &self,
        _device: &Device,
        handle: AccelerationStructureHandle,
        as_ext: &ash::khr::acceleration_structure::Device,
    ) -> Result<u64> {
        let entry = self
            .acceleration_structures
            .get(&handle)
            .ok_or(Error::InvalidHandle)?;
        let info = vk::AccelerationStructureDeviceAddressInfoKHR::default()
            .acceleration_structure(entry.acceleration_structure);
        let addr = unsafe { as_ext.get_acceleration_structure_device_address(&info) };
        Ok(addr)
    }
}

fn acceleration_structure_kind(ty: vk::AccelerationStructureTypeKHR) -> AccelerationStructureKind {
    match ty {
        vk::AccelerationStructureTypeKHR::TOP_LEVEL => AccelerationStructureKind::TopLevel,
        _ => AccelerationStructureKind::BottomLevel,
    }
}

fn destroy_cached_image_views(
    device: &Device,
    cache: &mut HashMap<ImageViewKey, vk::ImageView>,
    handle: ImageHandle,
) {
    let stale = cache
        .keys()
        .copied()
        .filter(|key| key.image == handle)
        .collect::<Vec<_>>();
    for key in stale {
        if let Some(view) = cache.remove(&key) {
            unsafe {
                device.destroy_image_view(view, None);
            }
        }
    }
}

fn normalize_subresource(
    desc: ImageDesc,
    subresource: SubresourceRange,
) -> Result<SubresourceRange> {
    let mip_count = normalize_subresource_count(
        subresource.base_mip,
        subresource.mip_count,
        desc.mip_levels,
        "mip",
    )?;
    let layer_count = normalize_subresource_count(
        subresource.base_layer,
        subresource.layer_count,
        desc.layers,
        "layer",
    )?;
    Ok(SubresourceRange {
        base_mip: subresource.base_mip,
        mip_count,
        base_layer: subresource.base_layer,
        layer_count,
    })
}

fn normalize_subresource_count(base: u16, count: u16, limit: u16, name: &str) -> Result<u16> {
    if count == 0 {
        return Err(Error::InvalidInput(format!(
            "{name} subresource count must be at least 1"
        )));
    }
    if base >= limit {
        return Err(Error::InvalidInput(format!(
            "{name} subresource base {base} is outside image limit {limit}"
        )));
    }
    if count == u16::MAX {
        return Ok(limit - base);
    }
    let end = u32::from(base).saturating_add(u32::from(count));
    if end > u32::from(limit) {
        return Err(Error::InvalidInput(format!(
            "{name} subresource range [{base}, {end}) exceeds image limit {limit}"
        )));
    }
    Ok(count)
}

fn image_type(desc: ImageDesc) -> vk::ImageType {
    match desc.dimension {
        crate::ImageDimension::D1 => vk::ImageType::TYPE_1D,
        crate::ImageDimension::D2 => vk::ImageType::TYPE_2D,
        crate::ImageDimension::D3 => vk::ImageType::TYPE_3D,
    }
}

fn vk_image_compression_control<'a>(
    compression: ImageCompression,
    enabled: bool,
    fixed_rate_flags: &'a mut [vk::ImageCompressionFixedRateFlagsEXT; 1],
) -> Option<vk::ImageCompressionControlEXT<'a>> {
    if !enabled {
        return None;
    }
    match compression {
        ImageCompression::Default => None,
        ImageCompression::Fixed { .. } => Some(
            vk::ImageCompressionControlEXT::default()
                .flags(vk::ImageCompressionFlagsEXT::FIXED_RATE_EXPLICIT)
                .fixed_rate_flags(fixed_rate_flags),
        ),
        ImageCompression::Disabled => Some(
            vk::ImageCompressionControlEXT::default().flags(vk::ImageCompressionFlagsEXT::DISABLED),
        ),
    }
}

fn vk_image_compression_fixed_rate_flags(
    compression: ImageCompression,
) -> Result<[vk::ImageCompressionFixedRateFlagsEXT; 1]> {
    let ImageCompression::Fixed { bits_per_component } = compression else {
        return Ok([vk::ImageCompressionFixedRateFlagsEXT::NONE]);
    };
    if !(1..=24).contains(&bits_per_component) {
        return Err(Error::InvalidInput(format!(
            "fixed image compression bits_per_component must be in 1..=24, got {bits_per_component}"
        )));
    }
    Ok([vk::ImageCompressionFixedRateFlagsEXT::from_raw(
        1u32 << (bits_per_component - 1),
    )])
}

fn vk_optical_flow_image_usage(usage: ImageUsage) -> Result<vk::OpticalFlowUsageFlagsNV> {
    let mut flags = vk::OpticalFlowUsageFlagsNV::empty();
    if usage.contains(ImageUsage::OPTICAL_FLOW_INPUT) {
        flags |= vk::OpticalFlowUsageFlagsNV::INPUT;
    }
    if usage.contains(ImageUsage::OPTICAL_FLOW_OUTPUT) {
        flags |= vk::OpticalFlowUsageFlagsNV::OUTPUT;
    }
    if usage.contains(ImageUsage::OPTICAL_FLOW_HINT) {
        flags |= vk::OpticalFlowUsageFlagsNV::HINT;
    }
    Ok(flags)
}

fn vk_optical_flow_image_info(
    optical_flow_usage: vk::OpticalFlowUsageFlagsNV,
    enabled: bool,
    image_usage: ImageUsage,
) -> Result<Option<vk::OpticalFlowImageFormatInfoNV<'static>>> {
    if optical_flow_usage.is_empty() {
        return Ok(None);
    }
    if !enabled {
        return Err(Error::Unsupported(
            "optical-flow image usage requires VK_NV_optical_flow".into(),
        ));
    }
    if image_usage.contains(ImageUsage::OPTICAL_FLOW_INPUT)
        && image_usage.contains(ImageUsage::OPTICAL_FLOW_OUTPUT)
    {
        return Err(Error::InvalidInput(
            "optical-flow input and output usage must use separate images".into(),
        ));
    }
    Ok(Some(
        vk::OpticalFlowImageFormatInfoNV::default().usage(optical_flow_usage),
    ))
}

pub(super) fn vk_image_view_type(desc: &ImageDesc) -> vk::ImageViewType {
    match desc.dimension {
        crate::ImageDimension::D1 if desc.layers > 1 => vk::ImageViewType::TYPE_1D_ARRAY,
        crate::ImageDimension::D1 => vk::ImageViewType::TYPE_1D,
        crate::ImageDimension::D2 if desc.layers > 1 => vk::ImageViewType::TYPE_2D_ARRAY,
        crate::ImageDimension::D2 => vk::ImageViewType::TYPE_2D,
        crate::ImageDimension::D3 => vk::ImageViewType::TYPE_3D,
    }
}

fn vk_image_view_type_for_subresource(
    desc: ImageDesc,
    subresource: SubresourceRange,
) -> vk::ImageViewType {
    match desc.dimension {
        crate::ImageDimension::D1 if subresource.layer_count > 1 => {
            vk::ImageViewType::TYPE_1D_ARRAY
        }
        crate::ImageDimension::D1 => vk::ImageViewType::TYPE_1D,
        crate::ImageDimension::D2 if subresource.layer_count > 1 => {
            vk::ImageViewType::TYPE_2D_ARRAY
        }
        crate::ImageDimension::D2 => vk::ImageViewType::TYPE_2D,
        crate::ImageDimension::D3 => vk::ImageViewType::TYPE_3D,
    }
}

pub(super) fn vk_format(format: Format) -> Result<vk::Format> {
    match format {
        Format::Unknown => Err(Error::InvalidInput("image format must be specified".into())),
        Format::Rgba8Unorm => Ok(vk::Format::R8G8B8A8_UNORM),
        Format::Bgra8Unorm => Ok(vk::Format::B8G8R8A8_UNORM),
        Format::Rgba16Float => Ok(vk::Format::R16G16B16A16_SFLOAT),
        Format::Rgba32Float => Ok(vk::Format::R32G32B32A32_SFLOAT),
        Format::R8Unorm => Ok(vk::Format::R8_UNORM),
        Format::Rg8Unorm => Ok(vk::Format::R8G8_UNORM),
        Format::Bc3Unorm => Ok(vk::Format::BC3_UNORM_BLOCK),
        Format::Bc3UnormSrgb => Ok(vk::Format::BC3_SRGB_BLOCK),
        Format::Bc4Unorm => Ok(vk::Format::BC4_UNORM_BLOCK),
        Format::Bc5Unorm => Ok(vk::Format::BC5_UNORM_BLOCK),
        Format::Bc7Unorm => Ok(vk::Format::BC7_UNORM_BLOCK),
        Format::Bc7UnormSrgb => Ok(vk::Format::BC7_SRGB_BLOCK),
        Format::Bc6hUfloat => Ok(vk::Format::BC6H_UFLOAT_BLOCK),
        Format::G8_B8R8_2PLANE_420_UNORM => Ok(vk::Format::G8_B8R8_2PLANE_420_UNORM),
        Format::Depth32Float => Ok(vk::Format::D32_SFLOAT),
        Format::Depth24Stencil8 => Ok(vk::Format::D24_UNORM_S8_UINT),
    }
}

pub(super) fn vk_aspect_mask(format: Format) -> vk::ImageAspectFlags {
    match format {
        Format::Depth32Float => vk::ImageAspectFlags::DEPTH,
        Format::Depth24Stencil8 => vk::ImageAspectFlags::DEPTH | vk::ImageAspectFlags::STENCIL,
        _ => vk::ImageAspectFlags::COLOR,
    }
}

fn vk_samples(samples: u8) -> Result<vk::SampleCountFlags> {
    match samples {
        1 => Ok(vk::SampleCountFlags::TYPE_1),
        2 => Ok(vk::SampleCountFlags::TYPE_2),
        4 => Ok(vk::SampleCountFlags::TYPE_4),
        8 => Ok(vk::SampleCountFlags::TYPE_8),
        16 => Ok(vk::SampleCountFlags::TYPE_16),
        32 => Ok(vk::SampleCountFlags::TYPE_32),
        64 => Ok(vk::SampleCountFlags::TYPE_64),
        _ => Err(Error::InvalidInput(format!(
            "unsupported Vulkan sample count: {samples}"
        ))),
    }
}

fn vk_image_usage(usage: ImageUsage) -> vk::ImageUsageFlags {
    let mut flags = vk::ImageUsageFlags::empty();
    if usage.contains(ImageUsage::SAMPLED) {
        flags |= vk::ImageUsageFlags::SAMPLED;
    }
    if usage.contains(ImageUsage::STORAGE) {
        flags |= vk::ImageUsageFlags::STORAGE;
    }
    if usage.contains(ImageUsage::RENDER_TARGET) {
        flags |= vk::ImageUsageFlags::COLOR_ATTACHMENT;
    }
    if usage.contains(ImageUsage::DEPTH_STENCIL) {
        flags |= vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT;
    }
    if usage.contains(ImageUsage::PRESENT) {
        flags |= vk::ImageUsageFlags::COLOR_ATTACHMENT;
    }
    if usage.contains(ImageUsage::COPY_SRC) {
        flags |= vk::ImageUsageFlags::TRANSFER_SRC;
    }
    if usage.contains(ImageUsage::COPY_DST) {
        flags |= vk::ImageUsageFlags::TRANSFER_DST;
    }
    // GFX-4: video decode/encode usage flags.
    if usage.contains(ImageUsage::VIDEO_DECODE_DST) {
        flags |= vk::ImageUsageFlags::VIDEO_DECODE_DST_KHR;
    }
    if usage.contains(ImageUsage::VIDEO_DECODE_DPB) {
        flags |= vk::ImageUsageFlags::VIDEO_DECODE_DPB_KHR;
    }
    if usage.contains(ImageUsage::VIDEO_ENCODE_SRC) {
        flags |= vk::ImageUsageFlags::VIDEO_ENCODE_SRC_KHR;
    }
    if usage.contains(ImageUsage::VIDEO_ENCODE_DPB) {
        flags |= vk::ImageUsageFlags::VIDEO_ENCODE_DPB_KHR;
    }
    flags
}

pub(super) fn vk_buffer_usage(usage: BufferUsage) -> vk::BufferUsageFlags {
    let mut flags = vk::BufferUsageFlags::empty();
    if usage.contains(BufferUsage::COPY_SRC) {
        flags |= vk::BufferUsageFlags::TRANSFER_SRC;
    }
    if usage.contains(BufferUsage::COPY_DST) {
        flags |= vk::BufferUsageFlags::TRANSFER_DST;
    }
    if usage.contains(BufferUsage::UNIFORM) {
        flags |= vk::BufferUsageFlags::UNIFORM_BUFFER;
    }
    if usage.contains(BufferUsage::STORAGE) {
        flags |= vk::BufferUsageFlags::STORAGE_BUFFER;
    }
    if usage.contains(BufferUsage::VERTEX) {
        flags |= vk::BufferUsageFlags::VERTEX_BUFFER;
    }
    if usage.contains(BufferUsage::INDEX) {
        flags |= vk::BufferUsageFlags::INDEX_BUFFER;
    }
    if usage.contains(BufferUsage::INDIRECT) {
        flags |= vk::BufferUsageFlags::INDIRECT_BUFFER;
    }
    if usage.contains(BufferUsage::ACCELERATION_STRUCTURE) {
        flags |= vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR;
    }
    if usage.contains(BufferUsage::SHADER_DEVICE_ADDRESS) {
        flags |= vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS;
    }
    if usage.contains(BufferUsage::SHADER_BINDING_TABLE) {
        flags |= vk::BufferUsageFlags::SHADER_BINDING_TABLE_KHR;
    }
    if usage.contains(BufferUsage::VIDEO_ENCODE_DST) {
        flags |= vk::BufferUsageFlags::VIDEO_ENCODE_DST_KHR;
    }
    flags
}

pub(super) fn vk_filter(filter: FilterMode) -> vk::Filter {
    match filter {
        FilterMode::Nearest => vk::Filter::NEAREST,
        FilterMode::Linear => vk::Filter::LINEAR,
        FilterMode::Cubic => vk::Filter::CUBIC_EXT,
    }
}

pub(super) fn vk_mipmap_mode(mode: MipmapMode) -> vk::SamplerMipmapMode {
    match mode {
        MipmapMode::Nearest => vk::SamplerMipmapMode::NEAREST,
        MipmapMode::Linear => vk::SamplerMipmapMode::LINEAR,
    }
}

pub(super) fn vk_address_mode(mode: AddressMode) -> vk::SamplerAddressMode {
    match mode {
        AddressMode::Repeat => vk::SamplerAddressMode::REPEAT,
        AddressMode::MirroredRepeat => vk::SamplerAddressMode::MIRRORED_REPEAT,
        AddressMode::ClampToEdge => vk::SamplerAddressMode::CLAMP_TO_EDGE,
        AddressMode::ClampToBorder => vk::SamplerAddressMode::CLAMP_TO_BORDER,
    }
}

pub(super) fn vk_compare_op(compare: CompareOp) -> vk::CompareOp {
    match compare {
        CompareOp::Never => vk::CompareOp::NEVER,
        CompareOp::Less => vk::CompareOp::LESS,
        CompareOp::Equal => vk::CompareOp::EQUAL,
        CompareOp::LessOrEqual => vk::CompareOp::LESS_OR_EQUAL,
        CompareOp::Greater => vk::CompareOp::GREATER,
        CompareOp::NotEqual => vk::CompareOp::NOT_EQUAL,
        CompareOp::GreaterOrEqual => vk::CompareOp::GREATER_OR_EQUAL,
        CompareOp::Always => vk::CompareOp::ALWAYS,
    }
}

pub(super) fn vk_border_color(color: BorderColor) -> vk::BorderColor {
    match color {
        BorderColor::FloatTransparentBlack => vk::BorderColor::FLOAT_TRANSPARENT_BLACK,
        BorderColor::IntTransparentBlack => vk::BorderColor::INT_TRANSPARENT_BLACK,
        BorderColor::FloatOpaqueBlack => vk::BorderColor::FLOAT_OPAQUE_BLACK,
        BorderColor::IntOpaqueBlack => vk::BorderColor::INT_OPAQUE_BLACK,
        BorderColor::FloatOpaqueWhite => vk::BorderColor::FLOAT_OPAQUE_WHITE,
        BorderColor::IntOpaqueWhite => vk::BorderColor::INT_OPAQUE_WHITE,
        BorderColor::Custom(_) => vk::BorderColor::FLOAT_CUSTOM_EXT,
    }
}

fn vk_custom_border_color_info(
    color: BorderColor,
) -> Option<vk::SamplerCustomBorderColorCreateInfoEXT<'static>> {
    match color {
        BorderColor::Custom(float32) => Some(
            vk::SamplerCustomBorderColorCreateInfoEXT::default()
                .custom_border_color(vk::ClearColorValue { float32 })
                .format(vk::Format::R32G32B32A32_SFLOAT),
        ),
        _ => None,
    }
}

fn vk_sampler_reduction_mode(mode: SamplerReductionMode) -> vk::SamplerReductionMode {
    match mode {
        SamplerReductionMode::WeightedAverage => vk::SamplerReductionMode::WEIGHTED_AVERAGE,
        SamplerReductionMode::Min => vk::SamplerReductionMode::MIN,
        SamplerReductionMode::Max => vk::SamplerReductionMode::MAX,
    }
}

#[cfg(test)]
#[path = "resources_tests.rs"]
mod tests;
