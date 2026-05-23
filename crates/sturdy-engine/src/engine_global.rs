use std::sync::{Mutex, OnceLock};

use crate::{Engine, FrameTimingReport};

// Process-wide engine singleton, set once at shell startup.
// All fields of Engine are Arc-backed, so clone is O(1) reference-count bumps.
static GLOBAL_ENGINE: OnceLock<Engine> = OnceLock::new();

// Latest frame-timing report, updated each frame by the runtime shell.
// Stored behind a mutex so Engine::frame_timing() can read it from any thread.
static FRAME_TIMING: OnceLock<Mutex<Option<FrameTimingReport>>> = OnceLock::new();

fn frame_timing_cell() -> &'static Mutex<Option<FrameTimingReport>> {
    FRAME_TIMING.get_or_init(|| Mutex::new(None))
}

pub(crate) fn set_engine(engine: &Engine) {
    let _ = GLOBAL_ENGINE.set(engine.clone());
}

pub(crate) fn engine() -> Option<&'static Engine> {
    GLOBAL_ENGINE.get()
}

pub(crate) fn is_engine_set() -> bool {
    GLOBAL_ENGINE.get().is_some()
}

pub(crate) fn set_frame_timing(report: FrameTimingReport) {
    *frame_timing_cell()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(report);
}

pub(crate) fn frame_timing() -> Option<FrameTimingReport> {
    frame_timing_cell()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}
