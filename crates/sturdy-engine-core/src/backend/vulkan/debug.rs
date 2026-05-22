use std::ffi::{CStr, CString, c_void};

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
                    let binding =
                        unsafe { &*(p_next as *const vk::DeviceAddressBindingCallbackDataEXT) };
                    tracing::debug!(
                        target: "vulkan::address_binding",
                        address = binding.base_address,
                        size_bytes = binding.size,
                        binding_type = ?binding.binding_type,
                        flags = ?binding.flags,
                        "GPU address binding event"
                    );
                    break;
                }
                p_next = header.p_next as *const vk::BaseInStructure;
            }
        }
    }
    vk::FALSE
}

// ── Validation messenger ──────────────────────────────────────────────────────

unsafe extern "system" fn validation_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    type_flags: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _user_data: *mut c_void,
) -> vk::Bool32 {
    let Some(data) = (unsafe { data.as_ref() }) else {
        return vk::FALSE;
    };

    let type_tag = if type_flags.contains(vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION) {
        "VALIDATION"
    } else if type_flags.contains(vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE) {
        "PERFORMANCE"
    } else {
        "GENERAL"
    };

    let msg_id = if data.p_message_id_name.is_null() {
        "<unknown>"
    } else {
        unsafe { CStr::from_ptr(data.p_message_id_name) }
            .to_str()
            .unwrap_or("<invalid utf8>")
    };

    let msg = if data.p_message.is_null() {
        "<no message>"
    } else {
        unsafe { CStr::from_ptr(data.p_message) }
            .to_str()
            .unwrap_or("<invalid utf8>")
    };

    // Build object detail string for structured logging.
    let obj_count = data.object_count as usize;
    let objects = unsafe { std::slice::from_raw_parts(data.p_objects, obj_count) };
    let mut object_detail = String::new();
    for obj in objects {
        let name = if obj.p_object_name.is_null() {
            None
        } else {
            unsafe { CStr::from_ptr(obj.p_object_name) }.to_str().ok()
        };
        object_detail.push_str(&format!(
            "\n  object: type={:?} handle={:#x}{}",
            obj.object_type,
            obj.object_handle,
            name.map(|n| format!(" name={n}")).unwrap_or_default()
        ));
    }

    match severity {
        s if s.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) => {
            tracing::error!(target: "vulkan", "[{type_tag}] {msg_id}: {msg}{object_detail}");
        }
        s if s.contains(vk::DebugUtilsMessageSeverityFlagsEXT::WARNING) => {
            tracing::warn!(target: "vulkan", "[{type_tag}] {msg_id}: {msg}{object_detail}");
        }
        s if s.contains(vk::DebugUtilsMessageSeverityFlagsEXT::INFO) => {
            tracing::info!(target: "vulkan", "[{type_tag}] {msg_id}: {msg}");
        }
        _ => {
            tracing::debug!(target: "vulkan", "[{type_tag}] {msg_id}: {msg}");
        }
    }

    // Abort on errors so the call-stack is visible in the debugger / backtrace.
    if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
        tracing::error!(target: "vulkan", "aborting on validation error — set RUST_BACKTRACE=1");
        std::process::abort();
    }

    vk::FALSE
}

/// Validation-layer messenger: captures all severity levels and message types
/// from `VK_LAYER_KHRONOS_validation`.  Only active in debug builds when the
/// layer and `VK_EXT_debug_utils` are both available.
pub struct ValidationMessenger {
    loader: ext::debug_utils::Instance,
    messenger: vk::DebugUtilsMessengerEXT,
}

impl ValidationMessenger {
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
                    vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                        | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                        | vk::DebugUtilsMessageSeverityFlagsEXT::INFO,
                )
                .message_type(
                    vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                        | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                        | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
                )
                .pfn_user_callback(Some(validation_callback));
            let messenger =
                unsafe { loader.create_debug_utils_messenger(&create_info, None) }.ok()?;
            Some(Self { loader, messenger })
        }
    }

    pub fn destroy(&mut self) {
        unsafe {
            self.loader
                .destroy_debug_utils_messenger(self.messenger, None)
        };
    }
}

// ── Address-binding messenger ─────────────────────────────────────────────────

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
            let messenger =
                unsafe { loader.create_debug_utils_messenger(&create_info, None) }.ok()?;
            Some(Self { loader, messenger })
        }
    }

    /// Destroy the messenger. Must be called before the instance is destroyed.
    pub fn destroy(&mut self) {
        unsafe {
            self.loader
                .destroy_debug_utils_messenger(self.messenger, None)
        };
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
