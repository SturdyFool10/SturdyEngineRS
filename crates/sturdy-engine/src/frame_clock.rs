use std::time::{Duration, Instant};

/// Per-frame timing information returned by [`FrameClock`].
#[derive(Copy, Clone, Debug)]
pub struct FrameTime {
    /// Time elapsed since the previous frame. Zero on the very first frame.
    pub delta: Duration,
    /// Total time elapsed since the clock was created.
    pub elapsed: Duration,
    /// Monotonically increasing frame counter, starting at 0.
    pub frame: u64,
}

impl FrameTime {
    /// Delta time in seconds as `f32`. The most common form needed by game logic.
    pub fn delta_secs(&self) -> f32 {
        self.delta.as_secs_f32()
    }

    /// Elapsed time in seconds as `f32`.
    pub fn elapsed_secs(&self) -> f32 {
        self.elapsed.as_secs_f32()
    }
}

/// Monotonic frame timer for game loops.
///
/// Call [`FrameClock::tick`] once per frame (typically inside `acquire_frame`)
/// and read [`FrameClock::time`] or the convenience accessors anywhere in the
/// same frame.
///
/// ```ignore
/// let mut clock = FrameClock::new();
///
/// // inside the game loop:
/// clock.tick();
/// let dt = clock.delta_secs();
/// player_pos += velocity * dt;
/// ```
pub struct FrameClock {
    start: Instant,
    last: Instant,
    time: FrameTime,
    /// Optional fixed-step accumulator. If `Some(step)`, `fixed_updates()`
    /// returns how many simulation ticks happened this frame.
    fixed_step: Option<Duration>,
    accumulator: Duration,
}

impl Default for FrameClock {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameClock {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            start: now,
            last: now,
            time: FrameTime {
                delta: Duration::ZERO,
                elapsed: Duration::ZERO,
                frame: 0,
            },
            fixed_step: None,
            accumulator: Duration::ZERO,
        }
    }

    /// Create a clock with a fixed-step size for `fixed_updates()` / `fixed_alpha()`.
    pub fn with_fixed_step(step: Duration) -> Self {
        let mut clock = Self::new();
        clock.fixed_step = Some(step);
        clock
    }

    /// Advance the clock by one frame. Call once per frame before reading time values.
    ///
    /// Returns the [`FrameTime`] for this frame.
    pub fn tick(&mut self) -> FrameTime {
        let now = Instant::now();
        let delta = now.duration_since(self.last);
        let elapsed = now.duration_since(self.start);
        self.last = now;
        self.time = FrameTime {
            delta,
            elapsed,
            frame: self.time.frame + 1,
        };
        if let Some(step) = self.fixed_step {
            self.accumulator += delta;
            // Cap accumulator to avoid spiral-of-death under sustained heavy load.
            let cap = step * 8;
            if self.accumulator > cap {
                self.accumulator = cap;
            }
        }
        self.time
    }

    /// The [`FrameTime`] produced by the most recent [`tick`](Self::tick) call.
    pub fn time(&self) -> FrameTime {
        self.time
    }

    /// Delta time since the previous frame.
    pub fn delta(&self) -> Duration {
        self.time.delta
    }

    /// Delta time in seconds as `f32`.
    pub fn delta_secs(&self) -> f32 {
        self.time.delta.as_secs_f32()
    }

    /// Total time elapsed since the clock was created.
    pub fn elapsed(&self) -> Duration {
        self.time.elapsed
    }

    /// Total elapsed time in seconds as `f32`.
    pub fn elapsed_secs(&self) -> f32 {
        self.time.elapsed.as_secs_f32()
    }

    /// Monotonically increasing frame index. Starts at 1 after the first tick.
    pub fn frame(&self) -> u64 {
        self.time.frame
    }

    /// How many fixed-step simulation ticks should run this frame.
    ///
    /// Returns 0 if no fixed step was configured. Advances the internal
    /// accumulator; call this exactly once per frame.
    pub fn fixed_updates(&mut self) -> u32 {
        let Some(step) = self.fixed_step else {
            return 0;
        };
        let mut count = 0u32;
        while self.accumulator >= step {
            self.accumulator -= step;
            count += 1;
        }
        count
    }

    /// Interpolation factor `[0, 1)` between the last fixed tick and the next.
    ///
    /// Use this to smoothly interpolate rendered positions when using a fixed
    /// simulation step. Returns `0.0` if no fixed step was configured.
    pub fn fixed_alpha(&self) -> f32 {
        match self.fixed_step {
            Some(step) if step > Duration::ZERO => {
                self.accumulator.as_secs_f64() as f32 / step.as_secs_f32()
            }
            _ => 0.0,
        }
    }
}
