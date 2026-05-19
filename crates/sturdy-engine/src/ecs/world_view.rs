// WorldView — a non-exclusive borrow of `World` for parallel systems.
//
// `WorldView` is handed to every system in a parallel wave. It provides
// per-component-type read/write access that is enforced at schedule-build time
// rather than at runtime: the scheduler verifies disjoint write sets before
// the wave starts, so the unsafe raw-pointer access here is sound.
//
// ## Safety invariant
//
// A `WorldView` is ONLY created inside `CompiledSchedule::run`, which holds
// an exclusive `&mut World` borrow for the entire duration of the schedule.
// The scheduler guarantees that within any single wave:
//
//   1. The `HashMap<TypeId, Box<dyn ComponentVec>>` is not structurally
//      modified (no insertions or removals). Structural modifications are
//      deferred to `WorldCommands` and applied between waves.
//
//   2. At most ONE active `ComponentWriteGuard<C>` exists for any given `C`.
//      Two systems in the same wave cannot both declare `write_component::<C>`.
//
//   3. No `ComponentWriteGuard<C>` coexists with any `ComponentReadGuard<C>`
//      in the same wave. A writer and a reader of the same `C` are always
//      placed in different waves by the dependency graph builder.
//
// These invariants make the raw-pointer casts below sound.

use std::any::TypeId;
use std::marker::PhantomData;
use std::ptr::NonNull;

use super::Entity;
use super::storage::{Component, ComponentStorage, downcast_storage, downcast_storage_mut};
use super::world::World;

// ── ComponentReadGuard ────────────────────────────────────────────────────────

/// Shared access to a component storage obtained from `WorldView::read::<C>()`.
///
/// Derefs to `ComponentStorage<C>`, giving access to `get`, `iter_with_entities`,
/// `contains`, etc.
pub struct ComponentReadGuard<'a, C: Component> {
    storage: &'a ComponentStorage<C>,
}

impl<'a, C: Component> std::ops::Deref for ComponentReadGuard<'a, C> {
    type Target = ComponentStorage<C>;
    fn deref(&self) -> &ComponentStorage<C> {
        self.storage
    }
}

// ── ComponentWriteGuard ───────────────────────────────────────────────────────

/// Exclusive access to a component storage obtained from `WorldView::write::<C>()`.
///
/// Derefs to `ComponentStorage<C>` (and `DerefMut`), giving access to `get_mut`,
/// `insert`, `remove`, `iter_mut_with_entities`, etc.
pub struct ComponentWriteGuard<'a, C: Component> {
    storage: &'a mut ComponentStorage<C>,
}

impl<'a, C: Component> std::ops::Deref for ComponentWriteGuard<'a, C> {
    type Target = ComponentStorage<C>;
    fn deref(&self) -> &ComponentStorage<C> {
        self.storage
    }
}

impl<'a, C: Component> std::ops::DerefMut for ComponentWriteGuard<'a, C> {
    fn deref_mut(&mut self) -> &mut ComponentStorage<C> {
        self.storage
    }
}

// ── WorldView ─────────────────────────────────────────────────────────────────

/// A non-exclusive view of the `World` for use in parallel systems.
///
/// Handed to every system in a parallel wave by `CompiledSchedule`. Provides
/// per-component-type guards that enforce the scheduler's access declarations.
///
/// See the module-level safety note for the invariants that make this sound.
pub struct WorldView<'w> {
    world: NonNull<World>,
    _marker: PhantomData<&'w World>,
}

// SAFETY: WorldView is only created when the World is exclusively borrowed by
// CompiledSchedule::run. The scheduler ensures disjoint write access per wave,
// so sending WorldView to rayon worker threads is safe.
unsafe impl<'w> Send for WorldView<'w> {}
unsafe impl<'w> Sync for WorldView<'w> {}

impl<'w> WorldView<'w> {
    /// Create a WorldView from an exclusively borrowed World.
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// 1. All `WorldView` instances derived from this `world` in a single wave
    ///    have disjoint write sets (enforced by the scheduler before this call).
    /// 2. No structural mutation of `world.components` occurs while any
    ///    `WorldView` derived from this pointer is live.
    pub(super) unsafe fn new(world: &'w mut World) -> Self {
        WorldView {
            world: NonNull::from(world),
            _marker: PhantomData,
        }
    }

    /// Create a WorldView from a raw pointer.
    ///
    /// # Safety
    ///
    /// All requirements of `new` apply. Additionally, the pointer must be
    /// valid and non-null for the lifetime `'w`.
    pub(super) unsafe fn from_ptr(world: *mut World) -> Self {
        WorldView {
            world: unsafe { NonNull::new_unchecked(world) },
            _marker: PhantomData,
        }
    }

    // ── Component access ──────────────────────────────────────────────────────

    /// Acquire shared access to the storage for component type `C`.
    ///
    /// Multiple `ComponentReadGuard<C>` instances may coexist (readers don't
    /// block each other). A read guard and a write guard for the **same** `C`
    /// must never exist simultaneously — the scheduler prevents this at
    /// schedule-build time.
    ///
    /// # Panics
    ///
    /// Panics if `C` has no storage in the world (no entity was ever spawned
    /// with a `C` component). Register the storage first by inserting at least
    /// one entity with `C`, or call `world.init_component::<C>()` before
    /// building the schedule.
    pub fn read<C: Component>(&self) -> ComponentReadGuard<'_, C> {
        let world = unsafe { self.world.as_ref() };
        let storage = world
            .components
            .get(&TypeId::of::<C>())
            // SAFETY: Scheduler guarantees no concurrent writer for C in this wave.
            .and_then(|cell| downcast_storage::<C>(unsafe { &*cell.get() }.as_ref()))
            .unwrap_or_else(|| std::process::abort());
        ComponentReadGuard { storage }
    }

    /// Acquire exclusive access to the storage for component type `C`.
    ///
    /// # Safety invariant (enforced by the scheduler, not at runtime)
    ///
    /// At most one `ComponentWriteGuard<C>` may exist per wave. The scheduler
    /// guarantees this by placing any two systems that write `C` in different
    /// waves.
    ///
    /// # Panics
    ///
    /// Same as `read` — panics if `C` has no storage.
    pub fn write<C: Component>(&self) -> ComponentWriteGuard<'_, C> {
        let world = unsafe { self.world.as_ref() };
        let cell = match world.components.get(&TypeId::of::<C>()) {
            Some(cell) => cell,
            None => std::process::abort(),
        };
        // SAFETY: UnsafeCell::get() is the sanctioned way to get a *mut T from
        // a shared reference. The scheduler guarantees at most one active
        // ComponentWriteGuard<C> per wave (disjoint write sets), making the
        // exclusive borrow sound.
        let storage = unsafe {
            match downcast_storage_mut::<C>((&mut *cell.get()).as_mut()) {
                Some(storage) => storage,
                None => std::process::abort(),
            }
        };
        ComponentWriteGuard { storage }
    }

    // ── Parallel iteration convenience ────────────────────────────────────────

    /// Iterate all entities with component `C` in parallel using rayon.
    ///
    /// The closure receives `(Entity, &C)`. Use `write_par` for mutable access.
    /// Requires `C: Sync`.
    pub fn read_par<C: Component + Sync>(&self, f: impl Fn(Entity, &C) + Send + Sync) {
        use rayon::prelude::*;
        let world = unsafe { self.world.as_ref() };
        let guard = self.read::<C>();
        let gens = &world.entities.generations;
        guard
            .entity_of
            .par_iter()
            .zip(guard.dense.par_iter())
            .for_each(|(&idx, val)| {
                let generation = gens.get(idx as usize).copied().unwrap_or(0);
                f(
                    Entity {
                        index: idx,
                        generation,
                    },
                    val,
                );
            });
    }

    /// Iterate all entities with component `C` in parallel, with mutable access.
    ///
    /// The closure receives `(Entity, &mut C)`. Requires `C: Send`.
    pub fn write_par<C: Component + Send>(&self, f: impl Fn(Entity, &mut C) + Send + Sync) {
        use rayon::prelude::*;
        let world = unsafe { self.world.as_ref() };
        let mut guard = self.write::<C>();
        let gens = &world.entities.generations;
        // Split the borrow: entity_of (read) and dense (write) are disjoint fields.
        let (entity_of, dense) = guard.entity_and_dense_mut();
        entity_of
            .par_iter()
            .zip(dense.par_iter_mut())
            .for_each(|(&idx, val)| {
                let generation = gens.get(idx as usize).copied().unwrap_or(0);
                f(
                    Entity {
                        index: idx,
                        generation,
                    },
                    val,
                );
            });
    }

    // ── Resource access ───────────────────────────────────────────────────────

    /// Borrow a resource immutably. Returns `None` if not present.
    pub fn resource<R: 'static>(&self) -> Option<&R> {
        unsafe { self.world.as_ref() }.resource::<R>()
    }

    /// Borrow a resource when it is present.
    pub fn resource_unwrap<R: 'static>(&self) -> Option<&R> {
        unsafe { self.world.as_ref() }.resource_unwrap::<R>()
    }

    // ── Entity access ─────────────────────────────────────────────────────────

    /// Returns `true` if the entity is still alive.
    pub fn is_alive(&self, entity: Entity) -> bool {
        unsafe { self.world.as_ref() }.is_alive(entity)
    }

    /// Number of live entities.
    pub fn entity_count(&self) -> usize {
        unsafe { self.world.as_ref() }.entity_count()
    }

    /// Borrow the generation table.
    ///
    /// Pass to `ComponentStorage::iter_with_entities` / `iter_mut_with_entities`
    /// when you want to iterate (entity, component) pairs manually from a guard.
    pub fn generations(&self) -> &[u32] {
        &unsafe { self.world.as_ref() }.entities.generations
    }
}
