// ParallelSystem — access-declaring systems for the parallel schedule.
//
// A `ParallelSystem` declares exactly which component types and resources it
// reads and writes. The scheduler uses these declarations to build a dependency
// graph, group non-conflicting systems into parallel waves, and validate that
// no two systems in the same wave write the same data.
//
// ## Implementing a parallel system
//
// ```ignore
// struct VelocityIntegrator { dt: f32 }
//
// impl ParallelSystem for VelocityIntegrator {
//     fn access() -> SystemAccess {
//         SystemAccess::new()
//             .write_component::<Transform>()
//             .read_component::<Velocity>()
//     }
//
//     fn run(&mut self, world: &WorldView<'_>, _commands: &mut WorldCommands) {
//         let transforms = world.write::<Transform>();
//         let velocities  = world.read::<Velocity>();
//         for (entity, vel) in velocities.iter() {
//             if let Some(tf) = transforms.get_mut(entity.index) {
//                 tf.position += vel.linear * self.dt;
//             }
//         }
//     }
// }
//
// // Register with Schedule:
// schedule.add_parallel_system("integrate_velocity", VelocityIntegrator { dt: 1.0 / 60.0 });
// ```

use std::any::TypeId;
use std::collections::HashSet;

use super::world_view::WorldView;
use super::world_commands::WorldCommands;

// ── SystemAccess ──────────────────────────────────────────────────────────────

/// Declares which component types and resources a parallel system accesses.
///
/// Used by the scheduler to build the dependency graph and validate that no two
/// systems in the same wave have conflicting access patterns. Two systems
/// conflict when one writes what the other reads or writes.
///
/// # Example
/// ```ignore
/// SystemAccess::new()
///     .write_component::<Transform>()
///     .read_component::<Velocity>()
///     .read_resource::<DeltaTime>()
/// ```
#[derive(Clone, Debug, Default)]
pub struct SystemAccess {
    pub component_reads: HashSet<TypeId>,
    pub component_writes: HashSet<TypeId>,
    pub resource_reads: HashSet<TypeId>,
    pub resource_writes: HashSet<TypeId>,
}

impl SystemAccess {
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare a shared read on component type `C`.
    pub fn read_component<C: 'static>(mut self) -> Self {
        self.component_reads.insert(TypeId::of::<C>());
        self
    }

    /// Declare an exclusive write on component type `C`.
    pub fn write_component<C: 'static>(mut self) -> Self {
        self.component_writes.insert(TypeId::of::<C>());
        self
    }

    /// Declare a shared read on resource type `R`.
    pub fn read_resource<R: 'static>(mut self) -> Self {
        self.resource_reads.insert(TypeId::of::<R>());
        self
    }

    /// Declare an exclusive write on resource type `R`.
    pub fn write_resource<R: 'static>(mut self) -> Self {
        self.resource_writes.insert(TypeId::of::<R>());
        self
    }

    /// Returns `true` when the two access declarations conflict.
    ///
    /// Two systems conflict when:
    /// - Both write the same component or resource (write-after-write), or
    /// - One writes what the other reads (read-after-write / write-after-read).
    pub fn conflicts_with(&self, other: &SystemAccess) -> bool {
        // Component conflicts
        !self.component_writes.is_disjoint(&other.component_writes)
            || !self.component_writes.is_disjoint(&other.component_reads)
            || !self.component_reads.is_disjoint(&other.component_writes)
            // Resource conflicts
            || !self.resource_writes.is_disjoint(&other.resource_writes)
            || !self.resource_writes.is_disjoint(&other.resource_reads)
            || !self.resource_reads.is_disjoint(&other.resource_writes)
    }


}

// ── ParallelSystem trait ──────────────────────────────────────────────────────

/// A system that declares its data access and can run in parallel with other
/// non-conflicting systems.
///
/// Implement this when you want the scheduler to automatically detect safe
/// parallelism. The `access()` associated function is called once at
/// `Schedule::build()` time — not per frame. Keep it cheap (construct the
/// sets, return them).
///
/// Existing `FnMut(&mut World)` systems continue to work unchanged as
/// "serial" systems that serialize everything around them.
pub trait ParallelSystem: Send + Sync + 'static {
    /// Declare the component types and resources this system accesses.
    fn access() -> SystemAccess
    where
        Self: Sized;

    /// Run the system against the world.
    ///
    /// `commands` queues deferred structural mutations (spawn, despawn, insert,
    /// remove) that are applied to the `World` between schedule waves.
    fn run(&mut self, world: &WorldView<'_>, commands: &mut WorldCommands);
}

// ── ErasedParallelSystem — object-safe internal wrapper ───────────────────────

/// Object-safe internal wrapper around `ParallelSystem`. Not part of the public
/// API — only the scheduler uses this.
pub(super) trait ErasedParallelSystem: Send + Sync + 'static {
    fn access(&self) -> &SystemAccess;
    fn run(&mut self, world: &WorldView<'_>, commands: &mut WorldCommands);
}

pub(super) struct ParallelSystemAdapter<S: ParallelSystem> {
    system: S,
    access: SystemAccess,
}

impl<S: ParallelSystem> ParallelSystemAdapter<S> {
    pub fn new(system: S) -> Self {
        let access = S::access();
        Self { system, access }
    }
}

impl<S: ParallelSystem> ErasedParallelSystem for ParallelSystemAdapter<S> {
    fn access(&self) -> &SystemAccess {
        &self.access
    }

    fn run(&mut self, world: &WorldView<'_>, commands: &mut WorldCommands) {
        self.system.run(world, commands);
    }
}
