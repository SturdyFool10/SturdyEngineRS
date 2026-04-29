use std::ffi::{CStr, CString};

use ash::{Entry, Instance, vk};

use crate::{Error, Result};

use super::VulkanBackendConfig;

pub fn load_entry() -> Result<Entry> {
    unsafe {
        Entry::load()
            .map_err(|error| Error::Backend(format!("failed to load Vulkan loader: {error}")))
    }
}

pub fn create_instance(entry: &Entry, config: &VulkanBackendConfig) -> Result<Instance> {
    //panic allowed, reason = "static string literal cannot contain NUL bytes"
    let app_name = CString::new("SturdyEngine").expect("static string has no nul");
    //panic allowed, reason = "static string literal cannot contain NUL bytes"
    let engine_name = CString::new("SturdyEngine").expect("static string has no nul");
    let app_info = vk::ApplicationInfo::default()
        .application_name(&app_name)
        .application_version(vk::make_api_version(0, 0, 1, 0))
        .engine_name(&engine_name)
        .engine_version(vk::make_api_version(0, 0, 1, 0))
        .api_version(vk::API_VERSION_1_2);

    let layer_names = requested_layers(entry, config.validation)?;
    let layer_ptrs = layer_names
        .iter()
        .map(|layer| layer.as_ptr())
        .collect::<Vec<_>>();
    let extension_names = required_instance_extensions(entry)?;
    let extension_ptrs = extension_names
        .iter()
        .map(|extension| extension.as_ptr())
        .collect::<Vec<_>>();

    let instance_info = vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .enabled_layer_names(&layer_ptrs)
        .enabled_extension_names(&extension_ptrs);
    #[cfg(target_os = "macos")]
    let instance_info = instance_info.flags(vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = &instance_info;
    }

    unsafe {
        entry
            .create_instance(&instance_info, None)
            .map_err(|error| Error::Backend(format!("failed to create Vulkan instance: {error:?}")))
    }
}

fn requested_layers(entry: &Entry, validation: bool) -> Result<Vec<CString>> {
    if !validation {
        return Ok(Vec::new());
    }

    //panic allowed, reason = "static string literal cannot contain NUL bytes"
    let wanted = CString::new("VK_LAYER_KHRONOS_validation").expect("static string has no nul");
    let layers = unsafe {
        entry
            .enumerate_instance_layer_properties()
            .map_err(|error| {
                Error::Backend(format!("failed to enumerate Vulkan layers: {error:?}"))
            })?
    };
    let available = layers.iter().any(|layer| {
        let name = unsafe { CStr::from_ptr(layer.layer_name.as_ptr()) };
        name == wanted.as_c_str()
    });

    Ok(available.then_some(wanted).into_iter().collect())
}

#[allow(dead_code)]
pub fn has_debug_utils_extension(entry: &Entry) -> bool {
    unsafe {
        entry
            .enumerate_instance_extension_properties(None)
            .unwrap_or_default()
            .iter()
            .any(|ext| {
                let name = CStr::from_ptr(ext.extension_name.as_ptr());
                name.to_bytes() == b"VK_EXT_debug_utils"
            })
    }
}

fn required_instance_extensions(entry: &Entry) -> Result<Vec<CString>> {
    let available = unsafe {
        entry
            .enumerate_instance_extension_properties(None)
            .map_err(|error| {
                Error::Backend(format!(
                    "failed to enumerate Vulkan instance extensions: {error:?}"
                ))
            })?
    };
    let has_extension = |name: &CStr| {
        available.iter().any(|extension| {
            let available_name = unsafe { CStr::from_ptr(extension.extension_name.as_ptr()) };
            available_name == name
        })
    };

    let mut extensions = Vec::new();
    push_extension(&mut extensions, &has_extension, "VK_KHR_surface");

    #[cfg(target_os = "windows")]
    push_extension(&mut extensions, &has_extension, "VK_KHR_win32_surface");

    #[cfg(target_os = "linux")]
    {
        push_extension(&mut extensions, &has_extension, "VK_KHR_xlib_surface");
        push_extension(&mut extensions, &has_extension, "VK_KHR_xcb_surface");
        push_extension(&mut extensions, &has_extension, "VK_KHR_wayland_surface");
    }

    #[cfg(target_os = "macos")]
    {
        push_extension(&mut extensions, &has_extension, "VK_EXT_metal_surface");
        push_extension(
            &mut extensions,
            &has_extension,
            "VK_KHR_portability_enumeration",
        );
    }

    push_extension(&mut extensions, &has_extension, "VK_EXT_debug_utils");

    Ok(extensions)
}

fn push_extension(
    extensions: &mut Vec<CString>,
    has_extension: &impl Fn(&CStr) -> bool,
    name: &'static str,
) {
    //panic allowed, reason = "static &str parameter cannot contain NUL bytes"
    let extension = CString::new(name).expect("static extension name has no nul bytes");
    if has_extension(&extension) {
        extensions.push(extension);
    }
}
