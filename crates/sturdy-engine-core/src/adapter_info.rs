use crate::{AdapterKind, BackendKind};

#[derive(Clone, Debug, Default)]
pub struct AdapterInfo {
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub kind: AdapterKind,
    pub backend: BackendKind,
    pub driver_version: u32,
    /// Human-readable driver version string (e.g. "560.35.03").
    pub driver_name: Option<String>,
    pub graphics_queue_count: u32,
    pub compute_queue_count: u32,
    pub transfer_queue_count: u32,
    /// Total dedicated video memory in bytes; 0 when unknown.
    pub vram_bytes: u64,
    /// Whether this adapter is a software / CPU emulation device.
    pub is_software: bool,
    /// API version the adapter supports, encoded as `(major << 22) | (minor << 12) | patch`.
    pub api_version: u32,
}

impl AdapterInfo {
    /// Returns the major.minor.patch components of `api_version`.
    pub fn api_version_triple(&self) -> (u32, u32, u32) {
        let major = self.api_version >> 22;
        let minor = (self.api_version >> 12) & 0x3ff;
        let patch = self.api_version & 0xfff;
        (major, minor, patch)
    }
}
