use ash::{Instance, vk};

use crate::Caps;

pub fn query_caps(instance: &Instance, physical_device: vk::PhysicalDevice) -> Caps {
    let properties = unsafe { instance.get_physical_device_properties(physical_device) };
    let max_dimension = properties.limits.max_image_dimension2_d.max(1);
    let max_mip_levels = u32::BITS - max_dimension.leading_zeros();

    Caps {
        supports_raytracing: false,
        supports_mesh_shading: false,
        supports_bindless: false,
        max_mip_levels,
        max_frames_in_flight: 2,
    }
}
