#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct WindowId(u64);

impl WindowId {
    pub fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct WindowHandle {
    id: WindowId,
    generation: u32,
}

impl WindowHandle {
    pub fn id(self) -> WindowId {
        self.id
    }

    pub fn generation(self) -> u32 {
        self.generation
    }
}

struct WindowSlot<T> {
    generation: u32,
    value: Option<T>,
}

pub struct WindowRegistry<T> {
    slots: Vec<WindowSlot<T>>,
    free: Vec<usize>,
}

impl<T> Default for WindowRegistry<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
        }
    }
}

impl<T> WindowRegistry<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, value: T) -> WindowHandle {
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index];
            slot.value = Some(value);
            return WindowHandle {
                id: WindowId(index as u64),
                generation: slot.generation,
            };
        }

        let index = self.slots.len();
        self.slots.push(WindowSlot {
            generation: 0,
            value: Some(value),
        });
        WindowHandle {
            id: WindowId(index as u64),
            generation: 0,
        }
    }

    pub fn get(&self, handle: WindowHandle) -> Option<&T> {
        let slot = self.slots.get(handle.id.0 as usize)?;
        if slot.generation == handle.generation {
            slot.value.as_ref()
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, handle: WindowHandle) -> Option<&mut T> {
        let slot = self.slots.get_mut(handle.id.0 as usize)?;
        if slot.generation == handle.generation {
            slot.value.as_mut()
        } else {
            None
        }
    }

    pub fn remove(&mut self, handle: WindowHandle) -> Option<T> {
        let index = handle.id.0 as usize;
        let slot = self.slots.get_mut(index)?;
        if slot.generation != handle.generation {
            return None;
        }
        let value = slot.value.take()?;
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(index);
        Some(value)
    }

    pub fn contains(&self, handle: WindowHandle) -> bool {
        self.get(handle).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = (WindowHandle, &T)> {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            slot.value.as_ref().map(|value| {
                (
                    WindowHandle {
                        id: WindowId(index as u64),
                        generation: slot.generation,
                    },
                    value,
                )
            })
        })
    }

    pub fn live_count(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.value.is_some())
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_returns_inserted_window_by_handle() {
        let mut registry = WindowRegistry::new();
        let handle = registry.insert("primary");

        assert_eq!(registry.get(handle), Some(&"primary"));
        assert!(registry.contains(handle));
        assert_eq!(registry.live_count(), 1);
    }

    #[test]
    fn removed_handle_does_not_resolve_after_slot_reuse() {
        let mut registry = WindowRegistry::new();
        let old = registry.insert("old");
        assert_eq!(registry.remove(old), Some("old"));

        let new = registry.insert("new");

        assert_eq!(old.id(), new.id());
        assert_ne!(old.generation(), new.generation());
        assert_eq!(registry.get(old), None);
        assert_eq!(registry.get(new), Some(&"new"));
    }

    #[test]
    fn stale_remove_does_not_remove_new_window() {
        let mut registry = WindowRegistry::new();
        let old = registry.insert(1);
        assert_eq!(registry.remove(old), Some(1));
        let new = registry.insert(2);

        assert_eq!(registry.remove(old), None);
        assert_eq!(registry.get(new), Some(&2));
    }

    #[test]
    fn iter_returns_live_handles() {
        let mut registry = WindowRegistry::new();
        let removed = registry.insert("removed");
        let live = registry.insert("live");
        assert_eq!(registry.remove(removed), Some("removed"));

        let entries = registry.iter().collect::<Vec<_>>();

        assert_eq!(entries, vec![(live, &"live")]);
    }
}
