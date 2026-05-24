use std::collections::HashSet;
use std::sync::{
    Mutex,
    atomic::{AtomicU32, Ordering},
};

use super::GpuObjectId;

/// Thread-safe allocator for stable GPU object slots.
///
/// Fresh IDs are issued from an atomic counter. Released IDs are kept in a
/// mutex-protected `HashSet` so that reserve/release are both O(1) and
/// duplicate releases are caught in O(1) as well.
#[derive(Debug)]
pub struct GpuObjectAllocator {
    next: AtomicU32,
    free: Mutex<HashSet<u32>>,
}

impl GpuObjectAllocator {
    pub fn new() -> Self {
        Self {
            next: AtomicU32::new(0),
            free: Mutex::new(HashSet::new()),
        }
    }

    /// Reserve one object slot from any thread.
    pub fn reserve(&self) -> GpuObjectId {
        let mut free = self
            .free
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(&raw) = free.iter().next() {
            free.remove(&raw);
            return GpuObjectId::from_raw(raw);
        }
        drop(free);

        let raw = self.next.fetch_add(1, Ordering::Relaxed);
        assert_ne!(
            raw,
            GpuObjectId::INVALID.as_u32(),
            "GpuObjectAllocator exhausted all valid object IDs"
        );
        GpuObjectId::from_raw(raw)
    }

    /// Return a slot to the allocator.
    ///
    /// `GpuObjectId::INVALID` is ignored. Double-releasing the same valid ID is
    /// a programming error and will panic in debug builds.
    pub fn release(&self, id: GpuObjectId) {
        if !id.is_valid() {
            return;
        }
        let mut free = self
            .free
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let inserted = free.insert(id.as_u32());
        debug_assert!(
            inserted,
            "GpuObjectAllocator::release called twice for {:?}",
            id
        );
    }

    /// Approximate number of slots currently checked out.
    pub fn allocated_count(&self) -> usize {
        let next = self.next.load(Ordering::Relaxed) as usize;
        let free = self.free.lock().map(|free| free.len()).unwrap_or(0);
        next.saturating_sub(free)
    }
}

impl Default for GpuObjectAllocator {
    fn default() -> Self {
        Self::new()
    }
}
