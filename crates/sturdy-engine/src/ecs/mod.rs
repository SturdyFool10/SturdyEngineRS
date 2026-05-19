// Entity Component System.
//
// A minimal, cache-friendly ECS built specifically for Sturdy Engine:
//
//   World     — owns all entities and component data
//   Entity    — generational handle (index + generation, 8 bytes, Copy)
//   Component — any 'static + Send + Sync type (blanket impl, no derive needed)
//   Schedule  — ordered list of systems; run with schedule.run(&mut world)
//
// Built-in components:
//   Transform      — position / rotation / scale → Mat4 for the render scene
//   LocalTransform — transform relative to a parent entity
//   Velocity       — linear + angular velocity for Newtonian integration
//   Acceleration   — force accumulator, cleared each step
//   SceneLink      — links an entity's Transform to a render-scene ObjectId
//   Name           — debug label
//   Active         — participation flag (checked by convention, not enforced)
//   Health         — current/max HP with damage/heal helpers
//
// Built-in systems (plain functions, register in your Schedule):
//   integrate_transforms(world, dt)   — apply Velocity to Transform
//   propagate_local_transforms(world) — world-space propagation of LocalTransform
//   despawn_dead(world)               — remove entities whose Health == 0
//
// # Minimal example
// ```ignore
// use sturdy_engine::{World, Schedule, Transform, Velocity, SceneLink, integrate_transforms};
//
// struct Game {
//     world:          World,
//     fixed_schedule: Schedule,
//     scene:          Scene,
//     // ...
// }
//
// impl GameApp for Game {
//     fn init(engine: &Engine, _surface: &Surface) -> Result<Self, Self::Error> {
//         let mut world          = World::new();
//         let mut fixed_schedule = Schedule::new();
//         let mut scene          = Scene::new();
//
//         // Register systems.
//         let fixed_step = Duration::from_secs_f32(1.0 / 60.0);
//         fixed_schedule.add_system("integrate", move |w| {
//             integrate_transforms(w, fixed_step.as_secs_f32());
//         });
//
//         // Spawn a moving cube.
//         let mesh_id   = scene.add_mesh(Mesh::cube(engine, 1.0)?, program);
//         let object_id = scene.add_object(mesh_id, ObjectKind::Dynamic);
//         world.spawn()
//             .with(Transform::from_position([0.0, 1.0, 0.0]))
//             .with(Velocity::linear(Vec3::new(1.0, 0.0, 0.0)))
//             .with(SceneLink { object_id })
//             .id();
//
//         Ok(Self { world, fixed_schedule, scene, /* ... */ })
//     }
//
//     fn fixed_update(&mut self, _ctx: &FixedUpdateContext<'_>) -> Result<(), Self::Error> {
//         self.fixed_schedule.run(&mut self.world);
//         Ok(())
//     }
//
//     fn render(&mut self, frame: &mut ShellFrame, ...) -> Result<(), Self::Error> {
//         // Sync ECS transforms → render scene, then draw.
//         self.world.sync_scene_transforms(&mut self.scene);
//         // ...
//         Ok(())
//     }
// }
// ```

mod compiled_schedule;
pub mod components;
mod entity;
mod parallel_system;
mod schedule;
mod storage;
mod world;
mod world_commands;
mod world_view;

pub use compiled_schedule::CompiledSchedule;
pub use components::{
    Acceleration,
    // Core transform + physics
    Active,
    Health,
    LocalTransform,
    Name,
    SceneLink,
    Transform,
    Velocity,
    // Built-in systems
    despawn_dead,
    integrate_transforms,
    propagate_local_transforms,
};
pub use entity::Entity;
pub use parallel_system::{ParallelSystem, SystemAccess};
pub use schedule::{Schedule, System, SystemFn, run_once};
pub use storage::Component;
pub use world::{EntityBuilder, World};
pub use world_commands::WorldCommands;
pub use world_view::{ComponentReadGuard, ComponentWriteGuard, WorldView};

#[cfg(test)]
mod tests;
