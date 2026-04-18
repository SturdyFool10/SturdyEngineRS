use ash::{Device, vk};

use crate::{Error, Result};

/// One command pool + command buffer per graph batch slot.
///
/// Pools are reset at the start of a frame and command buffers are reused
/// across submissions after the previous frame fence has signaled.
pub(super) struct BatchPool {
    pub(super) pool: vk::CommandPool,
    pub(super) command_buffer: vk::CommandBuffer,
}

impl BatchPool {
    pub(super) fn create(device: &Device, queue_family: u32) -> Result<Self> {
        let pool_info = vk::CommandPoolCreateInfo::default()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(queue_family);
        let pool = unsafe {
            device
                .create_command_pool(&pool_info, None)
                .map_err(|e| Error::Backend(format!("vkCreateCommandPool failed: {e:?}")))?
        };
        let alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        let command_buffer = unsafe {
            match device.allocate_command_buffers(&alloc_info) {
                Ok(bufs) => bufs[0],
                Err(e) => {
                    device.destroy_command_pool(pool, None);
                    return Err(Error::Backend(format!(
                        "vkAllocateCommandBuffers failed: {e:?}"
                    )));
                }
            }
        };
        Ok(Self {
            pool,
            command_buffer,
        })
    }

    pub(super) fn destroy(&self, device: &Device) {
        unsafe {
            device.destroy_command_pool(self.pool, None);
        }
    }
}
