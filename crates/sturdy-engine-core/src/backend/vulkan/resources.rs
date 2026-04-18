use std::collections::HashMap;

use ash::{vk, Device};

use crate::{
    AddressMode, BorderColor, BufferDesc, BufferHandle, BufferUsage, CompareOp, Error, FilterMode,
    Format, ImageDesc, ImageHandle, ImageUsage, MipmapMode, Result, SamplerDesc, SamplerHandle,
};

use super::allocator::{Allocation, GpuAllocator};

pub struct ResourceRegistry {
    allocator: GpuAllocator,
    images: HashMap<ImageHandle, VulkanImage>,
    buffers: HashMap<BufferHandle, VulkanBuffer>,
    samplers: HashMap<SamplerHandle, vk::Sampler>,
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
    allocation: Allocation,
}

impl ResourceRegistry {
    pub fn new(memory_properties: vk::PhysicalDeviceMemoryProperties) -> Self {
        Self {
            allocator: GpuAllocator::new(memory_properties),
            images: HashMap::new(),
            buffers: HashMap::new(),
            samplers: HashMap::new(),
        }
    }

    pub fn create_image(
        &mut self,
        device: &Device,
        handle: ImageHandle,
        desc: ImageDesc,
    ) -> Result<()> {
        let info = vk::ImageCreateInfo::default()
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

        let image = unsafe {
            device
                .create_image(&info, None)
                .map_err(|error| Error::Backend(format!("vkCreateImage failed: {error:?}")))?
        };
        let requirements = unsafe { device.get_image_memory_requirements(image) };
        let allocation =
            match self
                .allocator
                .alloc(device, requirements, vk::MemoryPropertyFlags::DEVICE_LOCAL)
            {
                Ok(a) => a,
                Err(error) => {
                    unsafe { device.destroy_image(image, None) };
                    return Err(error);
                }
            };
        unsafe {
            if let Err(error) =
                device.bind_image_memory(image, allocation.memory, allocation.offset)
            {
                self.allocator.dealloc(device, allocation);
                device.destroy_image(image, None);
                return Err(Error::Backend(format!(
                    "vkBindImageMemory failed: {error:?}"
                )));
            }
        }
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk_image_view_type(desc))
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
                Ok(view) => view,
                Err(error) => {
                    self.allocator.dealloc(device, allocation);
                    device.destroy_image(image, None);
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
        let info = vk::ImageCreateInfo::default()
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

        let image = unsafe {
            device
                .create_image(&info, None)
                .map_err(|e| Error::Backend(format!("vkCreateImage (unbound) failed: {e:?}")))?
        };
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk_image_view_type(desc))
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

    pub fn destroy_image(&mut self, device: &Device, handle: ImageHandle) -> Result<()> {
        let image = self.images.remove(&handle).ok_or(Error::InvalidHandle)?;
        if !image.imported {
            unsafe {
                device.destroy_image_view(image.view, None);
                device.destroy_image(image.image, None);
            }
            if let Some(allocation) = image.allocation {
                self.allocator.dealloc(device, allocation);
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

    pub fn create_buffer(
        &mut self,
        device: &Device,
        handle: BufferHandle,
        desc: BufferDesc,
    ) -> Result<()> {
        let info = vk::BufferCreateInfo::default()
            .size(desc.size)
            .usage(vk_buffer_usage(desc.usage))
            .sharing_mode(vk::SharingMode::EXCLUSIVE);

        let buffer = unsafe {
            device
                .create_buffer(&info, None)
                .map_err(|error| Error::Backend(format!("vkCreateBuffer failed: {error:?}")))?
        };
        let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
        let allocation = match self.allocator.alloc(
            device,
            requirements,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
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
                self.allocator.dealloc(device, allocation);
                device.destroy_buffer(buffer, None);
                return Err(Error::Backend(format!(
                    "vkBindBufferMemory failed: {error:?}"
                )));
            }
        }

        self.buffers
            .insert(handle, VulkanBuffer { buffer, allocation });
        Ok(())
    }

    pub fn destroy_buffer(&mut self, device: &Device, handle: BufferHandle) -> Result<()> {
        let buf = self.buffers.remove(&handle).ok_or(Error::InvalidHandle)?;
        unsafe { device.destroy_buffer(buf.buffer, None) };
        self.allocator.dealloc(device, buf.allocation);
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
        let info = vk::SamplerCreateInfo::default()
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
        let sampler = unsafe {
            device
                .create_sampler(&info, None)
                .map_err(|error| Error::Backend(format!("vkCreateSampler failed: {error:?}")))?
        };
        self.samplers.insert(handle, sampler);
        Ok(())
    }

    pub fn destroy_sampler(&mut self, device: &Device, handle: SamplerHandle) -> Result<()> {
        let sampler = self.samplers.remove(&handle).ok_or(Error::InvalidHandle)?;
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
        let base = buf
            .allocation
            .mapped_ptr
            .ok_or_else(|| Error::Backend("buffer allocation is not host-visible".into()))?;
        let start = offset as usize;
        let end = start + data.len();
        if end > buf.allocation.size as usize {
            return Err(Error::InvalidInput(format!(
                "write_buffer out of range: offset={offset} len={} capacity={}",
                data.len(),
                buf.allocation.size
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
        let base = buf
            .allocation
            .mapped_ptr
            .ok_or_else(|| Error::Backend("buffer allocation is not host-visible".into()))?;
        let start = offset as usize;
        let end = start + out.len();
        if end > buf.allocation.size as usize {
            return Err(Error::InvalidInput(format!(
                "read_buffer out of range: offset={offset} len={} capacity={}",
                out.len(),
                buf.allocation.size
            )));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(base.add(start), out.as_mut_ptr(), out.len());
        }
        Ok(())
    }

    pub fn destroy_all(&mut self, device: &Device) {
        for (_, image) in self.images.drain() {
            if !image.imported {
                unsafe {
                    device.destroy_image_view(image.view, None);
                    device.destroy_image(image.image, None);
                }
                if let Some(allocation) = image.allocation {
                    self.allocator.dealloc(device, allocation);
                }
            }
        }
        for (_, buf) in self.buffers.drain() {
            unsafe { device.destroy_buffer(buf.buffer, None) };
            self.allocator.dealloc(device, buf.allocation);
        }
        for (_, sampler) in self.samplers.drain() {
            unsafe {
                device.destroy_sampler(sampler, None);
            }
        }
        self.allocator.destroy_all(device);
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

    pub fn sampler(&self, handle: SamplerHandle) -> Result<vk::Sampler> {
        self.samplers
            .get(&handle)
            .copied()
            .ok_or(Error::InvalidHandle)
    }
}

fn image_type(desc: ImageDesc) -> vk::ImageType {
    if desc.extent.depth > 1 {
        vk::ImageType::TYPE_3D
    } else {
        vk::ImageType::TYPE_2D
    }
}

fn vk_image_view_type(desc: ImageDesc) -> vk::ImageViewType {
    if desc.extent.depth > 1 {
        vk::ImageViewType::TYPE_3D
    } else if desc.layers > 1 {
        vk::ImageViewType::TYPE_2D_ARRAY
    } else {
        vk::ImageViewType::TYPE_2D
    }
}

pub(super) fn vk_format(format: Format) -> Result<vk::Format> {
    match format {
        Format::Unknown => Err(Error::InvalidInput("image format must be specified".into())),
        Format::Rgba8Unorm => Ok(vk::Format::R8G8B8A8_UNORM),
        Format::Bgra8Unorm => Ok(vk::Format::B8G8R8A8_UNORM),
        Format::Rgba16Float => Ok(vk::Format::R16G16B16A16_SFLOAT),
        Format::Rgba32Float => Ok(vk::Format::R32G32B32A32_SFLOAT),
        Format::Depth32Float => Ok(vk::Format::D32_SFLOAT),
        Format::Depth24Stencil8 => Ok(vk::Format::D24_UNORM_S8_UINT),
    }
}

fn vk_aspect_mask(format: Format) -> vk::ImageAspectFlags {
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
    flags
}

fn vk_buffer_usage(usage: BufferUsage) -> vk::BufferUsageFlags {
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
        flags |= vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR;
    }
    flags
}

fn vk_filter(filter: FilterMode) -> vk::Filter {
    match filter {
        FilterMode::Nearest => vk::Filter::NEAREST,
        FilterMode::Linear => vk::Filter::LINEAR,
    }
}

fn vk_mipmap_mode(mode: MipmapMode) -> vk::SamplerMipmapMode {
    match mode {
        MipmapMode::Nearest => vk::SamplerMipmapMode::NEAREST,
        MipmapMode::Linear => vk::SamplerMipmapMode::LINEAR,
    }
}

fn vk_address_mode(mode: AddressMode) -> vk::SamplerAddressMode {
    match mode {
        AddressMode::Repeat => vk::SamplerAddressMode::REPEAT,
        AddressMode::MirroredRepeat => vk::SamplerAddressMode::MIRRORED_REPEAT,
        AddressMode::ClampToEdge => vk::SamplerAddressMode::CLAMP_TO_EDGE,
        AddressMode::ClampToBorder => vk::SamplerAddressMode::CLAMP_TO_BORDER,
    }
}

fn vk_compare_op(compare: CompareOp) -> vk::CompareOp {
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

fn vk_border_color(color: BorderColor) -> vk::BorderColor {
    match color {
        BorderColor::FloatTransparentBlack => vk::BorderColor::FLOAT_TRANSPARENT_BLACK,
        BorderColor::IntTransparentBlack => vk::BorderColor::INT_TRANSPARENT_BLACK,
        BorderColor::FloatOpaqueBlack => vk::BorderColor::FLOAT_OPAQUE_BLACK,
        BorderColor::IntOpaqueBlack => vk::BorderColor::INT_OPAQUE_BLACK,
        BorderColor::FloatOpaqueWhite => vk::BorderColor::FLOAT_OPAQUE_WHITE,
        BorderColor::IntOpaqueWhite => vk::BorderColor::INT_OPAQUE_WHITE,
    }
}
