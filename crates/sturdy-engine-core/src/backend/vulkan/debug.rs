use std::ffi::CString;

use ash::{ext, vk};

/// Wraps `VK_EXT_debug_utils` for object naming and command-buffer debug regions.
///
/// All methods are no-ops when the extension is unavailable or the loader is `None`.
pub struct DebugUtils {
    loader: Option<ext::debug_utils::Device>,
}

impl DebugUtils {
    pub fn new(instance: &ash::Instance, device: &ash::Device) -> Self {
        let loader = ext::debug_utils::Device::new(instance, device);
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
