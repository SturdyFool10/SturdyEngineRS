use std::ffi::CStr;

use ash::{Instance, vk};

use crate::{AdapterInfo, AdapterKind, AdapterSelection, BackendKind, Error, Result};

pub fn enumerate(instance: &Instance) -> Vec<AdapterInfo> {
    let physical_devices = unsafe { instance.enumerate_physical_devices().unwrap_or_default() };
    physical_devices
        .into_iter()
        .map(|pd| adapter_info(instance, pd))
        .collect()
}

pub fn pick(instance: &Instance, selection: &AdapterSelection) -> Result<vk::PhysicalDevice> {
    let physical_devices = unsafe {
        instance
            .enumerate_physical_devices()
            .map_err(|e| Error::Backend(format!("failed to enumerate physical devices: {e:?}")))?
    };

    if physical_devices.is_empty() {
        return Err(Error::Unsupported("no Vulkan physical devices found"));
    }

    let chosen = match selection {
        AdapterSelection::Auto => pick_best(instance, &physical_devices),
        AdapterSelection::ByIndex(idx) => physical_devices.get(*idx).copied(),
        AdapterSelection::ByName(name) => physical_devices
            .iter()
            .copied()
            .find(|&pd| device_name(instance, pd).contains(name.as_str())),
        AdapterSelection::ByVendorId(vid) => physical_devices.iter().copied().find(|&pd| {
            let props = unsafe { instance.get_physical_device_properties(pd) };
            props.vendor_id == *vid
        }),
        AdapterSelection::ByKind(kind) => physical_devices.iter().copied().find(|&pd| {
            let props = unsafe { instance.get_physical_device_properties(pd) };
            vk_type_to_kind(props.device_type) == *kind
        }),
    };

    chosen.ok_or(Error::Unsupported(
        "no Vulkan physical device matched the requested adapter selection",
    ))
}

fn pick_best(instance: &Instance, devices: &[vk::PhysicalDevice]) -> Option<vk::PhysicalDevice> {
    devices.iter().copied().max_by_key(|&pd| {
        let props = unsafe { instance.get_physical_device_properties(pd) };
        match props.device_type {
            vk::PhysicalDeviceType::DISCRETE_GPU => 4,
            vk::PhysicalDeviceType::INTEGRATED_GPU => 3,
            vk::PhysicalDeviceType::VIRTUAL_GPU => 2,
            vk::PhysicalDeviceType::CPU => 1,
            _ => 0,
        }
    })
}

fn adapter_info(instance: &Instance, pd: vk::PhysicalDevice) -> AdapterInfo {
    let props = unsafe { instance.get_physical_device_properties(pd) };
    let families = unsafe { instance.get_physical_device_queue_family_properties(pd) };
    let mem_props = unsafe { instance.get_physical_device_memory_properties(pd) };

    let graphics_queue_count = families
        .iter()
        .filter(|f| f.queue_flags.contains(vk::QueueFlags::GRAPHICS))
        .map(|f| f.queue_count)
        .sum();
    let compute_queue_count = families
        .iter()
        .filter(|f| f.queue_flags.contains(vk::QueueFlags::COMPUTE))
        .map(|f| f.queue_count)
        .sum();
    let transfer_queue_count = families
        .iter()
        .filter(|f| f.queue_flags.contains(vk::QueueFlags::TRANSFER))
        .map(|f| f.queue_count)
        .sum();

    let vram_bytes: u64 = mem_props.memory_heaps[..mem_props.memory_heap_count as usize]
        .iter()
        .filter(|h| h.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL))
        .map(|h| h.size)
        .sum();

    let is_software = matches!(
        props.device_type,
        vk::PhysicalDeviceType::CPU | vk::PhysicalDeviceType::OTHER
    );

    AdapterInfo {
        name: device_name(instance, pd),
        vendor_id: props.vendor_id,
        device_id: props.device_id,
        kind: vk_type_to_kind(props.device_type),
        backend: BackendKind::Vulkan,
        driver_version: props.driver_version,
        driver_name: None,
        graphics_queue_count,
        compute_queue_count,
        transfer_queue_count,
        vram_bytes,
        is_software,
        api_version: props.api_version,
    }
}

pub fn vk_type_to_kind(device_type: vk::PhysicalDeviceType) -> AdapterKind {
    match device_type {
        vk::PhysicalDeviceType::DISCRETE_GPU => AdapterKind::DiscreteGpu,
        vk::PhysicalDeviceType::INTEGRATED_GPU => AdapterKind::IntegratedGpu,
        vk::PhysicalDeviceType::VIRTUAL_GPU => AdapterKind::VirtualGpu,
        vk::PhysicalDeviceType::CPU => AdapterKind::Cpu,
        _ => AdapterKind::Unknown,
    }
}

fn device_name(instance: &Instance, pd: vk::PhysicalDevice) -> String {
    let props = unsafe { instance.get_physical_device_properties(pd) };
    unsafe {
        CStr::from_ptr(props.device_name.as_ptr())
            .to_string_lossy()
            .into_owned()
    }
}
