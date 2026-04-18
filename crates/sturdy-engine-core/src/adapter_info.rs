use crate::{AdapterKind, BackendKind};

#[derive(Clone, Debug)]
pub struct AdapterInfo {
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub kind: AdapterKind,
    pub backend: BackendKind,
    pub driver_version: u32,
    pub graphics_queue_count: u32,
    pub compute_queue_count: u32,
    pub transfer_queue_count: u32,
}
