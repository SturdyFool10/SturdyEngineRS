// CompiledSchedule — wave-based parallel schedule executor.
//
// Produced by `Schedule::build()`. The build step analyses every system's
// `SystemAccess` declaration and groups non-conflicting systems into parallel
// *waves*. All systems within a wave run concurrently via rayon; waves execute
// sequentially.
//
// ## Wave composition rules
//
// Systems are processed in the order they were added to the `Schedule`:
//
// 1. A *serial* system (plain `FnMut(&mut World)`) always gets its own
//    singleton wave and acts as a hard barrier between the waves around it.
//
// 2. A *parallel* system (`ParallelSystem`) is added to the current open wave
//    if it has no conflict with any system already in that wave. If it does
//    conflict, the current wave is closed and a new one is opened with this
//    system as the first member.
//
// This greedy algorithm is correct (never places conflicting systems in the
// same wave) though not necessarily optimal (a truly optimal algorithm would
// solve NP-hard graph colouring). In practice, the greedy ordering is
// excellent for the typical ECS pattern where related systems are registered
// together.
//
// ## Execution
//
// For a **parallel wave**: each system receives its own `WorldCommands` buffer
// and a `WorldView` raw pointer. All systems in the wave run concurrently on
// rayon worker threads. After the wave, commands are drained in system order
// and applied to the World before the next wave starts.
//
// For a **serial wave**: the system runs directly with `&mut World`.
// `WorldCommands` is not needed — serial systems can mutate the world
// immediately.
//
// ## Thread safety
//
// The raw `*mut World` passed to rayon workers is wrapped in `SendWorld`, a
// newtype that asserts `Send`. This is sound because:
// - The World is exclusively borrowed by `run(&mut self, world: &mut World)`.
// - The scheduler guarantees disjoint write sets within each parallel wave.
// - Only one wave executes at a time (waves are sequentially ordered).

use super::World;
use super::schedule::System;
use super::parallel_system::{ErasedParallelSystem, SystemAccess};
use super::world_commands::WorldCommands;
use super::world_view::WorldView;

// ── Wave ──────────────────────────────────────────────────────────────────────

enum Wave {
    /// A single serial system — runs with exclusive `&mut World`.
    Serial {
        name: String,
        system: Box<dyn System>,
    },
    /// One or more parallel systems — all run concurrently per wave.
    Parallel {
        entries: Vec<ParallelEntry>,
    },
}

struct ParallelEntry {
    name: String,
    system: Box<dyn ErasedParallelSystem>,
}

// ── CompiledSchedule ──────────────────────────────────────────────────────────

/// A schedule that has been analysed for parallelism.
///
/// Produced by [`Schedule::build`](super::schedule::Schedule::build). Run it
/// with [`CompiledSchedule::run`].
///
/// ```ignore
/// let compiled = my_schedule.build();
/// // In the game loop:
/// compiled.run(&mut world);
/// ```
pub struct CompiledSchedule {
    waves: Vec<Wave>,
    /// When `true`, each wave logs system names and wall-clock timing to stderr.
    /// Enabled only in debug builds.
    pub debug_timing: bool,
}

// ── Builder — used by Schedule::build() ──────────────────────────────────────

pub(super) struct CompiledScheduleBuilder {
    waves: Vec<Wave>,
    /// Access union of all systems currently in the open parallel wave.
    open_wave_access: SystemAccess,
    open_wave_entries: Vec<ParallelEntry>,
}

impl CompiledScheduleBuilder {
    pub fn new() -> Self {
        Self {
            waves: Vec::new(),
            open_wave_access: SystemAccess::new(),
            open_wave_entries: Vec::new(),
        }
    }

    /// Close the open parallel wave (if non-empty) and push it to `waves`.
    fn close_parallel_wave(&mut self) {
        if self.open_wave_entries.is_empty() {
            return;
        }
        let entries = std::mem::take(&mut self.open_wave_entries);
        self.open_wave_access = SystemAccess::new();
        self.waves.push(Wave::Parallel { entries });
    }

    /// Add a serial system. Closes the current parallel wave, appends a
    /// singleton serial wave, then the next parallel wave starts fresh.
    pub fn push_serial(&mut self, name: String, system: Box<dyn System>) {
        self.close_parallel_wave();
        self.waves.push(Wave::Serial { name, system });
    }

    /// Add a parallel system. Adds to the current open wave if possible,
    /// otherwise closes the wave and starts a new one.
    pub fn push_parallel(
        &mut self,
        name: String,
        access: SystemAccess,
        system: Box<dyn ErasedParallelSystem>,
    ) {
        if self.open_wave_access.conflicts_with(&access) {
            // Conflict with something already in the open wave — close it.
            self.close_parallel_wave();
        }
        // Merge access into the open wave's combined access.
        self.open_wave_access.component_reads.extend(access.component_reads.iter().copied());
        self.open_wave_access.component_writes.extend(access.component_writes.iter().copied());
        self.open_wave_access.resource_reads.extend(access.resource_reads.iter().copied());
        self.open_wave_access.resource_writes.extend(access.resource_writes.iter().copied());
        self.open_wave_entries.push(ParallelEntry { name, system });
    }

    /// Finalise: close any trailing parallel wave and return the schedule.
    pub fn finish(mut self) -> CompiledSchedule {
        self.close_parallel_wave();
        CompiledSchedule { waves: self.waves, debug_timing: false }
    }
}

// ── Parallel entry runner ─────────────────────────────────────────────────────

/// Free function that accepts `SendWorld` by value, preventing Rust 2024
/// precision capture from extracting `*mut World` (which is `!Send`) when
/// the closure is formed. The closure moves the whole `SendWorld` struct into
/// this call, so the closure's captured type is `SendWorld`, not `*mut World`.
fn run_parallel_entry(
    raw: SendWorld,
    entry: &mut ParallelEntry,
    commands: &mut WorldCommands,
) {
    // SAFETY: CompiledSchedule::run holds &mut World exclusively. The
    // scheduler verified disjoint write sets for all systems in this wave.
    let view = unsafe { WorldView::from_ptr(raw.0) };
    entry.system.run(&view, commands);
}

// ── Execution ─────────────────────────────────────────────────────────────────

/// Newtype that marks a `*mut World` as `Send + Sync + Copy`.
///
/// SAFETY: Only used inside `CompiledSchedule::run` where the World is
/// exclusively borrowed for the entire schedule run, and the scheduler
/// guarantees disjoint write access within each parallel wave.
#[derive(Copy, Clone)]
struct SendWorld(*mut World);
unsafe impl Send for SendWorld {}
unsafe impl Sync for SendWorld {}

impl CompiledSchedule {
    /// Run the schedule against `world`.
    ///
    /// Waves execute sequentially. Systems within a parallel wave execute
    /// concurrently on rayon worker threads. `WorldCommands` queued during a
    /// parallel wave are applied (in system order) before the next wave starts.
    pub fn run(&mut self, world: &mut World) {
        for wave in &mut self.waves {
            match wave {
                Wave::Serial { name, system } => {
                    if self.debug_timing && cfg!(debug_assertions) {
                        let t0 = std::time::Instant::now();
                        system.run(world);
                        eprintln!(
                            "[CompiledSchedule] serial '{}': {:.3}ms",
                            name,
                            t0.elapsed().as_secs_f64() * 1000.0
                        );
                    } else {
                        system.run(world);
                    }
                }

                Wave::Parallel { entries } => {
                    let t0 = self.debug_timing.then(std::time::Instant::now);

                    // Allocate one WorldCommands per system (no shared state).
                    let mut commands_per_system: Vec<WorldCommands> =
                        (0..entries.len()).map(|_| WorldCommands::new()).collect();

                    // Build a raw pointer that can cross the rayon boundary.
                    let world_ptr = SendWorld(world as *mut World);

                    // Run all systems in this wave concurrently.
                    // rayon::scope bounds by lifetime rather than 'static, so
                    // we can safely pass &mut references into worker threads.
                    //
                    // Each closure is kept minimal: it moves `world_ptr: SendWorld`
                    // (Copy) as a whole into a helper function, preventing Rust 2024
                    // precision capture from extracting `world_ptr.0: *mut World`
                    // (which is !Send) as the captured value.
                    rayon::scope(|scope| {
                        for (entry, commands) in
                            entries.iter_mut().zip(commands_per_system.iter_mut())
                        {
                            let raw = world_ptr; // copy SendWorld for this iteration
                            scope.spawn(move |_| {
                                // Pass raw by value to prevent precision capture of raw.0.
                                run_parallel_entry(raw, entry, commands);
                            });
                        }
                    });

                    // Apply commands in system order (main thread, serial).
                    for cmds in &mut commands_per_system {
                        cmds.apply(world);
                    }

                    if self.debug_timing && cfg!(debug_assertions) {
                        if let Some(t0) = t0 {
                            let names: Vec<&str> =
                                entries.iter().map(|e| e.name.as_str()).collect();
                            eprintln!(
                                "[CompiledSchedule] wave [{}]: {:.3}ms",
                                names.join(", "),
                                t0.elapsed().as_secs_f64() * 1000.0
                            );
                        }
                    }
                }
            }
        }
    }

    /// Number of waves in the compiled schedule.
    pub fn wave_count(&self) -> usize {
        self.waves.len()
    }

    /// Total system count across all waves.
    pub fn system_count(&self) -> usize {
        self.waves.iter().map(|w| match w {
            Wave::Serial { .. } => 1,
            Wave::Parallel { entries } => entries.len(),
        }).sum()
    }

    /// Names of all systems, wave by wave. Each inner Vec is one wave.
    pub fn wave_names(&self) -> Vec<Vec<&str>> {
        self.waves.iter().map(|w| match w {
            Wave::Serial { name, .. } => vec![name.as_str()],
            Wave::Parallel { entries } => entries.iter().map(|e| e.name.as_str()).collect(),
        }).collect()
    }
}
