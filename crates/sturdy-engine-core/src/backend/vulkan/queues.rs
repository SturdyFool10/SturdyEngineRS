use ash::vk;

use crate::QueueType;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct QueueFamilyMap {
    pub graphics: u32,
    pub compute: u32,
    pub transfer: u32,
}

impl QueueFamilyMap {
    pub fn unified(graphics: u32) -> Self {
        Self {
            graphics,
            compute: graphics,
            transfer: graphics,
        }
    }

    pub fn family(self, queue: QueueType) -> u32 {
        match queue {
            QueueType::Graphics => self.graphics,
            QueueType::Compute => self.compute,
            QueueType::Transfer => self.transfer,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct VulkanQueues {
    pub graphics: vk::Queue,
    pub compute: vk::Queue,
    pub transfer: vk::Queue,
}

impl VulkanQueues {
    pub fn queue(self, queue: QueueType) -> vk::Queue {
        match queue {
            QueueType::Graphics => self.graphics,
            QueueType::Compute => self.compute,
            QueueType::Transfer => self.transfer,
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
