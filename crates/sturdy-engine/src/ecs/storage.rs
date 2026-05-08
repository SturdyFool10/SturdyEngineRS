// Component storage — sparse set per component type.
//
// Each component type T gets one `ComponentStorage<T>`:
//   sparse[entity.index]  → Option<dense_index>
//   dense[dense_index]    → T
//   entity_of[dense_index] → entity index (for iteration)
//
// Insert: O(1)   Remove: O(1) (swap-remove from dense array)
// Get:    O(1)   Iterate: O(n) over only entities that have the component

use std::any::Any;

use super::Entity;

// ── Component trait ───────────────────────────────────────────────────────────

/// Marker trait for data that can be attached to an `Entity`.
///
/// Automatically implemented for any `'static + Send + Sync` type.
///
/// # Example
/// ```ignore
/// #[derive(Debug, Clone)]
/// struct Health { pub current: f32, pub max: f32 }
/// impl Component for Health {}
/// ```
pub trait Component: Any + Send + Sync + 'static {}

// ── Type-erased storage trait ─────────────────────────────────────────────────

/// Type-erased component storage. Stored as `Box<dyn ComponentVec>` in `World`.
pub(super) trait ComponentVec: Any + Send + Sync {
    /// Remove the component for the given entity index, if present.
    fn remove_entity(&mut self, entity_index: u32);
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// ── ComponentStorage<T> ───────────────────────────────────────────────────────

/// Sparse-set storage for component type `T`.
pub(super) struct ComponentStorage<T: Component> {
    /// sparse[entity_index] = dense_index (u32::MAX = absent).
    sparse: Vec<u32>,
    /// Packed component values (dense layout, good cache performance).
    pub(super) dense: Vec<T>,
    /// dense[i] = entity_index of the component at dense[i].
    pub(super) entity_of: Vec<u32>,
}

impl<T: Component> ComponentStorage<T> {
    pub fn new() -> Self {
        Self {
            sparse: Vec::new(),
            dense: Vec::new(),
            entity_of: Vec::new(),
        }
    }

    /// Insert or replace the component for `entity`.
    pub fn insert(&mut self, entity_index: u32, value: T) {
        // Grow sparse array if necessary.
        if entity_index as usize >= self.sparse.len() {
            self.sparse.resize(entity_index as usize + 1, u32::MAX);
        }
        let slot = self.sparse[entity_index as usize];
        if slot != u32::MAX {
            // Replace existing.
            self.dense[slot as usize] = value;
        } else {
            // Append to dense.
            let dense_idx = self.dense.len() as u32;
            self.sparse[entity_index as usize] = dense_idx;
            self.dense.push(value);
            self.entity_of.push(entity_index);
        }
    }

    /// Remove the component for `entity`. Returns it if present.
    pub fn remove(&mut self, entity_index: u32) -> Option<T> {
        let slot = *self.sparse.get(entity_index as usize)?;
        if slot == u32::MAX {
            return None;
        }
        let slot = slot as usize;
        let last = self.dense.len() - 1;
        let last_entity = self.entity_of[last];
        // Swap-remove: move target to the end, then pop.
        self.dense.swap(slot, last);
        self.entity_of.swap(slot, last);
        // If we moved another entity into `slot`, update its sparse entry.
        if slot < last {
            self.sparse[last_entity as usize] = slot as u32;
        }
        self.sparse[entity_index as usize] = u32::MAX;
        self.entity_of.pop();
        self.dense.pop()
    }

    pub fn get(&self, entity_index: u32) -> Option<&T> {
        let slot = *self.sparse.get(entity_index as usize)?;
        if slot == u32::MAX { None } else { Some(&self.dense[slot as usize]) }
    }

    pub fn get_mut(&mut self, entity_index: u32) -> Option<&mut T> {
        let slot = *self.sparse.get(entity_index as usize)?;
        if slot == u32::MAX { None } else { Some(&mut self.dense[slot as usize]) }
    }

    pub fn contains(&self, entity_index: u32) -> bool {
        self.sparse.get(entity_index as usize).map(|&s| s != u32::MAX).unwrap_or(false)
    }

    /// Iterate all (Entity, &T) pairs. Caller supplies generation table for Entity construction.
    pub fn iter_with_entities<'a>(
        &'a self,
        generations: &'a [u32],
    ) -> impl Iterator<Item = (Entity, &'a T)> + 'a {
        self.entity_of.iter().zip(self.dense.iter()).map(|(&idx, val)| {
            let generation = generations.get(idx as usize).copied().unwrap_or(0);
            (Entity { index: idx, generation }, val)
        })
    }

    /// Iterate all (Entity, &mut T) pairs.
    pub fn iter_mut_with_entities<'a>(
        &'a mut self,
        generations: &'a [u32],
    ) -> impl Iterator<Item = (Entity, &'a mut T)> + 'a {
        self.entity_of.iter().zip(self.dense.iter_mut()).map(|(&idx, val)| {
            let generation = generations.get(idx as usize).copied().unwrap_or(0);
            (Entity { index: idx, generation }, val)
        })
    }
}

impl<T: Component> ComponentVec for ComponentStorage<T> {
    fn remove_entity(&mut self, entity_index: u32) {
        // Delegate to `remove` — same swap-remove logic, return value discarded.
        self.remove(entity_index);
    }

    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

// ── Casting helpers ───────────────────────────────────────────────────────────

pub(super) fn downcast_storage<T: Component>(
    boxed: &dyn ComponentVec,
) -> Option<&ComponentStorage<T>> {
    boxed.as_any().downcast_ref::<ComponentStorage<T>>()
}

pub(super) fn downcast_storage_mut<T: Component>(
    boxed: &mut dyn ComponentVec,
) -> Option<&mut ComponentStorage<T>> {
    boxed.as_any_mut().downcast_mut::<ComponentStorage<T>>()
}
