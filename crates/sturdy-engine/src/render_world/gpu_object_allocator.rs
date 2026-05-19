use std::sync::{
    Mutex,
    atomic::{AtomicU32, Ordering},
};

use super::GpuObjectId;

/// Thread-safe allocator for stable GPU object slots.
///
/// Fresh IDs are issued from an atomic counter. Released IDs are kept in a
/// small mutex-protected free list so IDs can be reused after extraction has
/// observed the release.
#[derive(Debug)]
pub struct GpuObjectAllocator {
    next: AtomicU32,
    free: Mutex<Vec<GpuObjectId>>,
}

impl GpuObjectAllocator {
    pub fn new() -> Self {
        Self {
            next: AtomicU32::new(0),
            free: Mutex::new(Vec::new()),
        }
    }

    /// Reserve one object slot from any thread.
    pub fn reserve(&self) -> GpuObjectId {
        if let Some(id) = self
            .free
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop()
        {
            return id;
        }

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
    /// Duplicate releases are ignored to keep the free list from issuing the
    /// same slot twice. `GpuObjectId::INVALID` is ignored.
    pub fn release(&self, id: GpuObjectId) {
        if !id.is_valid() {
            return;
        }
        let mut free = self
            .free
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !free.contains(&id) {
            free.push(id);
        }
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
