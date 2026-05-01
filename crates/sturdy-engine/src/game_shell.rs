//! Default game runtime shell.
//!
//! [`GameApp`] and [`run_game`] are a zero-plumbing alternative to [`EngineApp`]
//! for game projects. The shell owns the [`InputHub`], manages fixed-step
//! accumulation with spiral-of-death protection, and drives [`GameApp::fixed_update`]
//! the right number of times per frame before calling [`GameApp::render`].
//!
//! # Minimal example
//!
//! ```ignore
//! use std::time::Duration;
//! use sturdy_engine::{Engine, Surface, SurfaceImage, ShellFrame, GameApp, GameConfig,
//!                     GameContext, WindowConfig};
//!
//! struct MyGame { /* ... */ }
//!
//! impl GameApp for MyGame {
//!     type Error = sturdy_engine::Error;
//!
//!     fn init(engine: &Engine, surface: &Surface) -> sturdy_engine::Result<Self, Self::Error> {
//!         Ok(Self { /* ... */ })
//!     }
//!
//!     fn render(
//!         &mut self,
//!         frame: &mut ShellFrame<'_>,
//!         surface_image: &SurfaceImage,
//!         ctx: &GameContext<'_>,
//!     ) -> sturdy_engine::Result<(), Self::Error> {
//!         let swapchain = frame.inner().swapchain_image(surface_image)?;
//!         // ... record render passes ...
//!         Ok(())
//!     }
//! }
//!
//! fn main() {
//!     sturdy_engine::run_game::<MyGame>(
//!         GameConfig::new(WindowConfig::new("My Game", 1280, 720))
//!             .with_fixed_step(Duration::from_secs_f32(1.0 / 60.0)),
//!     );
//! }
//! ```

use std::{cell::Cell, time::Duration};

use crate::{Engine, EngineApp, InputHub, ShellFrame, Surface, SurfaceImage, WindowConfig};

// Thread-local used to thread `fixed_step` from `run_game` into `GameShell::init`,
// which is called through the fixed `EngineApp::init` signature.
thread_local! {
    static GAME_FIXED_STEP: Cell<Option<Duration>> = const { Cell::new(None) };
}

/// Configuration for a game session.
pub struct GameConfig {
    /// Window and surface configuration.
    pub window: WindowConfig,
    /// Fixed simulation step size. `None` means variable-step only.
    pub fixed_step: Option<Duration>,
}

impl GameConfig {
    pub fn new(window: WindowConfig) -> Self {
        Self { window, fixed_step: None }
    }

    /// Set the fixed simulation step size.
    ///
    /// When set, [`GameApp::fixed_update`] is called the correct number of times
    /// per frame so the simulation advances in equal-size chunks regardless of
    /// frame rate. [`GameContext::fixed_alpha`] provides the interpolation factor
    /// for smooth rendering between steps.
    pub fn with_fixed_step(mut self, step: Duration) -> Self {
        self.fixed_step = Some(step);
        self
    }
}

/// Context passed to [`GameApp::fixed_update`] for each simulation step.
pub struct FixedUpdateContext<'a> {
    /// Which step this is within the current frame, starting at 0.
    pub step_index: u32,
    /// The fixed simulation step size.
    pub fixed_step: Duration,
    /// Unconsumed time left in the accumulator after all steps for this frame
    /// have been scheduled. Identical for every step within a single frame.
    pub pacing_error: Duration,
    /// Current input state. Snapshot is stable across all steps in one frame.
    pub input: &'a InputHub,
}

impl FixedUpdateContext<'_> {
    /// Fixed step size in seconds as `f32`. Convenience for physics math.
    pub fn fixed_step_secs(&self) -> f32 {
        self.fixed_step.as_secs_f32()
    }
}

/// Per-frame game context passed to [`GameApp::render`].
pub struct GameContext<'a> {
    /// Interpolation factor `[0, 1)` between the last fixed tick and the next.
    ///
    /// Multiply rendered object positions by `(1.0 - alpha) * prev + alpha * next`
    /// to get sub-step smooth rendering without changing the simulation rate.
    /// `0.0` when no fixed step is configured.
    pub fixed_alpha: f32,
    /// How many fixed-step simulation ticks ran before this render frame.
    pub fixed_updates_this_frame: u32,
    /// The configured fixed step size, if any.
    pub fixed_step: Option<Duration>,
    /// Unconsumed accumulator time after all fixed steps for this frame.
    /// Grows when the fixed step is too small for the current frame rate.
    pub pacing_error: Duration,
    /// Current input state.
    pub input: &'a InputHub,
}

/// Application trait for the default game runtime shell.
///
/// Implement this instead of [`EngineApp`] to get automatic fixed-step
/// accumulation, input routing, and gamepad polling with no boilerplate.
///
/// The shell owns the [`InputHub`]. Configure action bindings by overriding
/// [`configure_input`](GameApp::configure_input), which is called once after
/// [`init`](GameApp::init) before the first frame.
pub trait GameApp: Sized {
    type Error: std::error::Error;

    /// Create the application after the engine and surface are ready.
    fn init(engine: &Engine, surface: &Surface) -> Result<Self, Self::Error>;

    /// Configure the shell-owned [`InputHub`] before the first frame.
    ///
    /// Override to bind named actions to keys, mouse buttons, or gamepad inputs:
    ///
    /// ```ignore
    /// fn configure_input(&mut self, hub: &mut InputHub) {
    ///     hub.action_map_mut().bind("jump", Keybind::new(&[], "Space"));
    ///     hub.action_map_mut().bind("move_forward", Keybind::new(&[], "w"));
    /// }
    /// ```
    fn configure_input(&mut self, _hub: &mut InputHub) {}

    /// Advance the simulation by one fixed step.
    ///
    /// Called zero or more times per frame before [`render`](GameApp::render),
    /// depending on how much real time has elapsed. Input state in `ctx` is
    /// stable across all steps in a single frame.
    fn fixed_update(&mut self, _ctx: &FixedUpdateContext<'_>) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Record render commands for the current frame.
    ///
    /// `ctx.fixed_alpha` provides the sub-step interpolation factor for smooth
    /// rendering. `ctx.input` provides the current input state. `frame` and
    /// `surface_image` are the render frame and swapchain image, identical to
    /// [`EngineApp::render`].
    fn render(
        &mut self,
        frame: &mut ShellFrame<'_>,
        surface_image: &SurfaceImage,
        ctx: &GameContext<'_>,
    ) -> Result<(), Self::Error>;

    /// Called when the window is resized. Default is a no-op.
    fn resize(&mut self, _width: u32, _height: u32) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Internal `EngineApp` wrapper that drives a [`GameApp`].
struct GameShell<App: GameApp> {
    app: App,
    hub: InputHub,
    fixed_step: Option<Duration>,
    accumulator: Duration,
}

impl<App: GameApp> GameShell<App> {
    /// Advance the fixed-step accumulator by `delta` and return
    /// `(fixed_alpha, step_count, pacing_error)`.
    fn advance_fixed_steps(&mut self, delta: Duration) -> (f32, u32, Duration) {
        let Some(step) = self.fixed_step else {
            return (0.0, 0, Duration::ZERO);
        };

        self.accumulator += delta;

        // Spiral-of-death cap: never accumulate more than 8 steps worth of debt.
        let cap = step * 8;
        if self.accumulator > cap {
            self.accumulator = cap;
        }

        let mut count = 0u32;
        while self.accumulator >= step {
            self.accumulator -= step;
            count += 1;
        }

        let alpha = self.accumulator.as_secs_f64() as f32 / step.as_secs_f32();
        (alpha, count, self.accumulator)
    }
}

impl<App: GameApp> EngineApp for GameShell<App>
where
    App::Error: std::fmt::Debug,
{
    type Error = App::Error;

    fn init(engine: &Engine, surface: &Surface) -> Result<Self, App::Error> {
        let fixed_step = GAME_FIXED_STEP.get();
        let mut app = App::init(engine, surface)?;
        let mut hub = InputHub::new();
        app.configure_input(&mut hub);
        Ok(Self {
            app,
            hub,
            fixed_step,
            accumulator: Duration::ZERO,
        })
    }

    fn render(
        &mut self,
        frame: &mut ShellFrame<'_>,
        surface_image: &SurfaceImage,
    ) -> Result<(), App::Error> {
        let delta = frame.frame_time().delta;
        let (fixed_alpha, step_count, pacing_error) = self.advance_fixed_steps(delta);

        // Run fixed updates. `&self.hub` and `&mut self.app` are disjoint struct
        // fields, so Rust permits both borrows simultaneously.
        for step_index in 0..step_count {
            let ctx = FixedUpdateContext {
                step_index,
                fixed_step: self.fixed_step.unwrap_or_default(),
                pacing_error,
                input: &self.hub,
            };
            self.app.fixed_update(&ctx)?;
        }

        let ctx = GameContext {
            fixed_alpha,
            fixed_updates_this_frame: step_count,
            fixed_step: self.fixed_step,
            pacing_error,
            input: &self.hub,
        };
        self.app.render(frame, surface_image, &ctx)?;

        Ok(())
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(), App::Error> {
        self.app.resize(width, height)
    }

    fn input_hub(&mut self) -> Option<&mut InputHub> {
        Some(&mut self.hub)
    }
}

/// Run a game application with the given configuration.
///
/// Creates the event loop, window, and engine, then drives the [`GameApp`]
/// lifecycle. Returns only if the window is closed.
///
/// # Example
///
/// ```ignore
/// sturdy_engine::run_game::<MyGame>(
///     GameConfig::new(WindowConfig::new("My Game", 1280, 720))
///         .with_fixed_step(Duration::from_secs_f32(1.0 / 60.0)),
/// );
/// ```
#[cfg(not(target_arch = "wasm32"))]
pub fn run_game<App: GameApp>(config: GameConfig)
where
    App::Error: std::fmt::Debug,
{
    if let Err(error) = try_run_game::<App>(config) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

/// Try to run a game application, returning setup errors to the caller.
///
/// Unlike [`run_game`], setup and event-loop initialization errors are returned
/// rather than printed and exited. Fatal runtime errors still terminate the process.
#[cfg(not(target_arch = "wasm32"))]
pub fn try_run_game<App: GameApp>(config: GameConfig) -> Result<(), String>
where
    App::Error: std::fmt::Debug,
{
    GAME_FIXED_STEP.set(config.fixed_step);
    crate::application::try_run::<GameShell<App>>(config.window)
}
