use sturdy_engine_core::{
    AdapterInfo, AdapterKind, AdapterSelection, BackendKind, DeviceDesc,
    auto_backend_preference_order, available_backend_kinds, enumerate_adapters,
};

use crate::{Engine, Result};

/// A snapshot of one enumerated adapter.
#[derive(Clone, Debug)]
pub struct AdapterEntry {
    pub info: AdapterInfo,
    pub backend: BackendKind,
    /// Index of this adapter within its backend's enumeration.
    pub index: usize,
}

/// Enumerates and selects GPU adapters, and creates [`Engine`] instances from
/// the chosen adapter.
///
/// Use `DeviceManager::enumerate()` to list available adapters, then
/// `DeviceManager::create_engine_for` to open one.
pub struct DeviceManager {
    adapters: Vec<AdapterEntry>,
}

impl DeviceManager {
    /// Enumerate all adapters across all available backends.
    pub fn enumerate() -> Self {
        let mut adapters = Vec::new();
        for backend in available_backend_kinds() {
            if backend == BackendKind::Auto || backend == BackendKind::Null {
                continue;
            }
            if let Ok(infos) = enumerate_adapters(backend) {
                for (index, info) in infos.into_iter().enumerate() {
                    adapters.push(AdapterEntry {
                        info,
                        backend,
                        index,
                    });
                }
            }
        }
        Self { adapters }
    }

    /// Returns all enumerated adapters.
    pub fn adapters(&self) -> &[AdapterEntry] {
        &self.adapters
    }

    /// Returns the best adapter according to the platform preference order.
    ///
    /// Within a backend, discrete GPUs are always preferred over integrated
    /// GPUs regardless of reported VRAM (iGPUs typically report all system RAM
    /// as their heap, inflating the number).
    pub fn preferred(&self) -> Option<&AdapterEntry> {
        let preference = auto_backend_preference_order();
        for backend in &preference {
            // 1. Discrete GPU with the most dedicated VRAM.
            if let Some(entry) = self
                .adapters
                .iter()
                .filter(|e| {
                    &e.backend == backend
                        && !e.info.is_software
                        && e.info.kind == AdapterKind::DiscreteGpu
                })
                .max_by_key(|e| e.info.vram_bytes)
            {
                return Some(entry);
            }
            // 2. Any non-software adapter for this backend.
            if let Some(entry) = self
                .adapters
                .iter()
                .filter(|e| &e.backend == backend && !e.info.is_software)
                .max_by_key(|e| e.info.vram_bytes)
            {
                return Some(entry);
            }
        }
        // 3. Fallback: any non-software adapter, then any adapter.
        self.adapters
            .iter()
            .find(|e| !e.info.is_software)
            .or_else(|| self.adapters.first())
    }

    /// Create an `Engine` backed by the given adapter entry.
    pub fn create_engine_for(&self, entry: &AdapterEntry) -> Result<Engine> {
        Engine::with_desc(DeviceDesc {
            backend: entry.backend,
            validation: cfg!(debug_assertions),
            adapter: AdapterSelection::ByIndex(entry.index),
            ..DeviceDesc::default()
        })
    }

    /// Create an `Engine` backed by the preferred adapter, or the null backend
    /// if no adapters are available.
    pub fn create_preferred_engine(&self) -> Result<Engine> {
        match self.preferred() {
            Some(entry) => self.create_engine_for(entry),
            None => Engine::with_backend(BackendKind::Null),
        }
    }
}
