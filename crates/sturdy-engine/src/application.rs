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
    AntiAliasingMode, AntiAliasingPass, AppRuntime, BloomConfig, BloomPass, DebugImageRegistry,
    DefaultSceneTargetConfig, DiagnosticLevel, Engine, GraphImage, KeyInput, KeyModifiers,
    MotionVectorDebugPass, NativeWindowAppearanceError, Result as EngineResult, RuntimeController,
    RuntimeDiagnostics, RuntimeGraphDiagnostics, RuntimeSettingChange, RuntimeSettingId,
    RuntimeSettingKey, RuntimeUserDiagnostic, ShaderProgram, Surface, SurfaceHdrPreference,
    SurfaceImage, SurfacePresentMode, SurfaceRecreateDesc, SurfaceTransparency, WindowAppearance,
    WindowAppearancePreset, WindowBackdrop, WindowCornerStyle, WindowMaterialKind, WindowMode,
    WindowTransparencyDesc, apply_native_window_appearance_for_window, current_platform,
};

#[cfg(target_os = "macos")]
use winit::platform::macos::WindowAttributesExtMacOS;
#[cfg(target_os = "windows")]
use winit::platform::windows::{BackdropType, WindowAttributesExtWindows, WindowExtWindows};

/// Configuration for the application shell window.
#[derive(Clone, Debug)]
pub struct WindowConfig {
    title: String,
    width: u32,
    height: u32,
    position: Option<(i32, i32)>,
    resizable: bool,
    decorations: bool,
    maximized: bool,
    always_on_top: bool,
    window_mode: WindowMode,
    prefer_hdr: bool,
    appearance: WindowAppearance,
}

impl WindowConfig {
    /// Create a new window configuration with the given title, width, and height.
    pub fn new(title: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            title: title.into(),
            width,
            height,
            position: None,
            resizable: false,
            decorations: true,
            maximized: false,
            always_on_top: false,
            window_mode: WindowMode::Windowed,
            prefer_hdr: false,
            appearance: WindowAppearance::default(),
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

    pub fn with_position(mut self, x: i32, y: i32) -> Self {
        self.position = Some((x, y));
        self
    }

    /// Set whether the window is resizable.
    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    pub fn with_decorations(mut self, decorations: bool) -> Self {
        self.decorations = decorations;
        self
    }

    pub fn with_borderless(mut self, borderless: bool) -> Self {
        self.decorations = !borderless;
        self
    }

    pub fn with_maximized(mut self, maximized: bool) -> Self {
        self.maximized = maximized;
        self
    }

    pub fn with_always_on_top(mut self, always_on_top: bool) -> Self {
        self.always_on_top = always_on_top;
        self
    }

    pub fn with_window_mode(mut self, mode: WindowMode) -> Self {
        self.window_mode = mode;
        self
    }

    pub fn with_borderless_fullscreen(mut self, enabled: bool) -> Self {
        self.window_mode = if enabled {
            WindowMode::BorderlessFullscreen
        } else {
            WindowMode::Windowed
        };
        self
    }

    /// Set whether HDR is preferred for the surface.
    pub fn with_hdr(mut self, prefer_hdr: bool) -> Self {
        self.prefer_hdr = prefer_hdr;
        self
    }

    pub fn with_window_appearance(mut self, appearance: WindowAppearance) -> Self {
        self.appearance = appearance;
        self
    }

    pub fn with_window_corner_style(mut self, corner_style: WindowCornerStyle) -> Self {
        self.appearance.corner_style = Some(corner_style);
        self
    }

    pub fn with_window_appearance_preset(mut self, preset: WindowAppearancePreset) -> Self {
        self.appearance = WindowAppearance::from_preset(preset);
        self
    }

    pub fn with_transparency(mut self, enabled: bool) -> Self {
        self.appearance.transparency = if enabled {
            SurfaceTransparency::Enabled
        } else {
            SurfaceTransparency::Disabled
        };
        if !enabled && !matches!(self.appearance.backdrop, WindowBackdrop::None) {
            self.appearance.backdrop = WindowBackdrop::None;
        }
        self
    }

    pub fn appearance(&self) -> WindowAppearance {
        self.appearance
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

    pub fn decorations(&self) -> bool {
        self.decorations
    }

    pub fn maximized(&self) -> bool {
        self.maximized
    }

    pub fn always_on_top(&self) -> bool {
        self.always_on_top
    }

    pub fn position(&self) -> Option<(i32, i32)> {
        self.position
    }

    pub fn window_mode(&self) -> WindowMode {
        self.window_mode
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

    /// Handle a structured key input event for keybind matching/rebinding.
    fn key_input(&mut self, _input: &KeyInput, _surface: &mut Surface) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Handle pointer (mouse/touch) movement.
    ///
    /// `pos` is in top-left/Y-down `WindowLogicalPx` (DPI-scaled window pixels).
    fn pointer_moved(
        &mut self,
        _pos: clay_ui::WindowLogicalPx,
        _surface: &mut Surface,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Handle a pointer button press or release.
    ///
    /// `pos` is in top-left/Y-down `WindowLogicalPx`. `button` follows the
    /// convention 0 = primary, 1 = secondary, 2 = middle.
    fn pointer_button(
        &mut self,
        _pos: clay_ui::WindowLogicalPx,
        _button: u8,
        _pressed: bool,
        _surface: &mut Surface,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Handle a scroll wheel or touchpad scroll.
    ///
    /// `pos` is the cursor position in `WindowLogicalPx`.
    /// `delta_x` and `delta_y` are in logical pixels, positive Y = down.
    fn pointer_scroll(
        &mut self,
        _pos: clay_ui::WindowLogicalPx,
        _delta_x: f32,
        _delta_y: f32,
        _surface: &mut Surface,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Apply runtime setting changes before rendering.
    fn runtime_settings_changed(
        &mut self,
        _controller: &RuntimeController,
        _changes: &[RuntimeSettingChange],
        _surface: &mut Surface,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Return a mutable reference to the app's [`InputHub`] to enable
    /// automatic input routing from the engine shell.
    ///
    /// When this returns `Some`, the engine shell calls the hub's `on_*`
    /// methods instead of the individual `key_pressed`, `pointer_moved`,
    /// `pointer_button`, and `pointer_scroll` callbacks — those can be left
    /// as the default no-ops.
    ///
    /// ```ignore
    /// struct MyApp { hub: InputHub }
    /// impl EngineApp for MyApp {
    ///     fn input_hub(&mut self) -> Option<&mut InputHub> { Some(&mut self.hub) }
    /// }
    /// ```
    fn input_hub(&mut self) -> Option<&mut crate::InputHub> {
        None
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
    motion_debug_pass: &'a MotionVectorDebugPass,
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
        motion_debug_pass: &'a MotionVectorDebugPass,
    ) -> Self {
        Self {
            inner,
            surface_image,
            default_scene_target,
            debug_images,
            controller,
            motion_debug_pass,
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

    /// Return the shared runtime controller for settings/diagnostics queries.
    pub fn runtime_controller(&self) -> RuntimeController {
        self.controller.clone()
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
                diagnostics.adapter_name.as_deref().unwrap_or("<unknown>")
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
            format!(
                "motion warning: {}",
                diagnostics.motion_warning.as_deref().unwrap_or("none")
            ),
            format!(
                "native window appearance: {}",
                diagnostics
                    .native_window_appearance
                    .as_deref()
                    .unwrap_or("unpublished")
            ),
            format!(
                "runtime setting apply: {}",
                diagnostics
                    .runtime_setting_apply
                    .as_deref()
                    .unwrap_or("unpublished")
            ),
            format!(
                "frame sync: {}",
                diagnostics.frame_sync.as_deref().unwrap_or("unpublished")
            ),
            format!(
                "user diagnostics: {}",
                if diagnostics.user_diagnostics.is_empty() {
                    "none".to_string()
                } else {
                    diagnostics
                        .user_diagnostics
                        .iter()
                        .map(|diagnostic| diagnostic.message.as_str())
                        .collect::<Vec<_>>()
                        .join(" | ")
                }
            ),
            format!(
                "camera-locked passes: {}",
                if diagnostics.camera_locked_passes.is_empty() {
                    "none".to_string()
                } else {
                    diagnostics.camera_locked_passes.join(", ")
                }
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

    /// Run a camera-locked/screen-locked pass after world temporal effects.
    ///
    /// Use this for reticles, HUD markers, or similar overlays that should be
    /// composed relative to the camera rather than inheriting world motion blur.
    pub fn run_camera_locked_pass(
        &self,
        name: impl Into<String>,
        target: &GraphImage,
        render: impl FnOnce(&crate::RenderFrame, &GraphImage) -> EngineResult<()>,
    ) -> EngineResult<()> {
        let name = name.into();
        render(&self.inner, target)?;
        self.controller.update_diagnostics(|current| {
            if !current
                .camera_locked_passes
                .iter()
                .any(|entry| entry == &name)
            {
                current.camera_locked_passes.push(name);
            }
        });
        Ok(())
    }

    /// Run the default HDR post chain from scene color through tonemap.
    ///
    /// When the `MotionDebugView` runtime setting is enabled and `motion_vectors`
    /// are supplied, the runtime-owned debug visualization pass runs automatically —
    /// no external program or target image needed.
    pub fn run_default_post_process<T: bytemuck::Pod>(
        &self,
        desc: RuntimePostProcessDesc<'_, T>,
    ) -> EngineResult<RuntimePostProcessOutput> {
        let hdr_composite =
            if let (Some(bloom_pass), Some(bloom_config)) = (desc.bloom_pass, desc.bloom_config) {
                bloom_pass.execute(desc.scene_color, &self.inner, bloom_config, desc.bloom_only)?
            } else {
                desc.scene_color.clone()
            };

        let (motion_source, motion_validation, motion_warning) =
            classify_motion_vectors(desc.motion_vectors);
        self.controller.update_diagnostics(|current| {
            current.motion_validation = Some(motion_validation);
            current.motion_warning = motion_warning;
        });

        let anti_aliased = desc.aa_pass.execute_with_motion_vectors(
            &self.inner,
            &hdr_composite,
            motion_source,
            desc.aa_mode,
        )?;

        let show_motion_debug = self
            .controller
            .bool_setting(RuntimeSettingKey::MotionDebugView)
            .unwrap_or(false);

        let final_input = if show_motion_debug {
            if let Some(mv_desc) = desc.motion_vectors {
                let ext = desc.swapchain.desc().extent;
                let debug_target = self.motion_debug_pass.execute(
                    &self.inner,
                    mv_desc.image,
                    ext.width,
                    ext.height,
                )?;
                self.register_debug_image("motion_source", mv_desc.image);
                self.register_debug_image("motion_debug_view", &debug_target);
                self.register_debug_image("hdr_composite", &debug_target);
                debug_target
            } else {
                self.register_debug_image("hdr_composite", &anti_aliased);
                anti_aliased.clone()
            }
        } else {
            self.register_debug_image("hdr_composite", &anti_aliased);
            anti_aliased.clone()
        };

        desc.swapchain
            .execute_shader_with_constants_auto(desc.tonemap_program, desc.tonemap_constants)?;

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
        let flush_report = self
            .inner
            .flush_with_reason(crate::FrameSyncReason::FrameBoundaryPresent)?;
        let wait_report = self
            .inner
            .wait_with_reason(crate::FrameSyncReason::FrameBoundaryPresent)?;
        surface.present()?;
        self.controller.update_diagnostics(|diagnostics| {
            diagnostics.frame_sync = Some(format!(
                "reason={:?} submitted={} waited={} presented=true submission={:?}",
                flush_report.reason,
                flush_report.submitted,
                wait_report.waited,
                flush_report.submission
            ));
        });
        Ok(())
    }
}

fn classify_motion_vectors(
    motion_vectors: Option<RuntimeMotionVectorDesc<'_>>,
) -> (Option<&GraphImage>, String, Option<String>) {
    match motion_vectors {
        Some(desc)
            if desc.space == MotionVectorSpace::CameraLocal
                && desc.layer == MotionVectorLayer::World =>
        {
            (
                Some(desc.image),
                "camera-local world motion".to_string(),
                None,
            )
        }
        Some(desc) if desc.layer == MotionVectorLayer::CameraLocked => (
            None,
            "camera-locked layer bypasses world temporal motion".to_string(),
            Some(
                "camera-locked content should be composed in a dedicated post-temporal pass"
                    .to_string(),
            ),
        ),
        Some(_) => (
            None,
            "non-camera-local motion vectors ignored by default temporal path".to_string(),
            Some("default temporal effects require camera-local world motion vectors".to_string()),
        ),
        None => (
            None,
            "no motion vectors supplied".to_string(),
            Some("temporal effects are running without explicit motion vectors".to_string()),
        ),
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
    if let Err(error) = try_run::<App>(config) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

/// Try to run the application with the given window configuration.
///
/// Unlike [`run`], this returns setup and event-loop errors to the caller
/// instead of panicking. Once the platform event loop starts, fatal runtime
/// errors are still reported through the shell and terminate the process.
#[cfg(not(target_arch = "wasm32"))]
pub fn try_run<App: EngineApp>(config: WindowConfig) -> std::result::Result<(), String>
where
    App::Error: std::fmt::Debug,
{
    use winit::{
        dpi::{LogicalPosition, LogicalSize},
        event_loop::{ControlFlow, EventLoop},
        window::{Window, WindowLevel},
    };

    // Create the engine
    let engine = Engine::new().map_err(|error| format!("failed to create engine: {error}"))?;

    // Create event loop
    let event_loop: EventLoop<()> =
        EventLoop::new().map_err(|error| format!("failed to create event loop: {error}"))?;
    event_loop.set_control_flow(ControlFlow::Poll);

    // Create window
    let mut attributes = Window::default_attributes()
        .with_title(&config.title)
        .with_inner_size(LogicalSize::new(config.width as f64, config.height as f64))
        .with_resizable(config.resizable)
        .with_decorations(config.decorations)
        .with_maximized(config.maximized)
        .with_window_level(if config.always_on_top {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        })
        .with_fullscreen(fullscreen_for_mode(config.window_mode));
    if let Some((x, y)) = config.position {
        attributes = attributes.with_position(LogicalPosition::new(x as f64, y as f64));
    }

    #[allow(deprecated)]
    let window = event_loop
        .create_window(apply_window_appearance_to_attributes(
            attributes,
            config.appearance,
        ))
        .map_err(|error| format!("failed to create window: {error}"))?;

    let native_appearance_result = apply_native_window_appearance_for_window(
        &window,
        Some((window.inner_size().width, window.inner_size().height)),
        config.appearance,
    );
    if let Err(err) = &native_appearance_result {
        if err.is_degraded() {
            eprintln!("native window appearance setup degraded: {err}");
        } else {
            eprintln!("native window appearance setup fell back to winit: {err}");
        }
    }

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
        .map_err(|error| format!("failed to create surface: {error}"))?;

    // Initialize the application
    let app_state = App::init(&engine, &surface)
        .map_err(|error| format!("failed to initialize application: {error:?}"))?;

    let mut runtime = AppRuntime::new(engine, surface);
    seed_window_settings(runtime.controller_mut(), &config)
        .map_err(|error| format!("failed to seed runtime window settings: {error}"))?;
    runtime.controller().update_diagnostics(|diagnostics| {
        diagnostics.native_window_appearance = Some(native_window_appearance_diagnostic(
            config.appearance,
            &native_appearance_result,
        ));
    });
    runtime.controller().set_settings(window_settings_snapshot(
        &window,
        &config,
        runtime.controller(),
    ));

    // Run event loop
    event_loop
        .run_app(&mut ShellApp {
            runtime,
            window: Some(window),
            app_state,
            modifiers: KeyModifiers::default(),
            applied_settings_revision: 0,
            started_at: Instant::now(),
            _config: config,
            cursor_pos: clay_ui::WindowLogicalPx::ZERO,
            primary_held: false,
        })
        .map_err(|error| format!("event loop exited unexpectedly: {error}"))
}

// Internal winit ApplicationHandler implementation
#[cfg(not(target_arch = "wasm32"))]
struct ShellApp<App: EngineApp> {
    runtime: AppRuntime,
    window: Option<winit::window::Window>,
    app_state: App,
    modifiers: KeyModifiers,
    applied_settings_revision: u64,
    #[allow(dead_code)]
    started_at: Instant,
    _config: WindowConfig,
    cursor_pos: clay_ui::WindowLogicalPx,
    primary_held: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl<App: EngineApp> winit::application::ApplicationHandler for ShellApp<App>
where
    App::Error: std::fmt::Debug,
{
    fn new_events(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        _cause: winit::event::StartCause,
    ) {
        if let Err(e) = self.apply_pending_runtime_settings() {
            eprintln!("runtime settings apply failed: {e:?}");
            std::process::exit(1);
        }
    }

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
                        let snapshot = window_settings_snapshot(
                            window,
                            &self._config,
                            self.runtime.controller(),
                        );
                        self.runtime.controller().set_settings(snapshot);
                        apply_window_appearance_from_settings(window, self.runtime.controller());
                        window.request_redraw();
                    }
                }
            }
            winit::event::WindowEvent::Moved(position) => {
                if let Some(window) = self.window.as_ref() {
                    let mut snapshot =
                        window_settings_snapshot(window, &self._config, self.runtime.controller());
                    snapshot.window_position = Some((position.x, position.y));
                    self.runtime.controller().set_settings(snapshot);
                }
            }
            winit::event::WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = crate::input::key_modifiers_from_winit(modifiers.state());
            }
            winit::event::WindowEvent::KeyboardInput { event, .. } => {
                use winit::event::ElementState;
                use winit::keyboard::Key;
                if let Some(input) = crate::input::KeyInput::from_winit(&event, self.modifiers) {
                    if let Some(hub) = self.app_state.input_hub() {
                        hub.on_key_input(&input);
                    } else {
                        if let Err(e) = self.app_state.key_input(&input, self.runtime.surface_mut())
                        {
                            eprintln!("key input handler failed: {e:?}");
                            std::process::exit(1);
                        }
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
                }
            }
            winit::event::WindowEvent::CursorMoved { position, .. } => {
                // winit CursorMoved delivers PhysicalPosition. Convert to
                // WindowLogicalPx (top-left/Y-down) at the platform boundary.
                let scale = self
                    .window
                    .as_ref()
                    .map(|w| w.scale_factor() as f32)
                    .unwrap_or(1.0);
                let pos = clay_ui::WindowLogicalPx::new(
                    position.x as f32 / scale,
                    position.y as f32 / scale,
                );
                self.cursor_pos = pos;
                if let Some(hub) = self.app_state.input_hub() {
                    hub.on_pointer_moved(pos);
                } else if let Err(e) = self
                    .app_state
                    .pointer_moved(pos, self.runtime.surface_mut())
                {
                    eprintln!("pointer_moved failed: {e:?}");
                    std::process::exit(1);
                }
            }
            winit::event::WindowEvent::MouseInput { state, button, .. } => {
                use winit::event::{ElementState, MouseButton};
                let btn: u8 = match button {
                    MouseButton::Left => 0,
                    MouseButton::Right => 1,
                    MouseButton::Middle => 2,
                    _ => 3,
                };
                let pressed = state == ElementState::Pressed;
                if btn == 0 {
                    self.primary_held = pressed;
                }
                let pos = self.cursor_pos;
                if let Some(hub) = self.app_state.input_hub() {
                    hub.on_pointer_button(pos, btn, pressed);
                } else if let Err(e) =
                    self.app_state
                        .pointer_button(pos, btn, pressed, self.runtime.surface_mut())
                {
                    eprintln!("pointer_button failed: {e:?}");
                    std::process::exit(1);
                }
            }
            winit::event::WindowEvent::MouseWheel { delta, .. } => {
                use winit::event::MouseScrollDelta;
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x * 20.0, -y * 20.0),
                    MouseScrollDelta::PixelDelta(pos) => (pos.x as f32, pos.y as f32),
                };
                let pos = self.cursor_pos;
                if let Some(hub) = self.app_state.input_hub() {
                    hub.on_pointer_scroll(dx, dy);
                } else if let Err(e) =
                    self.app_state
                        .pointer_scroll(pos, dx, dy, self.runtime.surface_mut())
                {
                    eprintln!("pointer_scroll failed: {e:?}");
                    std::process::exit(1);
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

#[cfg(not(target_arch = "wasm32"))]
impl<App: EngineApp> ShellApp<App>
where
    App::Error: std::fmt::Debug,
{
    fn apply_pending_runtime_settings(&mut self) -> Result<(), App::Error> {
        let controller = self.runtime.controller().clone();
        let changes = controller.setting_changes_since(self.applied_settings_revision);
        if changes.is_empty() {
            self.applied_settings_revision = controller.settings_revision();
            return Ok(());
        }

        self.apply_engine_runtime_settings(&controller, &changes);
        self.app_state.runtime_settings_changed(
            &controller,
            &changes,
            self.runtime.surface_mut(),
        )?;
        self.applied_settings_revision = controller.settings_revision();
        Ok(())
    }

    fn apply_engine_runtime_settings(
        &mut self,
        controller: &RuntimeController,
        changes: &[RuntimeSettingChange],
    ) {
        if let Some(window) = self.window.as_ref() {
            apply_window_runtime_settings(window, controller, changes);
            let snapshot =
                window_settings_snapshot(window, &self._config, self.runtime.controller());
            self.runtime.controller().set_settings(snapshot);
        }

        let affects_native_window = changes.iter().any(|change| {
            matches!(
                change.setting,
                RuntimeSettingId::Engine(RuntimeSettingKey::WindowCornerStyle)
                    | RuntimeSettingId::Engine(RuntimeSettingKey::SurfaceTransparency)
                    | RuntimeSettingId::Engine(RuntimeSettingKey::WindowBackgroundEffect)
            )
        });
        if affects_native_window {
            if let Some(window) = self.window.as_ref() {
                apply_window_appearance_from_settings(window, controller);
                let snapshot =
                    window_settings_snapshot(window, &self._config, self.runtime.controller());
                self.runtime.controller().set_settings(snapshot);
            }
        }

        if !changes.iter().any(|change| {
            matches!(
                change.setting,
                RuntimeSettingId::Engine(RuntimeSettingKey::HdrMode)
                    | RuntimeSettingId::Engine(RuntimeSettingKey::PresentMode)
                    | RuntimeSettingId::Engine(RuntimeSettingKey::SurfaceTransparency)
            )
        }) {
            return;
        }

        let hdr_preference = if controller
            .bool_setting(RuntimeSettingKey::HdrMode)
            .unwrap_or(false)
        {
            match self.runtime.surface().hdr_caps() {
                Ok(caps) if caps.sc_rgb => Some(SurfaceHdrPreference::ScRgb),
                Ok(caps) if caps.hdr10 => Some(SurfaceHdrPreference::Hdr10),
                _ => Some(SurfaceHdrPreference::Sdr),
            }
        } else {
            Some(SurfaceHdrPreference::Sdr)
        };

        let preferred_present_mode = controller
            .text_setting(RuntimeSettingKey::PresentMode)
            .and_then(|value| parse_present_mode_setting(&value));
        let transparent = controller.bool_setting(RuntimeSettingKey::SurfaceTransparency);

        let surface_size = self.runtime.surface().size();
        if let Err(e) = self.runtime.surface_mut().recreate(SurfaceRecreateDesc {
            size: Some(surface_size),
            transparent,
            hdr: hdr_preference,
            preferred_present_mode,
            ..SurfaceRecreateDesc::default()
        }) {
            let context =
                runtime_setting_apply_context(controller, changes, surface_size, "failed");
            self.runtime.controller().update_diagnostics(|diagnostics| {
                diagnostics.runtime_setting_apply = Some(format!(
                    "{context} error_category={:?} reason={e}",
                    e.category()
                ));
                diagnostics.user_diagnostics.push(RuntimeUserDiagnostic {
                    message: "Runtime setting changes could not be applied to the surface."
                        .to_string(),
                    detail: Some(format!(
                        "{context} error_category={:?} reason={e}",
                        e.category()
                    )),
                    setting: None,
                });
            });
            eprintln!(
                "surface recreation from runtime settings failed: {context} error_category={:?} reason={e}",
                e.category()
            );
            std::process::exit(1);
        }
        let context = runtime_setting_apply_context(controller, changes, surface_size, "applied");
        self.runtime.controller().update_diagnostics(|diagnostics| {
            diagnostics.runtime_setting_apply = Some(context);
        });
        self.runtime.refresh_controller_state();
        if let Some(window) = self.window.as_ref() {
            let snapshot =
                window_settings_snapshot(window, &self._config, self.runtime.controller());
            self.runtime.controller().set_settings(snapshot);
        }
    }
}

fn runtime_setting_apply_context(
    controller: &RuntimeController,
    changes: &[RuntimeSettingChange],
    surface_size: SurfaceSize,
    status: &str,
) -> String {
    let diagnostics = controller.diagnostics();
    let settings = changes
        .iter()
        .map(|change| format!("{}@{}#{}", change.setting, change.path, change.revision))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "status={status} platform={:?} backend={:?} adapter={} surface={}x{} settings=[{}]",
        current_platform(),
        diagnostics.backend,
        diagnostics.adapter_name.as_deref().unwrap_or("<unknown>"),
        surface_size.width,
        surface_size.height,
        settings,
    )
}

fn parse_present_mode_setting(value: &str) -> Option<SurfacePresentMode> {
    match value {
        "Auto" => None,
        "Fifo" => Some(SurfacePresentMode::Fifo),
        "Mailbox" => Some(SurfacePresentMode::Mailbox),
        "Immediate" => Some(SurfacePresentMode::Immediate),
        "RelaxedFifo" => Some(SurfacePresentMode::RelaxedFifo),
        _ => None,
    }
}

fn apply_window_appearance_to_attributes(
    mut attributes: winit::window::WindowAttributes,
    appearance: WindowAppearance,
) -> winit::window::WindowAttributes {
    let wants_transparent = appearance.transparency == SurfaceTransparency::Enabled;
    attributes = attributes.with_transparent(wants_transparent);

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        attributes = attributes.with_blur(matches!(
            appearance.backdrop,
            WindowBackdrop::Blurred(_) | WindowBackdrop::Material(_)
        ));
    }

    #[cfg(target_os = "windows")]
    {
        attributes = attributes.with_system_backdrop(windows_backdrop_for_appearance(appearance));
    }

    #[cfg(target_os = "macos")]
    {
        let titlebar_transparent = matches!(
            appearance.backdrop,
            WindowBackdrop::Material(material)
                if material.kind == WindowMaterialKind::TitlebarTranslucent
        );
        attributes = attributes.with_titlebar_transparent(titlebar_transparent);
    }

    attributes
}

fn seed_window_settings(
    controller: &mut RuntimeController,
    config: &WindowConfig,
) -> Result<(), crate::Error> {
    let (transparent, effect) = window_effect_setting_from_appearance(config.appearance);
    controller
        .transact()
        .set_engine_value(RuntimeSettingKey::WindowTitle, config.title.clone())
        .set_engine_value(RuntimeSettingKey::WindowWidth, config.width as i64)
        .set_engine_value(RuntimeSettingKey::WindowHeight, config.height as i64)
        .set_engine_value(
            RuntimeSettingKey::WindowPositionX,
            config.position.map(|(x, _)| x as i64).unwrap_or(0),
        )
        .set_engine_value(
            RuntimeSettingKey::WindowPositionY,
            config.position.map(|(_, y)| y as i64).unwrap_or(0),
        )
        .set_engine_value(
            RuntimeSettingKey::WindowMode,
            window_mode_setting(config.window_mode),
        )
        .set_engine_value(RuntimeSettingKey::WindowDecorations, config.decorations)
        .set_engine_value(RuntimeSettingKey::WindowResizable, config.resizable)
        .set_engine_value(RuntimeSettingKey::WindowMaximized, config.maximized)
        .set_engine_value(RuntimeSettingKey::WindowAlwaysOnTop, config.always_on_top)
        .set_engine_value(
            RuntimeSettingKey::WindowCornerStyle,
            corner_style_setting(
                config
                    .appearance
                    .corner_style
                    .unwrap_or(WindowCornerStyle::Default),
            ),
        )
        .set_engine_value(RuntimeSettingKey::SurfaceTransparency, transparent)
        .set_engine_value(RuntimeSettingKey::WindowBackgroundEffect, effect)
        .apply()?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn apply_window_appearance_from_settings(
    window: &winit::window::Window,
    controller: &RuntimeController,
) {
    let appearance = window_appearance_from_settings(controller);
    let wants_transparency = appearance.transparency == SurfaceTransparency::Enabled;

    window.set_transparent(wants_transparency);

    let native_result = apply_native_window_appearance_for_window(
        window,
        Some((window.inner_size().width, window.inner_size().height)),
        appearance,
    );
    if let Err(err) = &native_result {
        if err.is_degraded() {
            eprintln!("native window appearance apply degraded: {err}");
        } else {
            eprintln!("native window appearance apply fell back to winit: {err}");
        }

        #[cfg(target_os = "windows")]
        {
            window.set_system_backdrop(windows_backdrop_for_appearance(appearance));
        }

        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            window.set_blur(matches!(
                appearance.backdrop,
                WindowBackdrop::Blurred(_) | WindowBackdrop::Material(_)
            ));
        }
    }

    controller.update_diagnostics(|diagnostics| {
        diagnostics.native_window_appearance = Some(native_window_appearance_diagnostic(
            appearance,
            &native_result,
        ));
    });
}

fn native_window_appearance_diagnostic(
    appearance: WindowAppearance,
    result: &std::result::Result<(), NativeWindowAppearanceError>,
) -> String {
    let requested = match appearance.backdrop {
        WindowBackdrop::None => "none",
        WindowBackdrop::Transparent(_) => "transparent",
        WindowBackdrop::Blurred(_) => "blur",
        WindowBackdrop::Material(_) => "material",
    };
    let protocol = native_window_appearance_protocol(appearance);
    match result {
        Ok(()) => format!("protocol={protocol} requested={requested} status=applied"),
        Err(error) if error.is_degraded() => {
            format!(
                "protocol={protocol} requested={requested} status=degraded fallback=winit reason={error}"
            )
        }
        Err(error) => {
            format!(
                "protocol={protocol} requested={requested} status=failed fallback=winit reason={error}"
            )
        }
    }
}

fn native_window_appearance_protocol(appearance: WindowAppearance) -> &'static str {
    let wants_backdrop = matches!(
        appearance.backdrop,
        WindowBackdrop::Blurred(_) | WindowBackdrop::Material(_)
    );
    if !wants_backdrop {
        return "none";
    }

    #[cfg(target_os = "linux")]
    {
        "wayland/ext-background-effect-v1"
    }
    #[cfg(target_os = "windows")]
    {
        "windows/system-backdrop"
    }
    #[cfg(target_os = "macos")]
    {
        "macos/native-visual-effect"
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        "unsupported"
    }
}

fn window_appearance_from_settings(controller: &RuntimeController) -> WindowAppearance {
    let effect = controller
        .text_setting(RuntimeSettingKey::WindowBackgroundEffect)
        .unwrap_or_else(|| "None".to_string());
    let transparent = controller
        .bool_setting(RuntimeSettingKey::SurfaceTransparency)
        .unwrap_or(false);
    let appearance = match effect.as_str() {
        "Auto" => WindowAppearance::material(WindowMaterialKind::Auto),
        "Transparent" => WindowAppearance {
            transparency: SurfaceTransparency::Enabled,
            backdrop: WindowBackdrop::Transparent(WindowTransparencyDesc::default()),
            ..WindowAppearance::default()
        },
        "Blur" => WindowAppearance::from_preset(WindowAppearancePreset::Blur),
        "ThinTranslucent" => WindowAppearance::from_preset(WindowAppearancePreset::ThinMaterial),
        "ThickTranslucent" => WindowAppearance::from_preset(WindowAppearancePreset::ThickMaterial),
        "NoiseTranslucent" => WindowAppearance::from_preset(WindowAppearancePreset::NoiseMaterial),
        "TitlebarTranslucent" => {
            WindowAppearance::from_preset(WindowAppearancePreset::TitlebarMaterial)
        }
        "Hud" => WindowAppearance::from_preset(WindowAppearancePreset::HudMaterial),
        _ => WindowAppearance::default(),
    };
    if transparent || !matches!(appearance.backdrop, WindowBackdrop::None) {
        let mut appearance = WindowAppearance {
            transparency: SurfaceTransparency::Enabled,
            ..appearance
        };
        appearance.corner_style = Some(corner_style_from_settings(controller));
        appearance
    } else {
        let mut appearance = appearance;
        appearance.corner_style = Some(corner_style_from_settings(controller));
        appearance
    }
}

fn window_effect_setting_from_appearance(appearance: WindowAppearance) -> (bool, String) {
    let effect = match appearance.backdrop {
        WindowBackdrop::None => "None",
        WindowBackdrop::Transparent(_) => "Transparent",
        WindowBackdrop::Blurred(_) => "Blur",
        WindowBackdrop::Material(material) => match material.kind {
            WindowMaterialKind::Auto => "Auto",
            WindowMaterialKind::ThinTranslucent => "ThinTranslucent",
            WindowMaterialKind::ThickTranslucent => "ThickTranslucent",
            WindowMaterialKind::NoiseTranslucent => "NoiseTranslucent",
            WindowMaterialKind::TitlebarTranslucent => "TitlebarTranslucent",
            WindowMaterialKind::Hud => "Hud",
        },
    };
    (
        appearance.transparency == SurfaceTransparency::Enabled,
        effect.to_string(),
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn apply_window_runtime_settings(
    window: &winit::window::Window,
    controller: &RuntimeController,
    changes: &[RuntimeSettingChange],
) {
    use winit::{
        dpi::{PhysicalPosition, PhysicalSize},
        window::WindowLevel,
    };

    let mut resize_requested = false;
    let mut position_requested = false;

    for change in changes {
        match change.setting {
            RuntimeSettingId::Engine(RuntimeSettingKey::WindowTitle) => {
                if let Some(title) = controller.text_setting(RuntimeSettingKey::WindowTitle) {
                    window.set_title(&title);
                }
            }
            RuntimeSettingId::Engine(RuntimeSettingKey::WindowWidth)
            | RuntimeSettingId::Engine(RuntimeSettingKey::WindowHeight) => {
                resize_requested = true;
            }
            RuntimeSettingId::Engine(RuntimeSettingKey::WindowPositionX)
            | RuntimeSettingId::Engine(RuntimeSettingKey::WindowPositionY) => {
                position_requested = true;
            }
            RuntimeSettingId::Engine(RuntimeSettingKey::WindowMode) => {
                let mode = controller
                    .text_setting(RuntimeSettingKey::WindowMode)
                    .as_deref()
                    .map(window_mode_from_setting)
                    .unwrap_or(WindowMode::Windowed);
                window.set_fullscreen(fullscreen_for_mode(mode));
            }
            RuntimeSettingId::Engine(RuntimeSettingKey::WindowDecorations) => {
                if let Some(value) = controller.bool_setting(RuntimeSettingKey::WindowDecorations) {
                    window.set_decorations(value);
                }
            }
            RuntimeSettingId::Engine(RuntimeSettingKey::WindowResizable) => {
                if let Some(value) = controller.bool_setting(RuntimeSettingKey::WindowResizable) {
                    window.set_resizable(value);
                }
            }
            RuntimeSettingId::Engine(RuntimeSettingKey::WindowMaximized) => {
                if let Some(value) = controller.bool_setting(RuntimeSettingKey::WindowMaximized) {
                    window.set_maximized(value);
                }
            }
            RuntimeSettingId::Engine(RuntimeSettingKey::WindowAlwaysOnTop) => {
                if let Some(value) = controller.bool_setting(RuntimeSettingKey::WindowAlwaysOnTop) {
                    window.set_window_level(if value {
                        WindowLevel::AlwaysOnTop
                    } else {
                        WindowLevel::Normal
                    });
                }
            }
            _ => {}
        }
    }

    if resize_requested {
        let width = controller
            .integer_setting(RuntimeSettingKey::WindowWidth)
            .unwrap_or(window.inner_size().width as i64)
            .max(1) as u32;
        let height = controller
            .integer_setting(RuntimeSettingKey::WindowHeight)
            .unwrap_or(window.inner_size().height as i64)
            .max(1) as u32;
        let _ = window.request_inner_size(PhysicalSize::new(width, height));
    }

    if position_requested {
        let x = controller
            .integer_setting(RuntimeSettingKey::WindowPositionX)
            .unwrap_or(0) as i32;
        let y = controller
            .integer_setting(RuntimeSettingKey::WindowPositionY)
            .unwrap_or(0) as i32;
        window.set_outer_position(PhysicalPosition::new(x, y));
    }
}

fn window_mode_setting(mode: WindowMode) -> &'static str {
    match mode {
        WindowMode::Windowed => "Windowed",
        WindowMode::BorderlessFullscreen => "BorderlessFullscreen",
    }
}

fn window_mode_from_setting(value: &str) -> WindowMode {
    match value {
        "BorderlessFullscreen" => WindowMode::BorderlessFullscreen,
        _ => WindowMode::Windowed,
    }
}

fn window_mode_from_fullscreen(fullscreen: bool) -> WindowMode {
    if fullscreen {
        WindowMode::BorderlessFullscreen
    } else {
        WindowMode::Windowed
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn fullscreen_for_mode(mode: WindowMode) -> Option<winit::window::Fullscreen> {
    match mode {
        WindowMode::Windowed => None,
        WindowMode::BorderlessFullscreen => Some(winit::window::Fullscreen::Borderless(None)),
    }
}

fn corner_style_setting(style: WindowCornerStyle) -> &'static str {
    match style {
        WindowCornerStyle::Default => "Default",
        WindowCornerStyle::Rounded => "Rounded",
        WindowCornerStyle::Square => "Square",
    }
}

fn corner_style_from_settings(controller: &RuntimeController) -> WindowCornerStyle {
    match controller
        .text_setting(RuntimeSettingKey::WindowCornerStyle)
        .as_deref()
    {
        Some("Rounded") => WindowCornerStyle::Rounded,
        Some("Square") => WindowCornerStyle::Square,
        _ => WindowCornerStyle::Default,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn window_settings_snapshot(
    window: &winit::window::Window,
    config: &WindowConfig,
    controller: &RuntimeController,
) -> crate::RuntimeSettingsSnapshot {
    let mut settings = controller.settings();
    settings.window_title = controller
        .text_setting(RuntimeSettingKey::WindowTitle)
        .unwrap_or_else(|| config.title.clone());
    settings.window_size = SurfaceSize {
        width: window.inner_size().width.max(1),
        height: window.inner_size().height.max(1),
    };
    settings.window_position = window
        .outer_position()
        .ok()
        .map(|position| (position.x, position.y));
    settings.window_mode = window_mode_from_fullscreen(window.fullscreen().is_some());
    settings.window_decorations = controller
        .bool_setting(RuntimeSettingKey::WindowDecorations)
        .unwrap_or(config.decorations);
    settings.window_resizable = controller
        .bool_setting(RuntimeSettingKey::WindowResizable)
        .unwrap_or(config.resizable);
    settings.window_maximized = controller
        .bool_setting(RuntimeSettingKey::WindowMaximized)
        .unwrap_or(config.maximized);
    settings.window_always_on_top = controller
        .bool_setting(RuntimeSettingKey::WindowAlwaysOnTop)
        .unwrap_or(config.always_on_top);
    settings.window_corner_style = corner_style_from_settings(controller);
    settings
}

#[cfg(target_os = "windows")]
fn windows_backdrop_for_appearance(appearance: WindowAppearance) -> BackdropType {
    match appearance.backdrop {
        WindowBackdrop::Material(material) => match material.kind {
            WindowMaterialKind::Auto => BackdropType::Auto,
            WindowMaterialKind::ThinTranslucent => BackdropType::MainWindow,
            WindowMaterialKind::NoiseTranslucent => BackdropType::TransientWindow,
            WindowMaterialKind::TitlebarTranslucent => BackdropType::TabbedWindow,
            WindowMaterialKind::ThickTranslucent | WindowMaterialKind::Hud => {
                BackdropType::TransientWindow
            }
        },
        _ => BackdropType::None,
    }
}
