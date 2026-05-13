// WorldCommands — deferred structural mutations for parallel systems.
//
// Parallel systems cannot mutate the World's entity/component structure
// directly (spawn, despawn, insert, remove) because those operations require
// an exclusive `&mut World`. Instead, they post commands to a `WorldCommands`
// buffer. After each wave, `CompiledSchedule` collects all commands and applies
// them to the World on the main thread before the next wave begins.
//
// Each system in a parallel wave gets its OWN `WorldCommands` instance — no
// shared state, no locking. Commands are plain closures boxed into a Vec.
// Merging is O(n) over total commands across all systems.
//
// ## Example
//
// ```ignore
// fn run(&mut self, world: &WorldView<'_>, commands: &mut WorldCommands) {
//     for (entity, health) in world.read::<Health>().iter() {
//         if health.current <= 0.0 {
//             commands.despawn(entity);
//         }
//     }
//     commands.spawn(|b| { b.with(Transform::default()).with(Health::new(100.0)); });
// }
// ```

use super::storage::Component;
use super::{Entity, EntityBuilder, World};

// ── Command type ──────────────────────────────────────────────────────────────

type CommandFn = Box<dyn FnOnce(&mut World) + Send + 'static>;

// ── WorldCommands ─────────────────────────────────────────────────────────────

/// A per-system buffer of deferred structural mutations.
///
/// Received as `&mut WorldCommands` in `ParallelSystem::run`. Commands are
/// applied to the World between waves, not during the wave.
///
/// All posted commands are applied in FIFO order within each system's buffer,
/// and system buffers are applied in the order systems appear in the wave.
pub struct WorldCommands {
    queue: Vec<CommandFn>,
}

impl WorldCommands {
    pub(super) fn new() -> Self {
        Self { queue: Vec::new() }
    }

    /// Push a raw command closure. Lower-level than the typed helpers below.
    pub fn push(&mut self, cmd: impl FnOnce(&mut World) + Send + 'static) {
        self.queue.push(Box::new(cmd));
    }

    // ── Structural mutations ──────────────────────────────────────────────────

    /// Despawn `entity` and remove all its components.
    ///
    /// If the entity is already dead when the command is applied, it is a no-op.
    pub fn despawn(&mut self, entity: Entity) {
        self.push(move |world| {
            world.despawn(entity);
        });
    }

    /// Spawn a new entity. The closure receives an `EntityBuilder` — attach
    /// components with `.with(component)`. The entity exists in the world only
    /// after the current wave ends and commands are flushed.
    ///
    /// ```ignore
    /// commands.spawn(|b| { b.with(Transform::default()).with(Velocity::default()); });
    /// ```
    pub fn spawn(&mut self, build: impl FnOnce(EntityBuilder<'_>) + Send + 'static) {
        self.push(move |world| {
            build(world.spawn());
        });
    }

    /// Insert (or replace) a component on an existing entity.
    pub fn insert<C: Component>(&mut self, entity: Entity, component: C) {
        self.push(move |world| {
            world.insert(entity, component);
        });
    }

    /// Remove a component from an entity. No-op if the component is absent.
    pub fn remove<C: Component>(&mut self, entity: Entity) {
        self.push(move |world| {
            world.remove::<C>(entity);
        });
    }

    // ── Internal ──────────────────────────────────────────────────────────────

    /// Apply all queued commands to `world` in order, then clear the queue.
    pub(super) fn apply(&mut self, world: &mut World) {
        for cmd in self.queue.drain(..) {
            cmd(world);
        }
    }

    /// Number of pending commands.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}
