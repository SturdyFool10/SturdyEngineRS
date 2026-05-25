// Tests extracted from crates/sturdy-engine/src/ecs/mod.rs
// See scripts/extract_tests.py for the extraction logic.

use super::*;

#[derive(Debug, Clone, PartialEq)]
struct Pos(f32, f32);
#[derive(Debug, Clone, PartialEq)]
struct Spd(f32);
#[derive(Debug, Clone, PartialEq)]
struct FrameCounter(u32);

fn assert_vec3_near(actual: glam::Vec3, expected: glam::Vec3) {
    assert!(
        actual.abs_diff_eq(expected, 1e-5),
        "expected {expected:?}, got {actual:?}"
    );
}

#[test]
fn transform_look_at_aligns_forward_and_world_up() {
    let mut transform = Transform::from_position(glam::Vec3::ZERO);
    transform.look_at(glam::Vec3::new(0.0, 0.0, -1.0), glam::Vec3::Y);

    assert_vec3_near(transform.forward(), glam::Vec3::NEG_Z);
    assert_vec3_near(transform.up(), glam::Vec3::Y);
    assert_vec3_near(transform.right(), glam::Vec3::X);
}

#[test]
fn transform_look_at_honors_roll_from_up_vector() {
    let mut transform = Transform::from_position(glam::Vec3::ZERO);
    transform.look_at(glam::Vec3::new(0.0, 0.0, -1.0), glam::Vec3::X);

    assert_vec3_near(transform.forward(), glam::Vec3::NEG_Z);
    assert_vec3_near(transform.up(), glam::Vec3::X);
    assert_vec3_near(transform.right(), glam::Vec3::NEG_Y);
}

#[test]
fn transform_look_at_handles_parallel_up_vector() {
    let mut transform = Transform::from_position(glam::Vec3::ZERO);
    transform.look_at(glam::Vec3::Y, glam::Vec3::Y);

    assert_vec3_near(transform.forward(), glam::Vec3::Y);
    assert!(transform.up().is_finite());
    assert!(transform.right().is_finite());
    assert!(transform.up().dot(transform.forward()).abs() < 1e-5);
    assert!(transform.right().dot(transform.forward()).abs() < 1e-5);
}

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
    assert_eq!(e1.index, e2.index); // same slot
    assert_ne!(e1.generation, e2.generation); // different generation
    assert!(!world.is_alive(e1)); // stale handle is dead
    assert!(world.is_alive(e2)); // new handle is live
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
    world.spawn().with(Spd(5.0)).id(); // no Pos

    let pairs: Vec<(f32, f32)> = world
        .query2::<Pos, Spd>()
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
    sched.add_system("first", move |_w| {
        o1.lock().unwrap().push(1);
    });
    sched.add_system("second", move |_w| {
        o2.lock().unwrap().push(2);
    });
    sched.run(&mut world);

    assert_eq!(*order.lock().unwrap(), vec![1, 2]);
}

#[test]
fn resource_insert_and_get() {
    let mut world = World::new();
    world.insert_resource(42u32);
    assert_eq!(world.resource::<u32>(), Some(&42));
    assert_eq!(world.resource::<i32>(), None); // different type
}

#[test]
fn resource_mut_modifies_in_place() {
    let mut world = World::new();
    world.insert_resource(0u32);
    *world.resource_mut::<u32>().unwrap() += 1;
    *world.resource_mut::<u32>().unwrap() += 1;
    assert_eq!(world.resource::<u32>(), Some(&2));
}

#[test]
fn resource_insert_replaces_previous() {
    let mut world = World::new();
    world.insert_resource(1u32);
    world.insert_resource(99u32);
    assert_eq!(world.resource::<u32>(), Some(&99));
}

#[test]
fn resource_remove_returns_value() {
    let mut world = World::new();
    world.insert_resource(7u32);
    let val = world.remove_resource::<u32>();
    assert_eq!(val, Some(7));
    assert!(!world.has_resource::<u32>());
    // Removing again returns None.
    assert_eq!(world.remove_resource::<u32>(), None);
}

#[test]
fn resource_has_resource() {
    let mut world = World::new();
    assert!(!world.has_resource::<String>());
    world.insert_resource(String::from("hello"));
    assert!(world.has_resource::<String>());
}

// ── Parallel schedule tests ───────────────────────────────────────────────

#[test]
fn compiled_schedule_serial_system_runs() {
    let mut world = World::new();
    let counter = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let c = counter.clone();
    let mut sched = Schedule::new();
    sched.add_system("bump", move |_w| {
        c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    });
    let mut compiled = sched.build();
    compiled.run(&mut world);
    assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 1);
}

/// Reads `Pos` without modifying anything — used to test read access behaviour.
struct ReadPos;
impl ParallelSystem for ReadPos {
    fn access() -> SystemAccess {
        SystemAccess::new().read_component::<Pos>()
    }
    fn run(&mut self, world: &WorldView<'_>, _commands: &mut WorldCommands) {
        let _guard = world.read::<Pos>();
    }
}

/// Adds `delta` to the `x` field of every `Pos` component.
struct Adder(f32);
impl ParallelSystem for Adder {
    fn access() -> SystemAccess {
        SystemAccess::new().write_component::<Pos>()
    }
    fn run(&mut self, world: &WorldView<'_>, _commands: &mut WorldCommands) {
        let delta = self.0;
        world.write_par::<Pos>(|_, pos| pos.0 += delta);
    }
}

#[test]
fn compiled_schedule_parallel_system_mutates_components() {
    let mut world = World::new();
    let e = world.spawn().with(Pos(1.0, 0.0)).id();

    let mut sched = Schedule::new();
    sched.add_parallel_system("add_one", Adder(1.0));
    let mut compiled = sched.build();
    compiled.run(&mut world);

    assert_eq!(world.get::<Pos>(e), Some(&Pos(2.0, 0.0)));
}

/// Despawns any entity whose `Spd` is zero or negative.
struct Reaper;
impl ParallelSystem for Reaper {
    fn access() -> SystemAccess {
        SystemAccess::new().read_component::<Spd>()
    }
    fn run(&mut self, world: &WorldView<'_>, commands: &mut WorldCommands) {
        let speeds = world.read::<Spd>();
        let gens = world.generations();
        for (entity, spd) in speeds.iter_with_entities(gens) {
            if spd.0 <= 0.0 {
                commands.despawn(entity);
            }
        }
    }
}

#[test]
fn world_commands_despawn_applied_after_wave() {
    let mut world = World::new();
    let alive = world.spawn().with(Spd(5.0)).id();
    let dead = world.spawn().with(Spd(0.0)).id();

    let mut sched = Schedule::new();
    sched.add_parallel_system("reap", Reaper);
    let mut compiled = sched.build();
    compiled.run(&mut world);

    assert!(world.is_alive(alive));
    assert!(!world.is_alive(dead));
}

#[test]
fn two_non_conflicting_parallel_systems_form_one_wave() {
    // Adder writes Pos, Reaper reads Spd — no conflict → same wave.
    let mut sched = Schedule::new();
    sched.add_parallel_system("adder", Adder(1.0));
    sched.add_parallel_system("reaper", Reaper);
    let compiled = sched.build();
    assert_eq!(compiled.wave_count(), 1);
    assert_eq!(compiled.system_count(), 2);
}

#[test]
fn two_conflicting_parallel_systems_form_two_waves() {
    // Both write Pos — conflict → separate waves.
    let mut sched = Schedule::new();
    sched.add_parallel_system("add_one", Adder(1.0));
    sched.add_parallel_system("add_two", Adder(2.0));
    let compiled = sched.build();
    assert_eq!(compiled.wave_count(), 2);
}

struct CountFrames;
impl ParallelSystem for CountFrames {
    fn access() -> SystemAccess {
        SystemAccess::new().write_resource::<FrameCounter>()
    }

    fn run(&mut self, world: &WorldView<'_>, _commands: &mut WorldCommands) {
        world.resource_mut::<FrameCounter>().unwrap().0 += 1;
    }
}

struct ReadFrameCounter;
impl ParallelSystem for ReadFrameCounter {
    fn access() -> SystemAccess {
        SystemAccess::new().read_resource::<FrameCounter>()
    }

    fn run(&mut self, world: &WorldView<'_>, _commands: &mut WorldCommands) {
        assert!(world.resource::<FrameCounter>().is_some());
    }
}

#[test]
fn resource_write_parallel_system_mutates_world_resource() {
    let mut world = World::new();
    world.insert_resource(FrameCounter(0));

    let mut sched = Schedule::new();
    sched.add_parallel_system("count_frames", CountFrames);
    let mut compiled = sched.build();
    compiled.run(&mut world);

    assert_eq!(world.resource::<FrameCounter>(), Some(&FrameCounter(1)));
}

#[test]
fn resource_read_write_conflict_splits_parallel_waves() {
    let mut sched = Schedule::new();
    sched.add_parallel_system("read_counter", ReadFrameCounter);
    sched.add_parallel_system("write_counter", CountFrames);
    let compiled = sched.build();

    assert_eq!(compiled.wave_count(), 2);
    assert_eq!(
        compiled.wave_names(),
        vec![vec!["read_counter"], vec!["write_counter"]]
    );
}

#[test]
fn serial_system_between_parallel_splits_into_three_waves() {
    let mut sched = Schedule::new();
    sched.add_parallel_system("a", Adder(1.0));
    sched.add_system("serial", |_w| {});
    sched.add_parallel_system("b", Reaper);
    let compiled = sched.build();
    // wave 0: [a], wave 1: [serial], wave 2: [b]
    assert_eq!(compiled.wave_count(), 3);
}

#[test]
fn schedule_run_downgrades_parallel_to_serial() {
    // schedule.run() executes parallel systems without rayon — no panic.
    let mut world = World::new();
    let e = world.spawn().with(Pos(0.0, 0.0)).id();
    let mut sched = Schedule::new();
    sched.add_parallel_system("add_one", Adder(1.0));
    sched.run(&mut world);
    assert_eq!(world.get::<Pos>(e), Some(&Pos(1.0, 0.0)));
}

#[test]
fn resource_returns_none_when_missing() {
    let world = World::new();
    assert!(world.resource::<u32>().is_none());
}

#[test]
fn world_view_read_panics_when_storage_missing() {
    let mut world = World::new();
    // No entities with Pos and no init_component call.
    let mut sched = Schedule::new();
    sched.add_parallel_system("read_pos", ReadPos);
    let mut compiled = sched.build();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        compiled.run(&mut world);
    }));
    assert!(
        result.is_err(),
        "expected WorldView::read to panic when storage is missing"
    );
    if let Some(msg) = result.unwrap_err().downcast_ref::<String>() {
        assert!(
            msg.contains("Pos"),
            "panic message should name the component type, got: {msg}"
        );
    }
}

#[test]
fn init_component_prevents_read_panic() {
    let mut world = World::new();
    world.init_component::<Pos>();
    // Storage pre-registered — no entities needed.
    let mut sched = Schedule::new();
    sched.add_parallel_system("read_pos", ReadPos);
    let mut compiled = sched.build();
    // Must not panic.
    compiled.run(&mut world);
}

#[test]
fn resources_are_independent_of_components() {
    let mut world = World::new();
    // Insert a resource with a type that is also a component.
    world.insert_resource(Pos(1.0, 2.0));
    let e = world.spawn().with(Pos(9.0, 9.0)).id();
    // Resource and component are separate.
    assert_eq!(world.resource::<Pos>(), Some(&Pos(1.0, 2.0)));
    assert_eq!(world.get::<Pos>(e), Some(&Pos(9.0, 9.0)));
}

#[test]
fn builtin_parallel_integrate_transforms_matches_serial_system() {
    let mut serial_world = World::new();
    let serial_entity = serial_world
        .spawn()
        .with(Transform::from_position(glam::Vec3::ZERO))
        .with(Velocity {
            linear: glam::Vec3::new(2.0, 0.0, 0.0),
            angular: glam::Vec3::Y,
        })
        .id();
    integrate_transforms(&mut serial_world, 0.5);

    let mut parallel_world = World::new();
    let parallel_entity = parallel_world
        .spawn()
        .with(Transform::from_position(glam::Vec3::ZERO))
        .with(Velocity {
            linear: glam::Vec3::new(2.0, 0.0, 0.0),
            angular: glam::Vec3::Y,
        })
        .id();
    let mut schedule = Schedule::new();
    schedule.add_parallel_system("integrate", IntegrateTransformsSystem::new(0.5));
    let mut compiled = schedule.build();
    compiled.run(&mut parallel_world);

    let serial = serial_world.get::<Transform>(serial_entity).unwrap();
    let parallel = parallel_world.get::<Transform>(parallel_entity).unwrap();
    assert_vec3_near(parallel.position, serial.position);
    assert!(parallel.rotation.abs_diff_eq(serial.rotation, 1e-5));
}

#[test]
fn builtin_parallel_acceleration_pipeline_updates_velocity_and_clears_accumulator() {
    let mut world = World::new();
    let entity = world
        .spawn()
        .with(Velocity::linear(glam::Vec3::ZERO))
        .with(Acceleration {
            linear: glam::Vec3::new(0.0, 4.0, 0.0),
        })
        .id();

    let mut schedule = Schedule::new();
    schedule
        .add_parallel_system("apply_acceleration", ApplyAccelerationSystem::new(0.25))
        .add_parallel_system("clear_acceleration", ClearAccelerationSystem);
    let mut compiled = schedule.build();
    assert_eq!(compiled.wave_count(), 2);
    compiled.run(&mut world);

    assert_vec3_near(
        world.get::<Velocity>(entity).unwrap().linear,
        glam::Vec3::new(0.0, 1.0, 0.0),
    );
    assert_vec3_near(
        world.get::<Acceleration>(entity).unwrap().linear,
        glam::Vec3::ZERO,
    );
}

#[test]
fn builtin_parallel_despawn_dead_uses_world_commands() {
    let mut world = World::new();
    let alive = world.spawn().with(Health::new(10.0)).id();
    let dead = world
        .spawn()
        .with(Health {
            current: 0.0,
            max: 10.0,
        })
        .id();

    let mut schedule = Schedule::new();
    schedule.add_parallel_system("despawn_dead", DespawnDeadSystem);
    let mut compiled = schedule.build();
    compiled.run(&mut world);

    assert!(world.is_alive(alive));
    assert!(!world.is_alive(dead));
}
