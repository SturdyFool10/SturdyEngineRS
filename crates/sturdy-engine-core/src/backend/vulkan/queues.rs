use ash::vk;

use crate::QueueType;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct QueueFamilyMap {
    pub graphics: u32,
    pub compute: u32,
    pub transfer: u32,
    /// GFX-4a: dedicated video decode queue family when `VK_QUEUE_VIDEO_DECODE_BIT_KHR` is available.
    /// Falls back to graphics when absent (video sessions still work on the graphics queue).
    pub video_decode: u32,
    /// GFX-4a: dedicated video encode queue family when `VK_QUEUE_VIDEO_ENCODE_BIT_KHR` is available.
    pub video_encode: u32,
    /// Track 11b: dedicated async compute queue family (COMPUTE but not GRAPHICS).
    /// Falls back to the unified compute family when absent.
    pub async_compute: u32,
    /// Track 11b: dedicated DMA/transfer queue family (TRANSFER only, no GRAPHICS/COMPUTE).
    /// Used for texture uploads and buffer copies without stalling the graphics queue.
    /// Falls back to the graphics family when absent.
    pub dma: u32,
}

impl QueueFamilyMap {
    pub fn unified(graphics: u32) -> Self {
        Self {
            graphics,
            compute: graphics,
            transfer: graphics,
            video_decode: graphics,
            video_encode: graphics,
            async_compute: graphics,
            dma: graphics,
        }
    }

    /// Find the best dedicated DMA queue family (TRANSFER only, no GRAPHICS or COMPUTE).
    pub fn select_dma(families: &[vk::QueueFamilyProperties], graphics: u32) -> u32 {
        // Prefer pure TRANSFER queue (DMA engine on discrete GPUs).
        for (i, f) in families.iter().enumerate() {
            if f.queue_count > 0
                && f.queue_flags.contains(vk::QueueFlags::TRANSFER)
                && !f.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                && !f.queue_flags.contains(vk::QueueFlags::COMPUTE)
            {
                return i as u32;
            }
        }
        // Fall back to any queue with TRANSFER support.
        for (i, f) in families.iter().enumerate() {
            if f.queue_count > 0 && f.queue_flags.contains(vk::QueueFlags::TRANSFER) {
                return i as u32;
            }
        }
        graphics
    }


    /// Find the best dedicated async compute queue family (COMPUTE, no GRAPHICS).
    pub fn select_async_compute(families: &[vk::QueueFamilyProperties], compute: u32) -> u32 {
        for (i, f) in families.iter().enumerate() {
            if f.queue_count > 0
                && f.queue_flags.contains(vk::QueueFlags::COMPUTE)
                && !f.queue_flags.contains(vk::QueueFlags::GRAPHICS)
            {
                return i as u32;
            }
        }
        compute
    }


    /// Select the best queue family for video decode operations.
    /// Prefers a dedicated video decode queue; falls back to graphics.
    pub fn select_video_decode(
        families: &[vk::QueueFamilyProperties],
        graphics: u32,
    ) -> u32 {
        // Prefer a queue that supports VIDEO_DECODE but not GRAPHICS (dedicated decode).
        for (i, f) in families.iter().enumerate() {
            if f.queue_count > 0
                && f.queue_flags.contains(vk::QueueFlags::VIDEO_DECODE_KHR)
                && !f.queue_flags.contains(vk::QueueFlags::GRAPHICS)
            {
                return i as u32;
            }
        }
        // Fall back to any queue with VIDEO_DECODE support (may overlap graphics).
        for (i, f) in families.iter().enumerate() {
            if f.queue_count > 0 && f.queue_flags.contains(vk::QueueFlags::VIDEO_DECODE_KHR) {
                return i as u32;
            }
        }
        graphics
    }

    /// Select the best queue family for video encode operations.
    pub fn select_video_encode(
        families: &[vk::QueueFamilyProperties],
        graphics: u32,
    ) -> u32 {
        for (i, f) in families.iter().enumerate() {
            if f.queue_count > 0
                && f.queue_flags.contains(vk::QueueFlags::VIDEO_ENCODE_KHR)
                && !f.queue_flags.contains(vk::QueueFlags::GRAPHICS)
            {
                return i as u32;
            }
        }
        for (i, f) in families.iter().enumerate() {
            if f.queue_count > 0 && f.queue_flags.contains(vk::QueueFlags::VIDEO_ENCODE_KHR) {
                return i as u32;
            }
        }
        graphics
    }


    pub fn family(self, queue: QueueType) -> u32 {
        match queue {
            QueueType::Graphics => self.graphics,
            QueueType::Compute => self.compute,
            QueueType::Transfer => self.transfer,
            QueueType::AsyncCompute => self.async_compute,
            QueueType::Dma => self.dma,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct VulkanQueues {
    pub graphics: vk::Queue,
    pub compute: vk::Queue,
    pub transfer: vk::Queue,
    /// Track 11b: dedicated async compute queue handle. May equal `compute` when no dedicated family.
    pub async_compute: vk::Queue,
    /// Track 11b: dedicated DMA transfer queue handle. May equal `graphics` when no dedicated family.
    pub dma: vk::Queue,
}

impl VulkanQueues {
    pub fn queue(self, queue: QueueType) -> vk::Queue {
        match queue {
            QueueType::Graphics => self.graphics,
            QueueType::Compute => self.compute,
            QueueType::Transfer => self.transfer,
            QueueType::AsyncCompute => self.async_compute,
            QueueType::Dma => self.dma,
        }
    }
}

pub fn queue_family_index(
    families: QueueFamilyMap,
    before: QueueType,
    after: QueueType,
    current: QueueType,
) -> (u32, u32) {
    let before_family = families.family(before);
    let after_family = families.family(after);
    if before_family == after_family {
        return (vk::QUEUE_FAMILY_IGNORED, vk::QUEUE_FAMILY_IGNORED);
    }

    let current_family = families.family(current);
    if current_family == before_family {
        (before_family, after_family)
    } else if current_family == after_family {
        (before_family, after_family)
    } else {
        (vk::QUEUE_FAMILY_IGNORED, vk::QUEUE_FAMILY_IGNORED)
    }
}
