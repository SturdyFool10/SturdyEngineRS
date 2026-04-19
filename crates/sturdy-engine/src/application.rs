//! Application shell and winit integration.
//!
//! This module provides the [`EngineApp`] trait, [`WindowConfig`] builder,
//! and the [`run`] function that replaces the typical ~80 lines of winit
//! scaffolding with a simple trait implementation.
//!
//! # Example
//!
//! ```ignore
//! use sturdy_engine::{EngineApp, WindowConfig, Result};
//!
//! struct MyApp {
//!     renderer: Option<Renderer>,
//! }
//!
//! impl EngineApp for MyApp {
//!     type Error = std::io::Error;
//!
//!     fn init(engine: &Engine, surface: &Surface) -> Result<Self, Self::Error> {
//!         let renderer = Renderer::new(engine, surface)?;
//!         Ok(Self {
//!             renderer: Some(renderer),
//!         })
//!     }
//!
//!     fn render(&mut self, frame: &mut ShellFrame, surface_image: &SurfaceImage) -> Result<(), Self::Error> {
//!         if let Some(renderer) = self.renderer.as_mut() {
//!             renderer.render(&frame, surface_image)?;
//!         }
//!         Ok(())
//!     }
//!
//!     fn resize(&mut self, width: u32, height: u32) -> Result<(), Self::Error> {
//!         if let Some(renderer) = self.renderer.as_mut() {
//!             renderer.resize(width, height)?;
//!         }
//!         Ok(())
//!     }
//! }
//!
//! fn main() {
//!     sturdy_engine::run(
//!         WindowConfig::new("My App", 1280, 720)
//!             .with_title("My Game")
//!             .with_resizable(true)
//!             .with_hdr(true),
//!         MyApp { renderer: None },
//!     );
//! }
//! ```

use std::time::Instant;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use sturdy_engine_core::SurfaceSize;

use crate::{Engine, NativeSurfaceDesc, Result as EngineResult, Surface, SurfaceImage};

/// Configuration for the application shell window.
#[derive(Clone, Debug)]
pub struct WindowConfig {
    title: String,
    width: u32,
    height: u32,
    resizable: bool,
    prefer_hdr: bool,
}

impl WindowConfig {
    /// Create a new window configuration with the given title, width, and height.
    pub fn new(title: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            title: title.into(),
            width,
            height,
            resizable: false,
            prefer_hdr: false,
        }
    }

    /// Set the window title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Set the window size.
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Set whether the window is resizable.
    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Set whether HDR is preferred for the surface.
    pub fn with_hdr(mut self, prefer_hdr: bool) -> Self {
        self.prefer_hdr = prefer_hdr;
        self
    }

    /// Get the title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Get the width.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Get the height.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Get whether the window is resizable.
    pub fn resizable(&self) -> bool {
        self.resizable
    }

    /// Get whether HDR is preferred.
    pub fn prefer_hdr(&self) -> bool {
        self.prefer_hdr
    }
}

/// Application handler trait for the engine's application shell.
///
/// Implement this trait to create a minimal application that integrates
/// with the engine's event loop. The engine handles window creation,
/// event processing, and surface management.
pub trait EngineApp {
    /// The error type returned by the application's methods.
    type Error: std::error::Error;

    /// Initialize the application after the engine and surface are created.
    ///
    /// This is called once, after the window is created and the surface
    /// is ready. Return `Ok(Self)` with the initialized application state.
    fn init(engine: &Engine, surface: &Surface) -> Result<Self, Self::Error>
    where
        Self: Sized;

    /// Render a frame.
    ///
    /// Called every frame after the window is created. The `frame` parameter
    /// provides the render frame API, and `surface_image` is the swapchain
    /// image to render to.
    fn render(
        &mut self,
        frame: &mut ShellFrame<'_>,
        surface_image: &SurfaceImage,
    ) -> Result<(), Self::Error>;

    /// Handle window resize.
    ///
    /// Called when the window is resized. The new dimensions are provided
    /// in logical pixels.
    fn resize(&mut self, width: u32, height: u32) -> Result<(), Self::Error>;
}

/// A render frame wrapper that provides the frame API and surface image.
pub struct ShellFrame<'a> {
    inner: crate::RenderFrame,
    #[allow(dead_code)]
    surface_image: &'a SurfaceImage,
}

impl<'a> ShellFrame<'a> {
    pub(crate) fn new(inner: crate::RenderFrame, surface_image: &'a SurfaceImage) -> Self {
        Self {
            inner,
            surface_image,
        }
    }

    /// Get the underlying frame.
    pub fn inner(&self) -> &crate::RenderFrame {
        &self.inner
    }

    /// Get the underlying frame mutably.
    pub fn inner_mut(&mut self) -> &mut crate::RenderFrame {
        &mut self.inner
    }

    /// Finish rendering and present to the surface.
    ///
    /// This is a convenience method that calls `flush()`, `wait()`, and
    /// `surface.present()` in sequence.
    pub fn finish_and_present(&mut self, surface: &Surface) -> EngineResult<()> {
        self.inner.flush()?;
        self.inner.wait()?;
        surface.present()?;
        Ok(())
    }
}

/// Run the application with the given window configuration.
///
/// This function creates the event loop, window, and engine, then drives
/// the application lifecycle by calling the `EngineApp` trait methods.
///
/// # Example
///
/// ```ignore
/// sturdy_engine::run(
///     WindowConfig::new("My App", 1280, 720)
///         .with_title("My Game")
///         .with_resizable(true),
///     MyApp { renderer: None },
/// );
/// ```
#[cfg(not(target_arch = "wasm32"))]
pub fn run<App: EngineApp>(config: WindowConfig, _app: App)
where
    App::Error: std::fmt::Debug,
{
    use winit::{
        dpi::LogicalSize,
        event_loop::{ControlFlow, EventLoop},
        window::Window,
    };

    // Create the engine
    let engine = Engine::new().expect("failed to create engine");

    // Create event loop
    let event_loop: EventLoop<()> = EventLoop::new().expect("failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Poll);

    // Create window
    #[allow(deprecated)]
    let window = event_loop
        .create_window(
            Window::default_attributes()
                .with_title(&config.title)
                .with_inner_size(LogicalSize::new(config.width as f64, config.height as f64))
                .with_resizable(config.resizable),
        )
        .expect("failed to create window");

    // Extract raw handles for surface creation
    let display = window
        .display_handle()
        .expect("failed to get display handle");
    let window_handle = window.window_handle().expect("failed to get window handle");

    // SAFETY: winit guarantees these raw handles are valid while the window exists.
    // The window outlives this scope and the surface created from it.
    let raw_display: RawDisplayHandle = unsafe { std::mem::transmute_copy(&display) };
    let raw_window: RawWindowHandle = unsafe { std::mem::transmute_copy(&window_handle) };

    // Create surface from window
    let surface = engine
        .create_surface(NativeSurfaceDesc::new(
            raw_display,
            raw_window,
            SurfaceSize {
                width: config.width.max(1),
                height: config.height.max(1),
            },
        ))
        .expect("failed to create surface");

    // Initialize the application
    let app_state = match App::init(&engine, &surface) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("failed to initialize application: {e:?}");
            std::process::exit(1);
        }
    };

    // Run event loop
    event_loop
        .run_app(&mut ShellApp {
            engine,
            window: Some(window),
            surface,
            app_state,
            started_at: Instant::now(),
            _config: config,
        })
        .expect("event loop exited unexpectedly");
}

// Internal winit ApplicationHandler implementation
#[cfg(not(target_arch = "wasm32"))]
struct ShellApp<App: EngineApp> {
    engine: Engine,
    window: Option<winit::window::Window>,
    surface: Surface,
    app_state: App,
    #[allow(dead_code)]
    started_at: Instant,
    _config: WindowConfig,
}

#[cfg(not(target_arch = "wasm32"))]
impl<App: EngineApp> winit::application::ApplicationHandler for ShellApp<App>
where
    App::Error: std::fmt::Debug,
{
    fn resumed(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        // Window already created
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        match event {
            winit::event::WindowEvent::CloseRequested => {
                std::process::exit(0);
            }
            winit::event::WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    if let Err(e) = self.app_state.resize(size.width, size.height) {
                        eprintln!("resize failed: {e:?}");
                        std::process::exit(1);
                    }
                    if let Err(e) = self.surface.resize(SurfaceSize {
                        width: size.width.max(1),
                        height: size.height.max(1),
                    }) {
                        eprintln!("surface resize failed: {e:?}");
                    }
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                    }
                }
            }
            winit::event::WindowEvent::RedrawRequested => {
                let surface_image = match self.surface.acquire_image() {
                    Ok(img) => img,
                    Err(e) => {
                        eprintln!("failed to acquire surface image: {e:?}");
                        std::process::exit(1);
                    }
                };

                let frame = self.engine.begin_render_frame_for(&surface_image);
                let frame = match frame {
                    Ok(f) => f,
                    Err(e) => {
                        eprintln!("failed to begin render frame: {e:?}");
                        std::process::exit(1);
                    }
                };

                let mut render_frame = ShellFrame::new(frame, &surface_image);

                if let Err(e) = self.app_state.render(&mut render_frame, &surface_image) {
                    eprintln!("render failed: {e:?}");
                    std::process::exit(1);
                }

                // Present
                if let Err(e) = render_frame.finish_and_present(&self.surface) {
                    eprintln!("present failed: {e:?}");
                    std::process::exit(1);
                }

                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}
