// Entity — generational index.
//
// Each `Entity` is a (index, generation) pair. Despawning increments the
// generation so stale handles become invalid without a separate "tombstone"
// scan. Up to u32::MAX live entities; u32::MAX generations per slot.

use std::fmt;

/// A lightweight, copyable handle to a world entity.
///
/// Two `Entity` values are equal only when both the index and the generation
/// match, so a handle to a despawned entity is never equal to a new entity
/// that reuses the same slot.
#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub struct Entity {
    /// Index into the entity allocator's generation table.
    pub(super) index: u32,
    /// Generation counter — incremented each time the slot is recycled.
    pub(super) generation: u32,
}

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Entity({}v{})", self.index, self.generation)
    }
}

impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}v{}", self.index, self.generation)
    }
}

// ── EntityAllocator ───────────────────────────────────────────────────────────

/// Manages entity slot allocation and generation tracking.
pub(super) struct EntityAllocator {
    /// Current generation per slot. Index = entity index.
    pub(super) generations: Vec<u32>,
    /// Slots available for reuse (freed by despawn).
    free: Vec<u32>,
    /// Total live entity count.
    live: u32,
}

impl EntityAllocator {
    pub fn new() -> Self {
        Self {
            generations: Vec::new(),
            free: Vec::new(),
            live: 0,
        }
    }

    /// Allocate a new entity slot.
    pub fn alloc(&mut self) -> Entity {
        self.live += 1;
        if let Some(index) = self.free.pop() {
            Entity { index, generation: self.generations[index as usize] }
        } else {
            let index = self.generations.len() as u32;
            self.generations.push(0);
            Entity { index, generation: 0 }
        }
    }

    /// Free an entity slot. Returns `false` if the entity was already dead.
    pub fn free(&mut self, entity: Entity) -> bool {
        let slot = self.generations.get_mut(entity.index as usize);
        match slot {
            Some(slot_gen) if *slot_gen == entity.generation => {
                *slot_gen = slot_gen.wrapping_add(1);
                self.free.push(entity.index);
                self.live -= 1;
                true
            }
            _ => false,
        }
    }

    /// Returns `true` when the entity handle refers to a live slot.
    pub fn is_alive(&self, entity: Entity) -> bool {
        self.generations
            .get(entity.index as usize)
            .map(|&g| g == entity.generation)
            .unwrap_or(false)
    }

    /// Number of live entities.
    pub fn live_count(&self) -> u32 {
        self.live
    }

    /// Total allocated slots (live + recycled).
    pub fn slot_count(&self) -> usize {
        self.generations.len()
    }
}
