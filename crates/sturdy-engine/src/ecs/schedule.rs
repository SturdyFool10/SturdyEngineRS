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
use super::parallel_system::{ErasedParallelSystem, ParallelSystem, ParallelSystemAdapter};
use super::compiled_schedule::{CompiledSchedule, CompiledScheduleBuilder};

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
/// Use multiple `Schedule` instances for different stages of your game loop.
/// Call `run` for simple serial execution, or `build` to compile a parallel
/// schedule that uses rayon to run non-conflicting systems concurrently.
///
/// ```ignore
/// // Serial (existing API — unchanged):
/// let mut sched = Schedule::new();
/// sched.add_system("integrate", integrate_transforms);
/// sched.run(&mut world);
///
/// // Parallel (new API):
/// let mut sched = Schedule::new();
/// sched.add_parallel_system("integrate",  VelocityIntegrator::default());
/// sched.add_parallel_system("health_regen", HealthRegen::default());
/// let mut compiled = sched.build();
/// compiled.run(&mut world);
/// ```
pub struct Schedule {
    entries: Vec<ScheduleEntry>,
    /// Whether to print system names + timing to stderr each run (debug builds only).
    pub debug_timing: bool,
}

enum ScheduleEntry {
    Serial(String, Box<dyn System>),
    Parallel(String, Box<dyn ErasedParallelSystem>),
}

impl Schedule {
    pub fn new() -> Self {
        Self { entries: Vec::new(), debug_timing: false }
    }

    /// Append a serial system (existing API — unchanged).
    ///
    /// Serial systems run with exclusive `&mut World` access and act as a full
    /// barrier in the compiled schedule: no parallel system from an adjacent
    /// wave will overlap with them.
    pub fn add_system(
        &mut self,
        name: impl Into<String>,
        system: impl FnMut(&mut World) + Send + Sync + 'static,
    ) -> &mut Self {
        self.entries.push(ScheduleEntry::Serial(name.into(), Box::new(system)));
        self
    }

    /// Append a stateful serial system.
    pub fn add_system_obj(
        &mut self,
        name: impl Into<String>,
        system: impl System,
    ) -> &mut Self {
        self.entries.push(ScheduleEntry::Serial(name.into(), Box::new(system)));
        self
    }

    /// Append a parallel system.
    ///
    /// The system's `access()` declaration is read once here and used by
    /// `build()` to determine wave membership. Systems with non-conflicting
    /// access run concurrently in the same wave.
    pub fn add_parallel_system(
        &mut self,
        name: impl Into<String>,
        system: impl ParallelSystem,
    ) -> &mut Self {
        let adapter = ParallelSystemAdapter::new(system);
        self.entries.push(ScheduleEntry::Parallel(name.into(), Box::new(adapter)));
        self
    }

    /// Run all systems serially in insertion order.
    ///
    /// Parallel systems registered via `add_parallel_system` are downgraded to
    /// serial execution here — they run with an exclusive world view.
    /// Use `build().run()` to get actual parallel execution.
    pub fn run(&mut self, world: &mut World) {
        if self.debug_timing && cfg!(debug_assertions) {
            for entry in &mut self.entries {
                let t0 = std::time::Instant::now();
                match entry {
                    ScheduleEntry::Serial(name, sys) => {
                        sys.run(world);
                        eprintln!("[Schedule] {name}: {:.3}ms", t0.elapsed().as_secs_f64() * 1000.0);
                    }
                    ScheduleEntry::Parallel(name, sys) => {
                        // Downgrade: run parallel system serially via a temporary WorldView.
                        let mut cmds = super::world_commands::WorldCommands::new();
                        {
                            // SAFETY: We hold &mut World, so no other borrow exists.
                            let view = unsafe { super::world_view::WorldView::new(world) };
                            sys.run(&view, &mut cmds);
                        }
                        cmds.apply(world);
                        eprintln!("[Schedule] {name} (parallel→serial): {:.3}ms", t0.elapsed().as_secs_f64() * 1000.0);
                    }
                }
            }
        } else {
            for entry in &mut self.entries {
                match entry {
                    ScheduleEntry::Serial(_, sys) => {
                        sys.run(world);
                    }
                    ScheduleEntry::Parallel(_, sys) => {
                        let mut cmds = super::world_commands::WorldCommands::new();
                        {
                            let view = unsafe { super::world_view::WorldView::new(world) };
                            sys.run(&view, &mut cmds);
                        }
                        cmds.apply(world);
                    }
                }
            }
        }
    }

    /// Compile the schedule into a `CompiledSchedule` that runs parallel
    /// systems concurrently using rayon.
    ///
    /// Call this once at startup (or whenever the system list changes) and keep
    /// the result for repeated use. `build` is O(n²) in the number of systems,
    /// but n is typically small (< 100 systems per schedule).
    pub fn build(self) -> CompiledSchedule {
        let mut builder = CompiledScheduleBuilder::new();
        for entry in self.entries {
            match entry {
                ScheduleEntry::Serial(name, sys) => {
                    builder.push_serial(name, sys);
                }
                ScheduleEntry::Parallel(name, sys) => {
                    let access = sys.access().clone();
                    builder.push_parallel(name, access, sys);
                }
            }
        }
        builder.finish()
    }

    /// Number of registered systems (serial + parallel).
    pub fn system_count(&self) -> usize { self.entries.len() }

    /// Names of registered systems in insertion order.
    pub fn system_names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| match e {
            ScheduleEntry::Serial(n, _) | ScheduleEntry::Parallel(n, _) => n.as_str(),
        })
    }

    /// Remove all systems.
    pub fn clear(&mut self) { self.entries.clear(); }
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
