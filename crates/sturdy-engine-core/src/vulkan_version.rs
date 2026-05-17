/// A Vulkan API version used to control the minimum required and maximum
/// requested Vulkan version when creating a Vulkan backend device.
///
/// # Examples
/// ```
/// use sturdy_engine_core::{DeviceDesc, VulkanApiVersion};
///
/// // Require at least Vulkan 1.3, request up to 1.4.
/// let desc = DeviceDesc {
///     min_vulkan_version: VulkanApiVersion::V1_3,
///     max_vulkan_version: VulkanApiVersion::LATEST,
///     ..DeviceDesc::default()
/// };
/// ```
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct VulkanApiVersion {
    pub major: u32,
    pub minor: u32,
}

impl VulkanApiVersion {
    pub const V1_0: Self = Self { major: 1, minor: 0 };
    pub const V1_1: Self = Self { major: 1, minor: 1 };
    /// Vulkan 1.2 — timeline semaphores, buffer device address, descriptor indexing.
    pub const V1_2: Self = Self { major: 1, minor: 2 };
    /// Vulkan 1.3 — dynamic rendering, synchronization2, extended dynamic state.
    pub const V1_3: Self = Self { major: 1, minor: 3 };
    /// Vulkan 1.4 — push descriptors, maintenance5/6, dynamic rendering local read.
    pub const V1_4: Self = Self { major: 1, minor: 4 };

    /// The latest Vulkan version the engine is aware of.
    pub const LATEST: Self = Self::V1_4;

    /// Convert to the packed `u32` used in Vulkan API calls.
    pub fn to_vk(self) -> u32 {
        ash::vk::make_api_version(0, self.major, self.minor, 0)
    }

    /// Parse from the packed `u32` returned by Vulkan (ignores the patch component).
    pub fn from_vk(version: u32) -> Self {
        Self {
            major: ash::vk::api_version_major(version),
            minor: ash::vk::api_version_minor(version),
        }
    }
}

impl std::fmt::Display for VulkanApiVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}
