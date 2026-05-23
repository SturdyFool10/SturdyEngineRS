use ash::vk;

use crate::{Error, Result};

pub(super) trait VkResultExt<T> {
    fn trace_vk(self, operation: &'static str) -> Result<T>;
    fn trace_vk_with(self, operation: &'static str, context: impl FnOnce() -> String) -> Result<T>;
}

impl<T> VkResultExt<T> for std::result::Result<T, vk::Result> {
    fn trace_vk(self, operation: &'static str) -> Result<T> {
        self.trace_vk_with(operation, String::new)
    }

    fn trace_vk_with(self, operation: &'static str, context: impl FnOnce() -> String) -> Result<T> {
        self.map_err(|error| {
            let context = context();
            if context.is_empty() {
                tracing::error!(operation, vk_error = ?error, "Vulkan API call failed");
                Error::Backend(format!("{operation} failed: {error:?}"))
            } else {
                tracing::error!(operation, vk_error = ?error, %context, "Vulkan API call failed");
                Error::Backend(format!("{operation} failed: {error:?}; {context}"))
            }
        })
    }
}
