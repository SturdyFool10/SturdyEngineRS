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
//!     sturdy_engine::run::<MyApp>(
//!         WindowConfig::new("My App", 1280, 720)
//!             .with_title("My Game")
//!             .with_resizable(true)
//!             .with_hdr(true),
//!     );
//! }
//! ```

use std::time::Instant;

use sturdy_engine_core::SurfaceSize;

use crate::{
    AntiAliasingMode, AntiAliasingPass, AppRuntime, BloomConfig, BloomPass,
    DebugImageRegistry, DefaultSceneTargetConfig, DiagnosticLevel, Engine, GraphImage,
    Result as EngineResult, RuntimeController, RuntimeDiagnostics, RuntimeGraphDiagnostics,
    ShaderProgram, Surface, SurfaceHdrPreference, SurfaceImage,
};

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

    /// Handle a key press. `key` is the logical character string (e.g. `"b"`, `"B"`).
    ///
    /// Only called on `ElementState::Pressed`. Default implementation does nothing.
    fn key_pressed(&mut self, _key: &str, _surface: &mut Surface) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// A render frame wrapper that provides the frame API and surface image.
pub struct ShellFrame<'a> {
    inner: crate::RenderFrame,
    #[allow(dead_code)]
    surface_image: &'a SurfaceImage,
    default_scene_target: DefaultSceneTargetConfig,
    debug_images: DebugImageRegistry,
    controller: RuntimeController,
}

pub struct RuntimeMotionDebugDesc<'a> {
    pub motion_vectors: RuntimeMotionVectorDesc<'a>,
    pub target: &'a GraphImage,
    pub program: &'a ShaderProgram,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MotionVectorSpace {
    CameraLocal,
    NonCameraLocal,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MotionVectorLayer {
    World,
    CameraLocked,
}

#[derive(Copy, Clone)]
pub struct RuntimeMotionVectorDesc<'a> {
    pub image: &'a GraphImage,
    pub space: MotionVectorSpace,
    pub layer: MotionVectorLayer,
}

pub struct RuntimePostProcessDesc<'a, T: bytemuck::Pod> {
    pub scene_color: &'a GraphImage,
    pub motion_vectors: Option<RuntimeMotionVectorDesc<'a>>,
    pub bloom_pass: Option<&'a BloomPass>,
    pub bloom_config: Option<&'a BloomConfig>,
    pub bloom_only: bool,
    pub aa_pass: &'a AntiAliasingPass,
    pub aa_mode: AntiAliasingMode,
    pub swapchain: &'a GraphImage,
    pub tonemap_program: &'a ShaderProgram,
    pub tonemap_constants: &'a T,
    pub motion_debug: Option<RuntimeMotionDebugDesc<'a>>,
}

pub struct RuntimePostProcessOutput {
    pub hdr_composite: GraphImage,
    pub final_color: GraphImage,
}

impl<'a> ShellFrame<'a> {
    pub(crate) fn new(
        inner: crate::RenderFrame,
        surface_image: &'a SurfaceImage,
        default_scene_target: DefaultSceneTargetConfig,
        debug_images: DebugImageRegistry,
        controller: RuntimeController,
    ) -> Self {
        Self {
            inner,
            surface_image,
            default_scene_target,
            debug_images,
            controller,
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

    /// Create the runtime-owned default HDR scene target for this frame.
    pub fn default_hdr_scene_target(
        &self,
        name: impl Into<String>,
        requested_msaa_samples: u8,
    ) -> EngineResult<GraphImage> {
        self.default_scene_target
            .create(&self.inner, name, requested_msaa_samples)
    }

    /// Resolve the runtime-owned default HDR scene target for downstream post-processing.
    pub fn resolve_default_hdr_scene_target(
        &self,
        scene_target: &GraphImage,
        resolved_name: impl Into<String>,
    ) -> EngineResult<GraphImage> {
        self.default_scene_target
            .resolve(&self.inner, scene_target, resolved_name)
    }

    /// Register a named debug image with the runtime-owned registry.
    pub fn register_debug_image(&self, name: impl Into<String>, image: &GraphImage) {
        self.debug_images.register(image, name);
    }

    /// Return the names of debug images registered for this frame.
    pub fn debug_image_names(&self) -> Vec<String> {
        self.debug_images.names()
    }

    /// Return the current runtime settings snapshot.
    pub fn runtime_diagnostics(&self) -> RuntimeDiagnostics {
        self.controller.diagnostics()
    }

    /// Return the current engine-owned overlay lines.
    pub fn runtime_overlay_lines(&self) -> Vec<String> {
        self.controller.overlay_lines()
    }

    /// Replace the engine-owned overlay lines for this frame.
    pub fn set_runtime_overlay_lines<I, S>(&self, lines: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.controller
            .set_overlay_lines(lines.into_iter().map(Into::into).collect());
    }

    /// Format the current diagnostics into simple overlay-friendly lines.
    pub fn default_runtime_overlay_lines(&self) -> Vec<String> {
        let diagnostics = self.runtime_diagnostics();
        let debug_images = if diagnostics.debug_images.is_empty() {
            "none".to_string()
        } else {
            diagnostics.debug_images.join(", ")
        };
        vec![
            format!(
                "runtime: backend={:?} adapter={}",
                diagnostics.backend,
                diagnostics
                    .adapter_name
                    .as_deref()
                    .unwrap_or("<unknown>")
            ),
            format!(
                "surface: {:?} {:?} {}",
                diagnostics.surface_format,
                diagnostics.surface_color_space,
                diagnostics
                    .present_mode
                    .map(|mode| format!("{mode:?}"))
                    .unwrap_or_else(|| "present=unknown".to_string())
            ),
            format!(
                "post: aa={} msaa={} bloom={} bloom-only={} hdr={}",
                diagnostics.aa_mode_label.as_deref().unwrap_or("n/a"),
                diagnostics
                    .actual_msaa_samples
                    .map(|samples| format!("{samples}x"))
                    .unwrap_or_else(|| "n/a".to_string()),
                diagnostics
                    .bloom_enabled
                    .map(|value| if value { "on" } else { "off" })
                    .unwrap_or("n/a"),
                diagnostics
                    .bloom_only
                    .map(|value| if value { "on" } else { "off" })
                    .unwrap_or("n/a"),
                if diagnostics.hdr_output { "on" } else { "off" },
            ),
            format!(
                "graph: passes={} images={} warnings={} errors={} timings={}",
                diagnostics.graph.pass_count,
                diagnostics.graph.image_count,
                diagnostics.graph.warning_count,
                diagnostics.graph.error_count,
                if diagnostics.timings.available {
                    "available"
                } else {
                    "pending"
                },
            ),
            format!(
                "motion: {}",
                diagnostics
                    .motion_validation
                    .as_deref()
                    .unwrap_or("unpublished")
            ),
            format!("debug images: {debug_images}"),
        ]
    }

    /// Publish per-frame runtime diagnostics gathered from the current render graph and shell state.
    pub fn publish_runtime_diagnostics(
        &self,
        aa_mode_label: impl Into<String>,
        actual_msaa_samples: u8,
        bloom_enabled: bool,
        bloom_only: bool,
    ) {
        let report = self.inner.describe();
        let diagnostics = self.inner.validate();
        let graph = RuntimeGraphDiagnostics {
            pass_count: report.passes.len(),
            image_count: report.images.len(),
            warning_count: diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.level == DiagnosticLevel::Warning)
                .count(),
            error_count: diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.level == DiagnosticLevel::Error)
                .count(),
        };
        let debug_images = self.debug_image_names();
        self.controller.update_diagnostics(|current| {
            current.aa_mode_label = Some(aa_mode_label.into());
            current.actual_msaa_samples = Some(actual_msaa_samples);
            current.bloom_enabled = Some(bloom_enabled);
            current.bloom_only = Some(bloom_only);
            current.debug_images = debug_images;
            current.graph = graph;
        });
    }

    /// Run the default HDR post chain from scene color through tonemap.
    pub fn run_default_post_process<T: bytemuck::Pod>(
        &self,
        desc: RuntimePostProcessDesc<'_, T>,
    ) -> EngineResult<RuntimePostProcessOutput> {
        let show_motion_debug = desc.motion_debug.is_some();
        let hdr_composite = if let (Some(bloom_pass), Some(bloom_config)) =
            (desc.bloom_pass, desc.bloom_config)
        {
            bloom_pass.execute(desc.scene_color, &self.inner, bloom_config, desc.bloom_only)?
        } else {
            desc.scene_color.clone()
        };

        let (motion_source, motion_validation) = classify_motion_vectors(desc.motion_vectors);
        self.controller.update_diagnostics(|current| {
            current.motion_validation = Some(motion_validation);
        });

        let anti_aliased = desc.aa_pass.execute_with_motion_vectors(
            &self.inner,
            &hdr_composite,
            motion_source,
            desc.aa_mode,
        )?;

        let final_input = if let Some(motion_debug) = desc.motion_debug {
            self.register_debug_image("motion_source", motion_debug.motion_vectors.image);
            self.inner
                .set_sampler("motion_sampler", crate::SamplerPreset::Linear);
            motion_debug.target.execute_shader_auto(motion_debug.program)?;
            motion_debug.target.clone()
        } else {
            self.register_debug_image("hdr_composite", &anti_aliased);
            anti_aliased.clone()
        };
        if show_motion_debug {
            self.register_debug_image("hdr_composite", &final_input);
        }
        desc.swapchain.execute_shader_with_constants_auto(
            desc.tonemap_program,
            desc.tonemap_constants,
        )?;

        Ok(RuntimePostProcessOutput {
            hdr_composite: final_input,
            final_color: desc.swapchain.clone(),
        })
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

fn classify_motion_vectors(
    motion_vectors: Option<RuntimeMotionVectorDesc<'_>>,
) -> (Option<&GraphImage>, String) {
    match motion_vectors {
        Some(desc)
            if desc.space == MotionVectorSpace::CameraLocal
                && desc.layer == MotionVectorLayer::World =>
        {
            (Some(desc.image), "camera-local world motion".to_string())
        }
        Some(desc) if desc.layer == MotionVectorLayer::CameraLocked => (
            None,
            "camera-locked layer bypasses world temporal motion".to_string(),
        ),
        Some(_) => (
            None,
            "non-camera-local motion vectors ignored by default temporal path".to_string(),
        ),
        None => (None, "no motion vectors supplied".to_string()),
    }
}

/// Run the application with the given window configuration.
///
/// This function creates the event loop, window, and engine, then drives
/// the application lifecycle by calling the `EngineApp` trait methods.
/// The app is constructed via `App::init` after the window and surface are
/// ready — you never need to construct a dummy instance manually.
///
/// # Example
///
/// ```ignore
/// sturdy_engine::run::<MyApp>(
///     WindowConfig::new("My App", 1280, 720).with_resizable(true),
/// );
/// ```
#[cfg(not(target_arch = "wasm32"))]
pub fn run<App: EngineApp>(config: WindowConfig)
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

    // Create surface from window
    let surface = engine
        .create_surface_for_window_with_hdr(
            &window,
            SurfaceSize {
                width: config.width.max(1),
                height: config.height.max(1),
            },
            if config.prefer_hdr {
                SurfaceHdrPreference::ScRgb
            } else {
                SurfaceHdrPreference::Sdr
            },
        )
        .expect("failed to create surface");

    // Initialize the application
    let app_state = match App::init(&engine, &surface) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("failed to initialize application: {e:?}");
            std::process::exit(1);
        }
    };

    let runtime = AppRuntime::new(engine, surface);

    // Run event loop
    event_loop
        .run_app(&mut ShellApp {
            runtime,
            window: Some(window),
            app_state,
            started_at: Instant::now(),
            _config: config,
        })
        .expect("event loop exited unexpectedly");
}

// Internal winit ApplicationHandler implementation
#[cfg(not(target_arch = "wasm32"))]
struct ShellApp<App: EngineApp> {
    runtime: AppRuntime,
    window: Option<winit::window::Window>,
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
                    if let Err(e) = self.runtime.surface_mut().resize(SurfaceSize {
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
            winit::event::WindowEvent::KeyboardInput { event, .. } => {
                use winit::event::ElementState;
                use winit::keyboard::Key;
                if event.state == ElementState::Pressed {
                    if let Key::Character(s) = &event.logical_key {
                        if let Err(e) = self
                            .app_state
                            .key_pressed(s.as_str(), self.runtime.surface_mut())
                        {
                            eprintln!("key handler failed: {e:?}");
                            std::process::exit(1);
                        }
                    }
                }
            }
            winit::event::WindowEvent::RedrawRequested => {
                let mut runtime_frame = match self.runtime.acquire_frame() {
                    Ok(frame) => frame,
                    Err(e) => {
                        eprintln!("failed to acquire runtime frame: {e:?}");
                        std::process::exit(1);
                    }
                };

                let mut render_frame = runtime_frame.shell_frame();

                if let Err(e) = self
                    .app_state
                    .render(&mut render_frame, runtime_frame.surface_image())
                {
                    eprintln!("render failed: {e:?}");
                    std::process::exit(1);
                }

                // Present
                if let Err(e) = runtime_frame.finish_and_present() {
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
