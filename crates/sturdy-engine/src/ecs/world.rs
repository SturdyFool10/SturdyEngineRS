// World — the root ECS container.
//
// Owns all entities and component storages. Provides the full API surface:
//   - spawn / despawn / is_alive
//   - insert / remove / get / get_mut / has  (single entity, single component)
//   - query / query_mut                      (one component type, all entities)
//   - query2 / query2_mut                    (two component types, filtered)
//   - query3                                 (three component types, filtered)
//   - entities_with                          (entity iterator for a component)
//   - sync_scene_transforms                  (flush Transform+SceneLink to Scene)

use std::any::TypeId;
use std::collections::HashMap;

use super::{
    Entity,
    entity::EntityAllocator,
    storage::{Component, ComponentStorage, ComponentVec, downcast_storage, downcast_storage_mut},
};

// ── World ─────────────────────────────────────────────────────────────────────

/// The central ECS container.
///
/// Owns all entities and their component data. Create once per game and pass
/// `&mut world` into your systems.
///
/// # Example
/// ```ignore
/// let mut world = World::new();
///
/// // Spawn an entity with components.
/// let player = world.spawn()
///     .with(Transform::from_position([0.0, 1.0, 0.0]))
///     .with(Velocity::default())
///     .with(Health::new(100.0))
///     .id();
///
/// // Query all entities with a Velocity component.
/// for (entity, vel) in world.query_mut::<Velocity>() {
///     vel.linear.y -= 9.81 * dt;
/// }
///
/// // Despawn.
/// world.despawn(player);
/// ```
pub struct World {
    pub(super) entities: EntityAllocator,
    /// Component storages keyed by TypeId of the component type.
    pub(super) components: HashMap<TypeId, Box<dyn ComponentVec>>,
}

impl World {
    /// Create an empty world.
    pub fn new() -> Self {
        Self {
            entities: EntityAllocator::new(),
            components: HashMap::new(),
        }
    }

    // ── Entity lifetime ───────────────────────────────────────────────────────

    /// Begin spawning a new entity. Call `.with(component)` to attach components
    /// and `.id()` to finish.
    pub fn spawn(&mut self) -> EntityBuilder<'_> {
        let entity = self.entities.alloc();
        EntityBuilder { world: self, entity }
    }

    /// Spawn an entity with no components and return its handle immediately.
    pub fn spawn_empty(&mut self) -> Entity {
        self.entities.alloc()
    }

    /// Remove an entity and all its components. Returns `false` if already dead.
    pub fn despawn(&mut self, entity: Entity) -> bool {
        if !self.entities.free(entity) {
            return false;
        }
        for storage in self.components.values_mut() {
            storage.remove_entity(entity.index);
        }
        true
    }

    /// Returns `true` when the entity handle refers to a live entity.
    pub fn is_alive(&self, entity: Entity) -> bool {
        self.entities.is_alive(entity)
    }

    /// Number of live entities in the world.
    pub fn entity_count(&self) -> usize {
        self.entities.live_count() as usize
    }

    // ── Per-entity component access ───────────────────────────────────────────

    /// Attach `component` to `entity`, replacing any existing value of the same type.
    ///
    /// Panics if `entity` is not alive.
    pub fn insert<T: Component>(&mut self, entity: Entity, component: T) {
        assert!(self.entities.is_alive(entity), "insert on dead entity {entity}");
        self.storage_mut::<T>().insert(entity.index, component);
    }

    /// Remove the component of type `T` from `entity`. Returns it if present.
    pub fn remove<T: Component>(&mut self, entity: Entity) -> Option<T> {
        let storage = self.components.get_mut(&TypeId::of::<T>())?;
        let storage = downcast_storage_mut::<T>(storage.as_mut())?;
        storage.remove(entity.index)
    }

    /// Return an immutable reference to `entity`'s component of type `T`.
    pub fn get<T: Component>(&self, entity: Entity) -> Option<&T> {
        let storage = downcast_storage::<T>(
            self.components.get(&TypeId::of::<T>())?.as_ref()
        )?;
        storage.get(entity.index)
    }

    /// Return a mutable reference to `entity`'s component of type `T`.
    pub fn get_mut<T: Component>(&mut self, entity: Entity) -> Option<&mut T> {
        let storage = downcast_storage_mut::<T>(
            self.components.get_mut(&TypeId::of::<T>())?.as_mut()
        )?;
        storage.get_mut(entity.index)
    }

    /// Returns `true` when `entity` has a component of type `T`.
    pub fn has<T: Component>(&self, entity: Entity) -> bool {
        self.components
            .get(&TypeId::of::<T>())
            .and_then(|s| downcast_storage::<T>(s.as_ref()))
            .map(|s| s.contains(entity.index))
            .unwrap_or(false)
    }

    // ── Single-component queries ──────────────────────────────────────────────

    /// Iterate all live entities that have a component of type `T`.
    ///
    /// Yields `(Entity, &T)` in dense-storage order (not spawn order).
    pub fn query<T: Component>(&self) -> Box<dyn Iterator<Item = (Entity, &T)> + '_> {
        let Some(storage) = self.components
            .get(&TypeId::of::<T>())
            .and_then(|s| downcast_storage::<T>(s.as_ref()))
        else {
            return Box::new(std::iter::empty());
        };
        let gens = &self.entities.generations;
        Box::new(storage.iter_with_entities(gens))
    }

    /// Iterate all live entities with mutable access to their `T` component.
    pub fn query_mut<T: Component>(&mut self) -> Box<dyn Iterator<Item = (Entity, &mut T)> + '_> {
        let gens: *const Vec<u32> = &self.entities.generations;
        let Some(storage) = self.components
            .get_mut(&TypeId::of::<T>())
            .and_then(|s| downcast_storage_mut::<T>(s.as_mut()))
        else {
            return Box::new(std::iter::empty())
                as Box<dyn Iterator<Item = (Entity, &mut T)>>;
        };
        // SAFETY: `gens` points into `self.entities.generations` which is never
        // mutated during iteration (we only mutate `storage`).
        let gens: &Vec<u32> = unsafe { &*gens };
        Box::new(storage.iter_mut_with_entities(gens))
    }

    /// Iterate entities that have component `T`, yielding their `Entity` handles.
    ///
    /// Useful for manual multi-component access via `world.get(entity)`.
    pub fn entities_with<T: Component>(&self) -> impl Iterator<Item = Entity> + '_ {
        let gens = &self.entities.generations;
        self.components
            .get(&TypeId::of::<T>())
            .and_then(|s| downcast_storage::<T>(s.as_ref()))
            .into_iter()
            .flat_map(move |s| {
                s.entity_of.iter().map(move |&idx| Entity {
                    index: idx,
                    generation: gens.get(idx as usize).copied().unwrap_or(0),
                })
            })
    }

    // ── Two-component queries ─────────────────────────────────────────────────

    /// Iterate entities that have **both** `A` and `B`, yielding immutable refs.
    ///
    /// Iterates the storage of `A` (the primary) and skips entities missing `B`.
    pub fn query2<A: Component, B: Component>(
        &self,
    ) -> impl Iterator<Item = (Entity, &A, &B)> {
        let tid_a = TypeId::of::<A>();
        let tid_b = TypeId::of::<B>();

        // Immutable borrows of both storages from the map.
        let sa = self.components.get(&tid_a)
            .and_then(|s| downcast_storage::<A>(s.as_ref()));
        let sb = self.components.get(&tid_b)
            .and_then(|s| downcast_storage::<B>(s.as_ref()));

        let (Some(sa), Some(sb)) = (sa, sb) else {
            return Box::new(std::iter::empty())
                as Box<dyn Iterator<Item = (Entity, &A, &B)>>;
        };

        let gens = &self.entities.generations;
        Box::new(
            sa.entity_of.iter().zip(sa.dense.iter()).filter_map(move |(&idx, a)| {
                let b = sb.get(idx)?;
                let generation = gens.get(idx as usize).copied().unwrap_or(0);
                Some((Entity { index: idx, generation }, a, b))
            }),
        )
    }

    /// Iterate entities with both `A` and `B`, yielding `&mut A` and `&B`.
    ///
    /// `A` and `B` must be different types; panics otherwise.
    pub fn query2_mut<A: Component, B: Component>(
        &mut self,
    ) -> impl Iterator<Item = (Entity, &mut A, &B)> {
        let tid_a = TypeId::of::<A>();
        let tid_b = TypeId::of::<B>();
        assert_ne!(tid_a, tid_b, "query2_mut: A and B must be distinct component types");

        // Obtain raw pointers to both storages from the HashMap.
        // SAFETY: `tid_a != tid_b` guarantees these are different HashMap values.
        // We access storage A mutably and storage B read-only — no aliasing.
        let sa_ptr: Option<*mut ComponentStorage<A>> = self.components
            .get_mut(&tid_a)
            .and_then(|s| downcast_storage_mut::<A>(s.as_mut()))
            .map(|s| s as *mut _);
        let sb_ptr: Option<*const ComponentStorage<B>> = self.components
            .get(&tid_b)
            .and_then(|s| downcast_storage::<B>(s.as_ref()))
            .map(|s| s as *const _);

        let (Some(sa_ptr), Some(sb_ptr)) = (sa_ptr, sb_ptr) else {
            return Box::new(std::iter::empty())
                as Box<dyn Iterator<Item = (Entity, &mut A, &B)>>;
        };

        let gens: *const Vec<u32> = &self.entities.generations;

        Box::new(
            // SAFETY: sa_ptr and sb_ptr point to distinct storage entries.
            // 'a outlives the returned iterator (tied to &mut self).
            unsafe {
                let sa: &mut ComponentStorage<A> = &mut *sa_ptr;
                sa.entity_of.iter().zip(sa.dense.iter_mut()).filter_map(move |(&idx, a)| {
                    let sb: &ComponentStorage<B> = &*sb_ptr;
                    let b = sb.get(idx)?;
                    let gens: &Vec<u32> = &*gens;
                    let generation = gens.get(idx as usize).copied().unwrap_or(0);
                    Some((Entity { index: idx, generation }, a, b))
                })
            },
        )
    }

    /// Iterate entities with `A`, `B`, and `C`, yielding immutable refs to all three.
    pub fn query3<A: Component, B: Component, C: Component>(
        &self,
    ) -> impl Iterator<Item = (Entity, &A, &B, &C)> {
        let tid_a = TypeId::of::<A>();
        let tid_b = TypeId::of::<B>();
        let tid_c = TypeId::of::<C>();

        let sa = self.components.get(&tid_a).and_then(|s| downcast_storage::<A>(s.as_ref()));
        let sb = self.components.get(&tid_b).and_then(|s| downcast_storage::<B>(s.as_ref()));
        let sc = self.components.get(&tid_c).and_then(|s| downcast_storage::<C>(s.as_ref()));

        let (Some(sa), Some(sb), Some(sc)) = (sa, sb, sc) else {
            return Box::new(std::iter::empty())
                as Box<dyn Iterator<Item = (Entity, &A, &B, &C)>>;
        };

        let gens = &self.entities.generations;
        Box::new(
            sa.entity_of.iter().zip(sa.dense.iter()).filter_map(move |(&idx, a)| {
                let b = sb.get(idx)?;
                let c = sc.get(idx)?;
                let generation = gens.get(idx as usize).copied().unwrap_or(0);
                Some((Entity { index: idx, generation }, a, b, c))
            }),
        )
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    /// Get or create the storage for component type `T`.
    pub(super) fn storage_mut<T: Component>(&mut self) -> &mut ComponentStorage<T> {
        self.components
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(ComponentStorage::<T>::new()))
            .as_any_mut()
            .downcast_mut::<ComponentStorage<T>>()
            .expect("storage type mismatch — should never happen")
    }

    // ── Scene integration ─────────────────────────────────────────────────────

    /// Flush all `(Transform, SceneLink)` pairs to the render `Scene`.
    ///
    /// Call once per frame, after systems have finished updating transforms and
    /// before `scene.prepare()` / `deferred.draw()`.
    ///
    /// ```ignore
    /// world.sync_scene_transforms(&mut scene);
    /// scene.prepare(&engine)?;
    /// deferred.draw(&mut scene, view, proj, &output, &frame, &engine, time)?;
    /// ```
    pub fn sync_scene_transforms(&self, scene: &mut crate::Scene) {
        for (_, transform, link) in self.query2::<super::components::Transform, super::components::SceneLink>() {
            scene.set_transform(link.object_id, transform.to_mat4());
        }
    }
}

impl Default for World {
    fn default() -> Self { Self::new() }
}

// ── EntityBuilder ─────────────────────────────────────────────────────────────

/// Fluent builder returned by `World::spawn()`.
///
/// ```ignore
/// let enemy = world.spawn()
///     .with(Transform::from_position([5.0, 0.0, 0.0]))
///     .with(Health::new(50.0))
///     .with(EnemyAI::default())
///     .id();
/// ```
pub struct EntityBuilder<'w> {
    pub(super) world: &'w mut World,
    pub(super) entity: Entity,
}

impl<'w> EntityBuilder<'w> {
    /// Attach a component to the entity being built.
    pub fn with<T: Component>(self, component: T) -> Self {
        self.world.storage_mut::<T>().insert(self.entity.index, component);
        self
    }

    /// Finish building and return the `Entity` handle.
    pub fn id(self) -> Entity {
        self.entity
    }
}

