/// GPU memory usage snapshot reported by the sub-allocator.
///
/// Obtain each frame via [`Device::memory_budget`]. Use `over_budget` to detect
/// VRAM pressure before the GPU driver starts evicting resources.
///
/// # Note
/// Reported sizes reflect engine-side sub-allocator blocks, not raw OS memory.
/// On unified-memory GPUs (Apple Silicon, integrated AMD) device-local and
/// host-visible may be the same physical pool; both counters are still populated.
#[derive(Clone, Debug, Default)]
pub struct GpuMemoryBudget {
    /// Bytes currently sub-allocated from device-local (VRAM) blocks.
    pub device_local_used_bytes: u64,
    /// Total device-local VkDeviceMemory committed by the sub-allocator.
    pub device_local_capacity_bytes: u64,
    /// Bytes currently sub-allocated from host-visible (upload/staging) blocks.
    pub host_visible_used_bytes: u64,
    /// Total host-visible VkDeviceMemory committed.
    pub host_visible_capacity_bytes: u64,
    /// Number of live `VkDeviceMemory` blocks. Should stay small (< 20).
    pub block_count: u32,
}

impl GpuMemoryBudget {
    /// Device-local utilisation in [0, 1]. > 0.8 means the engine is likely
    /// approaching VRAM limits and streaming eviction should kick in.
    pub fn device_local_fraction(&self) -> f32 {
        if self.device_local_capacity_bytes == 0 {
            return 0.0;
        }
        self.device_local_used_bytes as f32 / self.device_local_capacity_bytes as f32
    }

    /// Returns `true` when device-local usage exceeds 80 % of committed capacity.
    pub fn over_budget(&self) -> bool {
        self.device_local_fraction() > 0.80
    }

    /// Device-local used in MiB.
    pub fn device_local_used_mib(&self) -> f32 {
        self.device_local_used_bytes as f32 / (1024.0 * 1024.0)
    }

    /// Device-local capacity in MiB.
    pub fn device_local_capacity_mib(&self) -> f32 {
        self.device_local_capacity_bytes as f32 / (1024.0 * 1024.0)
    }

    /// One-line human-readable summary, e.g. `"VRAM 423 / 512 MiB (82 %) [over budget]"`.
    pub fn summary(&self) -> String {
        let used = self.device_local_used_mib();
        let cap = self.device_local_capacity_mib();
        let pct = (self.device_local_fraction() * 100.0) as u32;
        let flag = if self.over_budget() {
            " [over budget]"
        } else {
            ""
        };
        format!("VRAM {used:.0} / {cap:.0} MiB ({pct} %){flag}")
    }
}

/// Per-heap memory budget and usage reported by `VK_EXT_memory_budget`.
///
/// More precise than `GpuMemoryBudget` — the driver reports actual OS-level budget and
/// usage per heap, including allocations made by other processes sharing the same GPU.
#[derive(Clone, Debug, Default)]
pub struct MemoryHeapBudget {
    /// Driver-reported budget for this heap in bytes. Staying below this avoids eviction.
    pub budget: u64,
    /// Driver-reported current usage for this heap in bytes (includes all processes).
    pub usage: u64,
}

/// Full memory budget report from `VK_EXT_memory_budget`, one entry per Vulkan memory heap.
///
/// Obtain via [`Device::memory_budget_ext`]. Returns `None` when the extension is unavailable.
#[derive(Clone, Debug, Default)]
pub struct MemoryBudgetReport {
    /// Per-heap entries; index matches `VkPhysicalDeviceMemoryProperties::memoryHeaps`.
    pub heaps: Vec<MemoryHeapBudget>,
}

impl MemoryBudgetReport {
    /// Returns `true` when any device-local heap has usage > 80 % of its budget.
    pub fn any_over_budget(&self) -> bool {
        self.heaps
            .iter()
            .any(|h| h.budget > 0 && h.usage > h.budget * 4 / 5)
    }
}
