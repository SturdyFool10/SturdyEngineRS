use std::ffi::CStr;

use ash::{Device as AshDevice, Instance, vk};

use crate::{Error, Result};

pub struct DeviceSelection {
    pub physical_device: vk::PhysicalDevice,
    pub graphics_queue_family: u32,
}

pub struct LogicalDevice {
    pub device: AshDevice,
    pub graphics_queue: vk::Queue,
}

impl DeviceSelection {
    pub fn pick(instance: &Instance) -> Result<Self> {
        let physical_devices = unsafe {
            instance.enumerate_physical_devices().map_err(|error| {
                Error::Backend(format!(
                    "failed to enumerate Vulkan physical devices: {error:?}"
                ))
            })?
        };

        physical_devices
            .into_iter()
            .filter_map(|physical_device| {
                let families = unsafe {
                    instance.get_physical_device_queue_family_properties(physical_device)
                };
                families
                    .iter()
                    .enumerate()
                    .find(|(_, family)| {
                        family.queue_count > 0
                            && family.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                    })
                    .map(|(index, _)| Self {
                        physical_device,
                        graphics_queue_family: index as u32,
                    })
            })
            .next()
            .ok_or(Error::Unsupported(
                "no Vulkan physical device with a graphics queue was found",
            ))
    }
}

pub fn create_logical_device(
    instance: &Instance,
    selection: &DeviceSelection,
) -> Result<LogicalDevice> {
    let priority = [1.0f32];
    let queue_info = [vk::DeviceQueueCreateInfo::default()
        .queue_family_index(selection.graphics_queue_family)
        .queue_priorities(&priority)];
    let device_extensions = required_device_extensions();
    let device_extension_ptrs = device_extensions
        .iter()
        .map(|extension| extension.as_ptr())
        .collect::<Vec<_>>();
    let device_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queue_info)
        .enabled_extension_names(&device_extension_ptrs);

    let device = unsafe {
        instance
            .create_device(selection.physical_device, &device_info, None)
            .map_err(|error| Error::Backend(format!("failed to create Vulkan device: {error:?}")))?
    };
    let graphics_queue = unsafe { device.get_device_queue(selection.graphics_queue_family, 0) };

    Ok(LogicalDevice {
        device,
        graphics_queue,
    })
}

pub fn physical_device_name(instance: &Instance, physical_device: vk::PhysicalDevice) -> String {
    let properties = unsafe { instance.get_physical_device_properties(physical_device) };
    let name = unsafe { CStr::from_ptr(properties.device_name.as_ptr()) };
    name.to_string_lossy().into_owned()
}

fn required_device_extensions() -> Vec<&'static CStr> {
    #[cfg(target_os = "macos")]
    {
        vec![
            ash::khr::swapchain::NAME,
            CStr::from_bytes_with_nul(b"VK_KHR_portability_subset\0")
                .expect("static extension name has nul terminator"),
        ]
    }
    #[cfg(not(target_os = "macos"))]
    {
        vec![ash::khr::swapchain::NAME]
    }
}
