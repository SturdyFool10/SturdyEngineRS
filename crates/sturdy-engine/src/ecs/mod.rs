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

mod entity;
mod storage;
mod world;
mod schedule;
pub mod components;

pub use entity::Entity;
pub use storage::Component;
pub use world::{EntityBuilder, World};
pub use schedule::{Schedule, System, SystemFn, run_once};
pub use components::{
    // Core transform + physics
    Active, Acceleration, Health, LocalTransform, Name, SceneLink, Transform, Velocity,
    // Built-in systems
    despawn_dead, integrate_transforms, propagate_local_transforms,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct Pos(f32, f32);
    #[derive(Debug, Clone, PartialEq)]
    struct Spd(f32);

    #[test]
    fn spawn_and_despawn() {
        let mut world = World::new();
        let e = world.spawn().with(Pos(1.0, 2.0)).id();
        assert!(world.is_alive(e));
        assert_eq!(world.entity_count(), 1);
        assert!(world.despawn(e));
        assert!(!world.is_alive(e));
        assert_eq!(world.entity_count(), 0);
        // Double-despawn returns false.
        assert!(!world.despawn(e));
    }

    #[test]
    fn generational_index_prevents_stale_access() {
        let mut world = World::new();
        let e1 = world.spawn().with(Pos(0.0, 0.0)).id();
        world.despawn(e1);
        // Spawn a new entity that reuses the same slot.
        let e2 = world.spawn().with(Pos(9.9, 9.9)).id();
        assert_eq!(e1.index, e2.index);  // same slot
        assert_ne!(e1.generation, e2.generation); // different generation
        assert!(!world.is_alive(e1)); // stale handle is dead
        assert!(world.is_alive(e2));  // new handle is live
    }

    #[test]
    fn insert_remove_get() {
        let mut world = World::new();
        let e = world.spawn_empty();
        world.insert(e, Pos(3.0, 4.0));
        assert_eq!(world.get::<Pos>(e), Some(&Pos(3.0, 4.0)));
        let removed = world.remove::<Pos>(e);
        assert_eq!(removed, Some(Pos(3.0, 4.0)));
        assert_eq!(world.get::<Pos>(e), None);
    }

    #[test]
    fn query_single() {
        let mut world = World::new();
        world.spawn().with(Pos(1.0, 0.0)).id();
        world.spawn().with(Pos(2.0, 0.0)).with(Spd(5.0)).id();
        world.spawn().with(Spd(3.0)).id();

        let positions: Vec<f32> = world.query::<Pos>().map(|(_, p)| p.0).collect();
        assert_eq!(positions.len(), 2);
        assert!(positions.contains(&1.0));
        assert!(positions.contains(&2.0));
    }

    #[test]
    fn query_mut_modifies_in_place() {
        let mut world = World::new();
        let e = world.spawn().with(Pos(0.0, 0.0)).id();
        for (_, p) in world.query_mut::<Pos>() {
            p.0 += 1.0;
        }
        assert_eq!(world.get::<Pos>(e), Some(&Pos(1.0, 0.0)));
    }

    #[test]
    fn query2_filters_correctly() {
        let mut world = World::new();
        world.spawn().with(Pos(1.0, 0.0)).with(Spd(10.0)).id();
        world.spawn().with(Pos(2.0, 0.0)).id(); // no Spd
        world.spawn().with(Spd(5.0)).id();       // no Pos

        let pairs: Vec<(f32, f32)> = world.query2::<Pos, Spd>()
            .map(|(_, p, s)| (p.0, s.0))
            .collect();
        assert_eq!(pairs, vec![(1.0, 10.0)]);
    }

    #[test]
    fn despawn_removes_components() {
        let mut world = World::new();
        let e = world.spawn().with(Pos(1.0, 2.0)).with(Spd(5.0)).id();
        world.despawn(e);
        assert_eq!(world.query::<Pos>().count(), 0);
        assert_eq!(world.query::<Spd>().count(), 0);
    }

    #[test]
    fn schedule_runs_systems_in_order() {
        let mut world = World::new();
        world.spawn().with(Pos(0.0, 0.0)).id();

        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u32>::new()));
        let o1 = order.clone();
        let o2 = order.clone();

        let mut sched = Schedule::new();
        sched.add_system("first",  move |_w| { o1.lock().unwrap().push(1); });
        sched.add_system("second", move |_w| { o2.lock().unwrap().push(2); });
        sched.run(&mut world);

        assert_eq!(*order.lock().unwrap(), vec![1, 2]);
    }
}
