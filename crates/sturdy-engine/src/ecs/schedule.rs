// Schedule — ordered list of systems that run against a World.
//
// A `SystemFn` is any `FnMut(&mut World)`. Systems are added by name for
// diagnostics and run in insertion order. The schedule can be split into
// multiple schedules for different update stages (fixed, per-frame, render-prep).
//
// # Example
// ```ignore
// let mut physics = Schedule::new();
// physics
//     .add_system("apply_gravity",   apply_gravity)
//     .add_system("integrate_pos",   integrate_positions)
//     .add_system("resolve_collisions", resolve_collisions);
//
// // In fixed_update:
// physics.run(&mut world);
// ```

use super::World;

// ── SystemFn ──────────────────────────────────────────────────────────────────

/// A function that reads/writes component data in a `World`.
///
/// Any `FnMut(&mut World) + Send + Sync + 'static` qualifies.
pub type SystemFn = Box<dyn FnMut(&mut World) + Send + Sync + 'static>;

// ── System trait (optional — use when you need state or config) ───────────────

/// Object-safe alternative to `SystemFn` for systems that need associated state.
///
/// Implement this trait and register via `Schedule::add_system_obj` when your
/// system needs to carry configuration or per-run state that doesn't belong in
/// a component.
pub trait System: Send + Sync + 'static {
    fn run(&mut self, world: &mut World);
}

// Blanket impl for plain function pointers.
impl<F: FnMut(&mut World) + Send + Sync + 'static> System for F {
    fn run(&mut self, world: &mut World) { (self)(world); }
}

// ── Schedule ──────────────────────────────────────────────────────────────────

/// An ordered collection of named systems that execute against a `World`.
///
/// Use multiple `Schedule` instances for different stages of your game loop:
///
/// ```ignore
/// struct MyGame {
///     world: World,
///     fixed_schedule: Schedule,   // physics, AI
///     render_schedule: Schedule,  // animation, transform sync
/// }
///
/// impl GameApp for MyGame {
///     fn fixed_update(&mut self, ctx: &FixedUpdateContext<'_>) -> Result<(), Self::Error> {
///         self.fixed_schedule.run(&mut self.world);
///         Ok(())
///     }
///
///     fn render(&mut self, ...) -> Result<(), Self::Error> {
///         self.render_schedule.run(&mut self.world);
///         self.world.sync_scene_transforms(&mut self.scene);
///         // ... draw calls ...
///         Ok(())
///     }
/// }
/// ```
pub struct Schedule {
    systems: Vec<(String, Box<dyn System>)>,
    /// Whether to print system names + timing to stderr each run (debug builds only).
    pub debug_timing: bool,
}

impl Schedule {
    pub fn new() -> Self {
        Self { systems: Vec::new(), debug_timing: false }
    }

    /// Append a named system.
    ///
    /// Systems run in insertion order. The name is used in diagnostic output.
    pub fn add_system(
        &mut self,
        name: impl Into<String>,
        system: impl FnMut(&mut World) + Send + Sync + 'static,
    ) -> &mut Self {
        self.systems.push((name.into(), Box::new(system)));
        self
    }

    /// Append a system that implements the `System` trait (useful for stateful systems).
    pub fn add_system_obj(
        &mut self,
        name: impl Into<String>,
        system: impl System,
    ) -> &mut Self {
        self.systems.push((name.into(), Box::new(system)));
        self
    }

    /// Run all systems in order against `world`.
    pub fn run(&mut self, world: &mut World) {
        if self.debug_timing && cfg!(debug_assertions) {
            for (name, system) in &mut self.systems {
                let t0 = std::time::Instant::now();
                system.run(world);
                eprintln!("[Schedule] {name}: {:.3}ms", t0.elapsed().as_secs_f64() * 1000.0);
            }
        } else {
            for (_, system) in &mut self.systems {
                system.run(world);
            }
        }
    }

    /// Number of systems in this schedule.
    pub fn system_count(&self) -> usize { self.systems.len() }

    /// Names of registered systems in run order.
    pub fn system_names(&self) -> impl Iterator<Item = &str> {
        self.systems.iter().map(|(name, _)| name.as_str())
    }

    /// Remove all systems.
    pub fn clear(&mut self) { self.systems.clear(); }
}

impl Default for Schedule { fn default() -> Self { Self::new() } }

// ── Convenience: one-shot system execution ────────────────────────────────────

/// Run a single system function directly against a world, without a schedule.
///
/// Useful for infrequent operations (level load, save, etc.) that don't belong
/// in a recurring schedule.
pub fn run_once(world: &mut World, mut system: impl FnMut(&mut World)) {
    system(world);
}
