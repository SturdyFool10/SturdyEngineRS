use std::ffi::{CString, c_void};

use ash::{ext, vk};

// GFX-1g: Callback for VK_EXT_device_address_binding_report events.
// Events arrive through the debug utils messenger when the extension is active.
unsafe extern "system" fn address_binding_report_callback(
    _severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _type_flags: vk::DebugUtilsMessageTypeFlagsEXT,
    _data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _user_data: *mut c_void,
) -> vk::Bool32 {
    #[cfg(debug_assertions)]
    {
        if let Some(data) = unsafe { _data.as_ref() } {
            // Walk the pNext chain for VkDeviceAddressBindingCallbackDataEXT.
            let mut p_next = data.p_next as *const vk::BaseInStructure;
            while !p_next.is_null() {
                let header = unsafe { &*p_next };
                if header.s_type == vk::StructureType::DEVICE_ADDRESS_BINDING_CALLBACK_DATA_EXT {
                    let binding = unsafe {
                        &*(p_next as *const vk::DeviceAddressBindingCallbackDataEXT)
                    };
                    eprintln!(
                        "[address-binding] address={:#x} size={}B type={:?} flags={:?}",
                        binding.base_address,
                        binding.size,
                        binding.binding_type,
                        binding.flags,
                    );
                    break;
                }
                p_next = header.p_next as *const vk::BaseInStructure;
            }
        }
    }
    vk::FALSE
}

/// GFX-1g: Instance-level debug messenger that receives `VK_EXT_device_address_binding_report`
/// events and logs GPU VA binding/unbinding in debug builds.
pub struct AddressBindingMessenger {
    loader: ext::debug_utils::Instance,
    messenger: vk::DebugUtilsMessengerEXT,
}

impl AddressBindingMessenger {
    /// Create the messenger in debug builds when the instance has `VK_EXT_debug_utils`.
    /// Returns `None` in release builds or when messenger creation fails.
    pub fn create(entry: &ash::Entry, instance: &ash::Instance) -> Option<Self> {
        #[cfg(not(debug_assertions))]
        {
            let _ = (entry, instance);
            return None;
        }
        #[cfg(debug_assertions)]
        {
            let loader = ext::debug_utils::Instance::load(entry, instance);
            let create_info = vk::DebugUtilsMessengerCreateInfoEXT::default()
                .message_severity(
                    vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE
                        | vk::DebugUtilsMessageSeverityFlagsEXT::INFO
                        | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                        | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR,
                )
                .message_type(vk::DebugUtilsMessageTypeFlagsEXT::DEVICE_ADDRESS_BINDING)
                .pfn_user_callback(Some(address_binding_report_callback));
            let messenger = unsafe { loader.create_debug_utils_messenger(&create_info, None) }.ok()?;
            Some(Self { loader, messenger })
        }
    }

    /// Destroy the messenger. Must be called before the instance is destroyed.
    pub fn destroy(&mut self) {
        unsafe { self.loader.destroy_debug_utils_messenger(self.messenger, None) };
    }
}

/// Wraps `VK_EXT_debug_utils` for object naming and command-buffer debug regions.
///
/// All methods are no-ops when the extension is unavailable or the loader is `None`.
pub struct DebugUtils {
    loader: Option<ext::debug_utils::Device>,
}

impl DebugUtils {
    pub fn new(instance: &ash::Instance, device: &ash::Device) -> Self {
        let loader = ext::debug_utils::Device::load(instance, device);
        Self {
            loader: Some(loader),
        }
    }

    #[allow(dead_code)]
    pub fn none() -> Self {
        Self { loader: None }
    }

    /// Assign a debug name to a typed Vulkan handle.
    pub fn set_name<T: vk::Handle>(&self, _device: &ash::Device, handle: T, name: &str) {
        let Some(loader) = &self.loader else { return };
        let Ok(c_name) = CString::new(name) else {
            return;
        };
        let info = vk::DebugUtilsObjectNameInfoEXT::default()
            .object_handle(handle)
            .object_name(&c_name);
        let _ = unsafe { loader.set_debug_utils_object_name(&info) };
    }

    /// Push a labeled debug region onto a command buffer.
    pub fn begin_region(&self, cmd: vk::CommandBuffer, label: &str, color: [f32; 4]) {
        let Some(loader) = &self.loader else { return };
        let Ok(c_label) = CString::new(label) else {
            return;
        };
        let info = vk::DebugUtilsLabelEXT::default()
            .label_name(&c_label)
            .color(color);
        unsafe { loader.cmd_begin_debug_utils_label(cmd, &info) };
    }

    /// Pop the most recently pushed debug region from a command buffer.
    pub fn end_region(&self, cmd: vk::CommandBuffer) {
        let Some(loader) = &self.loader else { return };
        unsafe { loader.cmd_end_debug_utils_label(cmd) };
    }
}
