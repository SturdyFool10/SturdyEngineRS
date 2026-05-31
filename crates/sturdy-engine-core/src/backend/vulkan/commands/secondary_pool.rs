use ash::{Device, vk};

use crate::{Error, Result};

/// One command pool + secondary command buffer per parallel recording slot.
///
/// Pools are reset each frame (after the previous frame fence signals).
/// The command buffer is always secondary-level so it can be recorded
/// from any thread and later executed via `vkCmdExecuteCommands`.
pub(super) struct SecondaryPool {
    pub(super) pool: vk::CommandPool,
    pub(super) cmd: vk::CommandBuffer,
}

impl SecondaryPool {
    pub(super) fn create(device: &Device, queue_family: u32) -> Result<Self> {
        let pool_info = vk::CommandPoolCreateInfo::default()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(queue_family);
        let pool = unsafe {
            device
                .create_command_pool(&pool_info, None)
                .map_err(|e| Error::Backend(format!("vkCreateCommandPool (secondary): {e:?}")))?
        };
        let alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(pool)
            .level(vk::CommandBufferLevel::SECONDARY)
            .command_buffer_count(1);
        let cmd = unsafe {
            match device.allocate_command_buffers(&alloc_info) {
                Ok(bufs) => bufs[0],
                Err(e) => {
                    device.destroy_command_pool(pool, None);
                    return Err(Error::Backend(format!(
                        "vkAllocateCommandBuffers (secondary): {e:?}"
                    )));
                }
            }
        };
        Ok(Self { pool, cmd })
    }

    pub(super) fn destroy(&self, device: &Device) {
        unsafe {
            device.destroy_command_pool(self.pool, None);
        }
    }
}

/// A `vk::CommandBuffer` (secondary) that is safe to send to a rayon worker.
///
/// # Safety
///
/// The caller must ensure that each `SecondaryRecordToken` is used from at
/// most one thread at a time and that no primary command buffer references it
/// while it is being recorded.  All of these invariants are upheld by
/// [`CommandContext::record_parallel_compute`], which distributes one token
/// per rayon task and waits for all tasks before calling `vkCmdExecuteCommands`.
#[derive(Clone, Copy)]
pub(crate) struct SecondaryRecordToken(pub(super) vk::CommandBuffer);

// SAFETY: vk::CommandBuffer is a u64 handle.  Vulkan guarantees that recording
// to distinct command buffers from distinct threads is safe.  We enforce the
// "one thread per handle" contract at the call site.
unsafe impl Send for SecondaryRecordToken {}
unsafe impl Sync for SecondaryRecordToken {}
