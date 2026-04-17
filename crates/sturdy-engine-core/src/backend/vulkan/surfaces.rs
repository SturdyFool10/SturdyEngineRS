use std::collections::HashMap;

use ash::{Device, Entry, Instance, khr, vk};

use crate::{
    Error, Extent3d, Format, ImageDesc, ImageUsage, NativeSurfaceDesc, Result, SurfaceHandle,
    SurfaceSize,
};

#[derive(Default)]
pub struct SurfaceRegistry {
    surfaces: HashMap<SurfaceHandle, VulkanSurface>,
}

struct VulkanSurface {
    surface_loader: khr::surface::Instance,
    swapchain_loader: khr::swapchain::Device,
    surface: vk::SurfaceKHR,
    swapchain: VulkanSwapchain,
    acquired_image_index: Option<u32>,
    size: SurfaceSize,
}

#[allow(dead_code)]
struct VulkanSwapchain {
    swapchain: vk::SwapchainKHR,
    format: vk::Format,
    color_space: vk::ColorSpaceKHR,
    extent: vk::Extent2D,
    images: Vec<vk::Image>,
    image_views: Vec<vk::ImageView>,
}

pub struct AcquiredSurfaceImage {
    pub image: vk::Image,
    pub image_view: vk::ImageView,
    pub desc: ImageDesc,
}

impl SurfaceRegistry {
    pub fn create_surface(
        &mut self,
        entry: &Entry,
        instance: &Instance,
        device: &Device,
        physical_device: vk::PhysicalDevice,
        queue_family: u32,
        handle: SurfaceHandle,
        desc: NativeSurfaceDesc,
    ) -> Result<()> {
        let surface = unsafe {
            ash_window::create_surface(
                entry,
                instance,
                desc.display_handle,
                desc.window_handle,
                None,
            )
            .map_err(|error| Error::Backend(format!("vkCreateSurfaceKHR failed: {error:?}")))?
        };
        let surface_loader = khr::surface::Instance::new(entry, instance);
        let swapchain_loader = khr::swapchain::Device::new(instance, device);

        let swapchain = match (|| {
            let present_supported = unsafe {
                surface_loader
                    .get_physical_device_surface_support(physical_device, queue_family, surface)
                    .map_err(|error| {
                        Error::Backend(format!(
                            "vkGetPhysicalDeviceSurfaceSupportKHR failed: {error:?}"
                        ))
                    })?
            };
            if !present_supported {
                return Err(Error::Unsupported(
                    "selected Vulkan graphics queue family cannot present to this surface",
                ));
            }

            create_swapchain(
                device,
                &surface_loader,
                &swapchain_loader,
                physical_device,
                surface,
                desc.size,
                vk::SwapchainKHR::null(),
            )
        })() {
            Ok(swapchain) => swapchain,
            Err(error) => {
                unsafe {
                    surface_loader.destroy_surface(surface, None);
                }
                return Err(error);
            }
        };
        self.surfaces.insert(
            handle,
            VulkanSurface {
                surface_loader,
                swapchain_loader,
                surface,
                swapchain,
                acquired_image_index: None,
                size: desc.size,
            },
        );
        Ok(())
    }

    pub fn resize_surface(
        &mut self,
        device: &Device,
        physical_device: vk::PhysicalDevice,
        handle: SurfaceHandle,
        size: SurfaceSize,
    ) -> Result<()> {
        let surface = self.surfaces.get_mut(&handle).ok_or(Error::InvalidHandle)?;
        if surface.acquired_image_index.is_some() {
            return Err(Error::InvalidInput(
                "cannot resize a Vulkan surface while an image is acquired".into(),
            ));
        }
        let old_swapchain = surface.swapchain.swapchain;
        let new_swapchain = create_swapchain(
            device,
            &surface.surface_loader,
            &surface.swapchain_loader,
            physical_device,
            surface.surface,
            size,
            old_swapchain,
        )?;
        destroy_swapchain(device, &surface.swapchain_loader, &mut surface.swapchain);
        surface.swapchain = new_swapchain;
        surface.size = size;
        Ok(())
    }

    pub fn acquire_image(
        &mut self,
        device: &Device,
        handle: SurfaceHandle,
    ) -> Result<AcquiredSurfaceImage> {
        let surface = self.surfaces.get_mut(&handle).ok_or(Error::InvalidHandle)?;
        if surface.acquired_image_index.is_some() {
            return Err(Error::InvalidInput(
                "surface already has an acquired image".into(),
            ));
        }

        let fence_info = vk::FenceCreateInfo::default();
        let fence = unsafe {
            device
                .create_fence(&fence_info, None)
                .map_err(|error| Error::Backend(format!("vkCreateFence failed: {error:?}")))?
        };
        let acquire_result = unsafe {
            surface.swapchain_loader.acquire_next_image(
                surface.swapchain.swapchain,
                u64::MAX,
                vk::Semaphore::null(),
                fence,
            )
        };
        let (image_index, _suboptimal) = match acquire_result {
            Ok(result) => result,
            Err(error) => unsafe {
                device.destroy_fence(fence, None);
                return Err(Error::Backend(format!(
                    "vkAcquireNextImageKHR failed: {error:?}"
                )));
            },
        };
        unsafe {
            if let Err(error) = device.wait_for_fences(&[fence], true, u64::MAX) {
                device.destroy_fence(fence, None);
                return Err(Error::Backend(format!("vkWaitForFences failed: {error:?}")));
            }
            device.destroy_fence(fence, None);
        }

        surface.acquired_image_index = Some(image_index);
        let image_index = image_index as usize;
        Ok(AcquiredSurfaceImage {
            image: surface.swapchain.images[image_index],
            image_view: surface.swapchain.image_views[image_index],
            desc: ImageDesc {
                extent: Extent3d {
                    width: surface.swapchain.extent.width,
                    height: surface.swapchain.extent.height,
                    depth: 1,
                },
                mip_levels: 1,
                layers: 1,
                samples: 1,
                format: vk_format_to_engine(surface.swapchain.format)?,
                usage: ImageUsage::RENDER_TARGET | ImageUsage::PRESENT | ImageUsage::COPY_DST,
            },
        })
    }

    pub fn present(&mut self, queue: vk::Queue, handle: SurfaceHandle) -> Result<()> {
        let surface = self.surfaces.get_mut(&handle).ok_or(Error::InvalidHandle)?;
        let image_index = surface.acquired_image_index.ok_or_else(|| {
            Error::InvalidInput("surface present requires an acquired image".into())
        })?;
        let swapchains = [surface.swapchain.swapchain];
        let image_indices = [image_index];
        let present_info = vk::PresentInfoKHR::default()
            .swapchains(&swapchains)
            .image_indices(&image_indices);
        unsafe {
            surface
                .swapchain_loader
                .queue_present(queue, &present_info)
                .map_err(|error| Error::Backend(format!("vkQueuePresentKHR failed: {error:?}")))?;
        }
        surface.acquired_image_index = None;
        Ok(())
    }

    pub fn destroy_surface(&mut self, device: &Device, handle: SurfaceHandle) -> Result<()> {
        let mut surface = self.surfaces.remove(&handle).ok_or(Error::InvalidHandle)?;
        destroy_surface(device, &mut surface);
        Ok(())
    }

    pub fn destroy_all(&mut self, device: &Device) {
        for (_, mut surface) in self.surfaces.drain() {
            destroy_surface(device, &mut surface);
        }
    }
}

fn destroy_surface(device: &Device, surface: &mut VulkanSurface) {
    destroy_swapchain(device, &surface.swapchain_loader, &mut surface.swapchain);
    unsafe {
        surface
            .surface_loader
            .destroy_surface(surface.surface, None);
    }
}

fn create_swapchain(
    device: &Device,
    surface_loader: &khr::surface::Instance,
    swapchain_loader: &khr::swapchain::Device,
    physical_device: vk::PhysicalDevice,
    surface: vk::SurfaceKHR,
    size: SurfaceSize,
    old_swapchain: vk::SwapchainKHR,
) -> Result<VulkanSwapchain> {
    let capabilities = unsafe {
        surface_loader
            .get_physical_device_surface_capabilities(physical_device, surface)
            .map_err(|error| {
                Error::Backend(format!(
                    "vkGetPhysicalDeviceSurfaceCapabilitiesKHR failed: {error:?}"
                ))
            })?
    };
    let formats = unsafe {
        surface_loader
            .get_physical_device_surface_formats(physical_device, surface)
            .map_err(|error| {
                Error::Backend(format!(
                    "vkGetPhysicalDeviceSurfaceFormatsKHR failed: {error:?}"
                ))
            })?
    };
    let present_modes = unsafe {
        surface_loader
            .get_physical_device_surface_present_modes(physical_device, surface)
            .map_err(|error| {
                Error::Backend(format!(
                    "vkGetPhysicalDeviceSurfacePresentModesKHR failed: {error:?}"
                ))
            })?
    };

    let format = choose_surface_format(&formats)?;
    let present_mode = choose_present_mode(&present_modes);
    let extent = choose_extent(&capabilities, size);
    let image_count = choose_image_count(&capabilities);
    let create_info = vk::SwapchainCreateInfoKHR::default()
        .surface(surface)
        .min_image_count(image_count)
        .image_format(format.format)
        .image_color_space(format.color_space)
        .image_extent(extent)
        .image_array_layers(1)
        .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST)
        .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
        .pre_transform(capabilities.current_transform)
        .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
        .present_mode(present_mode)
        .clipped(true)
        .old_swapchain(old_swapchain);
    let swapchain = unsafe {
        swapchain_loader
            .create_swapchain(&create_info, None)
            .map_err(|error| Error::Backend(format!("vkCreateSwapchainKHR failed: {error:?}")))?
    };
    let images = unsafe {
        swapchain_loader
            .get_swapchain_images(swapchain)
            .map_err(|error| Error::Backend(format!("vkGetSwapchainImagesKHR failed: {error:?}")))?
    };
    let mut image_views = Vec::with_capacity(images.len());
    for image in images.iter().copied() {
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(format.format)
            .subresource_range(
                vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .base_mip_level(0)
                    .level_count(1)
                    .base_array_layer(0)
                    .layer_count(1),
            );
        let image_view = unsafe {
            match device.create_image_view(&view_info, None) {
                Ok(image_view) => image_view,
                Err(error) => {
                    for image_view in image_views.drain(..) {
                        device.destroy_image_view(image_view, None);
                    }
                    swapchain_loader.destroy_swapchain(swapchain, None);
                    return Err(Error::Backend(format!(
                        "vkCreateImageView failed: {error:?}"
                    )));
                }
            }
        };
        image_views.push(image_view);
    }

    Ok(VulkanSwapchain {
        swapchain,
        format: format.format,
        color_space: format.color_space,
        extent,
        images,
        image_views,
    })
}

fn choose_surface_format(formats: &[vk::SurfaceFormatKHR]) -> Result<vk::SurfaceFormatKHR> {
    if formats.is_empty() {
        return Err(Error::Unsupported(
            "Vulkan surface did not report any supported formats",
        ));
    }
    formats
        .iter()
        .copied()
        .find(|format| {
            matches!(
                format.format,
                vk::Format::B8G8R8A8_UNORM
                    | vk::Format::R8G8B8A8_UNORM
                    | vk::Format::R16G16B16A16_SFLOAT
                    | vk::Format::R32G32B32A32_SFLOAT
            ) && format.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
        })
        .or_else(|| {
            formats.iter().copied().find(|format| {
                matches!(
                    format.format,
                    vk::Format::B8G8R8A8_UNORM
                        | vk::Format::R8G8B8A8_UNORM
                        | vk::Format::R16G16B16A16_SFLOAT
                        | vk::Format::R32G32B32A32_SFLOAT
                )
            })
        })
        .ok_or(Error::Unsupported(
            "Vulkan surface did not report a format supported by the engine",
        ))
}

fn choose_present_mode(present_modes: &[vk::PresentModeKHR]) -> vk::PresentModeKHR {
    present_modes
        .iter()
        .copied()
        .find(|mode| *mode == vk::PresentModeKHR::MAILBOX)
        .unwrap_or(vk::PresentModeKHR::FIFO)
}

fn choose_extent(capabilities: &vk::SurfaceCapabilitiesKHR, size: SurfaceSize) -> vk::Extent2D {
    if capabilities.current_extent.width != u32::MAX {
        return capabilities.current_extent;
    }
    vk::Extent2D {
        width: size.width.clamp(
            capabilities.min_image_extent.width,
            capabilities.max_image_extent.width,
        ),
        height: size.height.clamp(
            capabilities.min_image_extent.height,
            capabilities.max_image_extent.height,
        ),
    }
}

fn choose_image_count(capabilities: &vk::SurfaceCapabilitiesKHR) -> u32 {
    let preferred = capabilities.min_image_count.saturating_add(1).max(2);
    if capabilities.max_image_count == 0 {
        preferred
    } else {
        preferred.min(capabilities.max_image_count)
    }
}

fn destroy_swapchain(
    device: &Device,
    swapchain_loader: &khr::swapchain::Device,
    swapchain: &mut VulkanSwapchain,
) {
    unsafe {
        for image_view in swapchain.image_views.drain(..) {
            device.destroy_image_view(image_view, None);
        }
        swapchain_loader.destroy_swapchain(swapchain.swapchain, None);
    }
}

fn vk_format_to_engine(format: vk::Format) -> Result<Format> {
    match format {
        vk::Format::B8G8R8A8_UNORM => Ok(Format::Bgra8Unorm),
        vk::Format::R8G8B8A8_UNORM => Ok(Format::Rgba8Unorm),
        vk::Format::R16G16B16A16_SFLOAT => Ok(Format::Rgba16Float),
        vk::Format::R32G32B32A32_SFLOAT => Ok(Format::Rgba32Float),
        _ => Err(Error::Unsupported(
            "Vulkan surface format is not supported by the engine",
        )),
    }
}
