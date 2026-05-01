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

use std::{
    collections::{HashMap, VecDeque},
    time::Instant,
};

use sturdy_engine_core::SurfaceSize;

use crate::{
    AntiAliasingMode, AntiAliasingPass, AppRuntime, BloomConfig, BloomPass, DebugImageRegistry,
    DefaultSceneTargetConfig, DiagnosticLevel, Engine, FrameTime, GamepadAxis, GamepadButton,
    GamepadId, GraphImage, InputHub, KeyInput, KeyModifiers, MotionVectorDebugPass,
    NativeWindowAppearanceApplyReport, NativeWindowAppearanceStatus, Result as EngineResult,
    RuntimeApplyPath, RuntimeApplyReport, RuntimeChangeResult, RuntimeController,
    RuntimeDiagnostics, RuntimeGraphDiagnostics, RuntimeSettingChange, RuntimeSettingId,
    RuntimeSettingKey, RuntimeWindowDiagnostics, ScreenshotCapture, ScreenshotExportReport,
    ShaderProgram, Surface, SurfaceHdrPreference, SurfaceImage, SurfaceTransparency,
    WindowAppearance, WindowAppearancePreset, WindowBackdrop, WindowCornerStyle, WindowHandle,
    WindowMaterialKind, WindowMode, WindowRegistry, WindowTransparencyDesc,
    appearance_wants_native_blur, apply_native_window_appearance_report_for_window,
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

#[derive(Clone, Debug, PartialEq)]
pub struct WindowDesc {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub position: Option<(i32, i32)>,
    pub resizable: bool,
    pub decorations: bool,
    pub maximized: bool,
    pub always_on_top: bool,
    pub window_mode: WindowMode,
    pub prefer_hdr: bool,
    pub appearance: WindowAppearance,
}

impl WindowDesc {
    pub fn from_config(config: &WindowConfig) -> Self {
        Self {
            title: config.title.clone(),
            width: config.width,
            height: config.height,
            position: config.position,
            resizable: config.resizable,
            decorations: config.decorations,
            maximized: config.maximized,
            always_on_top: config.always_on_top,
            window_mode: config.window_mode,
            prefer_hdr: config.prefer_hdr,
            appearance: config.appearance,
        }
    }
}

impl From<&WindowConfig> for WindowDesc {
    fn from(config: &WindowConfig) -> Self {
        Self::from_config(config)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum ShellEventLoopCommand {
    CreateWindow(WindowDesc),
}

#[derive(Default)]
struct ShellEventLoopCommandQueue {
    pending: VecDeque<ShellEventLoopCommand>,
}

impl ShellEventLoopCommandQueue {
    fn new() -> Self {
        Self::default()
    }

    fn create_window(&mut self, desc: WindowDesc) {
        self.pending
            .push_back(ShellEventLoopCommand::CreateWindow(desc));
    }

    fn pop_front(&mut self) -> Option<ShellEventLoopCommand> {
        self.pending.pop_front()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.pending.len()
    }
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
    default_scene_target: DefaultSceneTargetConfig,
    debug_images: DebugImageRegistry,
    controller: RuntimeController,
    motion_debug_pass: &'a MotionVectorDebugPass,
    frame_time: FrameTime,
    window_scale_factor: f32,
    window_logical_size: Option<[f32; 2]>,
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
        default_scene_target: DefaultSceneTargetConfig,
        debug_images: DebugImageRegistry,
        controller: RuntimeController,
        motion_debug_pass: &'a MotionVectorDebugPass,
        frame_time: FrameTime,
    ) -> Self {
        Self {
            inner,
            default_scene_target,
            debug_images,
            controller,
            motion_debug_pass,
            frame_time,
            window_scale_factor: 1.0,
            window_logical_size: None,
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

    /// Timing for the runtime frame that produced this shell frame.
    pub fn frame_time(&self) -> FrameTime {
        self.frame_time
    }

    /// Delta time since the previous frame in seconds.
    pub fn delta_secs(&self) -> f32 {
        self.frame_time.delta_secs()
    }

    /// Total elapsed runtime in seconds.
    pub fn elapsed_secs(&self) -> f32 {
        self.frame_time.elapsed_secs()
    }

    /// Monotonic frame index for this shell frame.
    pub fn frame_index(&self) -> u64 {
        self.frame_time.frame
    }

    /// DPI scale factor for converting logical window/UI pixels to physical surface pixels.
    pub fn window_scale_factor(&self) -> f32 {
        self.window_scale_factor
    }

    /// Current drawable window size in logical window/UI pixels, when known.
    pub fn window_logical_size(&self) -> Option<[f32; 2]> {
        self.window_logical_size
    }

    pub(crate) fn set_window_scale_factor(&mut self, scale_factor: f32) {
        self.window_scale_factor = scale_factor.max(f32::EPSILON);
    }

    pub(crate) fn set_window_logical_size(&mut self, size: [f32; 2]) {
        self.window_logical_size = Some([size[0].max(1.0), size[1].max(1.0)]);
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
                "windows: live={} focused={} hovered={} dirty={} surface-wait={}",
                diagnostics.windows.live_count,
                diagnostics
                    .windows
                    .focused_window
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "none".to_string()),
                diagnostics
                    .windows
                    .hovered_window
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "none".to_string()),
                diagnostics.windows.dirty_count,
                diagnostics.windows.waiting_for_surface_recreation_count,
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

    /// Return compact render-graph inspection lines for the frame recorded so far.
    pub fn runtime_graph_inspection_lines(
        &self,
        max_passes: usize,
        max_images: usize,
    ) -> Vec<String> {
        RuntimeController::graph_inspection_lines(&self.inner.describe(), max_passes, max_images)
    }

    /// Save a named graph image from this frame as a PNG.
    ///
    /// This is an explicit blocking screenshot/readback helper. It submits and
    /// waits for the current render graph with `FrameSyncReason::ReadbackCompletion`.
    pub fn save_named_graph_image_png(
        &self,
        name: &str,
        path: impl AsRef<std::path::Path>,
    ) -> EngineResult<ScreenshotExportReport> {
        let image = self
            .inner
            .find_image_by_name(name)
            .ok_or_else(|| crate::Error::InvalidInput(format!("graph image `{name}` not found")))?;
        ScreenshotCapture::capture_render_frame_png(&self.inner.engine(), &self.inner, &image, path)
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
    /// Flushes queued passes and presents to the display. The CPU does not wait
    /// for GPU completion — synchronisation is handled by the GPU's render-complete
    /// semaphore. The frames-in-flight fence is waited at the start of the next
    /// frame's submission, enabling CPU/GPU overlap.
    pub fn finish_and_present(&mut self, surface: &Surface) -> EngineResult<()> {
        let flush_report = self
            .inner
            .flush_with_reason(crate::FrameSyncReason::FrameBoundaryPresent)?;
        surface.present()?;
        self.inner.mark_presented();
        self.controller.update_diagnostics(|diagnostics| {
            diagnostics.frame_sync = Some(format!(
                "reason={:?} submitted={} waited=false presented=true submission={:?}",
                flush_report.reason, flush_report.submitted, flush_report.submission
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
    use winit::event_loop::{ControlFlow, EventLoop};

    // Create the engine
    let engine = Engine::new().map_err(|error| format!("failed to create engine: {error}"))?;

    // Create event loop
    let event_loop: EventLoop<()> =
        EventLoop::new().map_err(|error| format!("failed to create event loop: {error}"))?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let primary_desc = WindowDesc::from_config(&config);
    let mut shell_commands = ShellEventLoopCommandQueue::new();
    shell_commands.create_window(primary_desc);
    let created_window = drain_initial_window_commands(&event_loop, &mut shell_commands)?
        .into_iter()
        .next()
        .ok_or_else(|| "primary window command did not create a window".to_string())?;
    let created_desc = created_window.desc.clone();
    let window = created_window.window;
    let native_appearance_report = created_window.native_appearance_report;

    if native_appearance_report.is_degraded() {
        eprintln!(
            "native window appearance setup degraded: {}",
            native_appearance_report.diagnostic_string()
        );
    } else if native_appearance_report.is_failed() {
        eprintln!(
            "native window appearance setup fell back to winit: {}",
            native_appearance_report.diagnostic_string()
        );
    }

    // Create surface from window
    let surface = engine
        .create_surface_for_window_with_hdr(
            &window,
            SurfaceSize {
                width: created_desc.width.max(1),
                height: created_desc.height.max(1),
            },
            if created_desc.prefer_hdr {
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
        diagnostics.native_window_appearance = Some(native_appearance_report.diagnostic_string());
    });
    runtime.controller().set_settings(window_settings_snapshot(
        &window,
        &config,
        runtime.controller(),
    ));

    let mut windows = WindowRegistry::new();
    let primary_window = windows.insert(ShellWindow::new(
        window,
        config.appearance,
        native_appearance_report.clone(),
    ));
    let mut winit_windows = HashMap::new();
    if let Some(window) = windows.get(primary_window) {
        winit_windows.insert(window.window().id(), primary_window);
    }

    // Run event loop
    event_loop
        .run_app(&mut ShellApp {
            runtime,
            windows,
            winit_windows,
            primary_window,
            app_state,
            gamepad_backend: GamepadBackend::new(),
            modifiers: KeyModifiers::default(),
            applied_settings_revision: 0,
            started_at: Instant::now(),
            _config: config,
        })
        .map_err(|error| format!("event loop exited unexpectedly: {error}"))
}

// Internal winit ApplicationHandler implementation
#[cfg(not(target_arch = "wasm32"))]
struct ShellApp<App: EngineApp> {
    runtime: AppRuntime,
    windows: WindowRegistry<ShellWindow>,
    winit_windows: HashMap<winit::window::WindowId, WindowHandle>,
    primary_window: WindowHandle,
    app_state: App,
    gamepad_backend: GamepadBackend,
    modifiers: KeyModifiers,
    applied_settings_revision: u64,
    #[allow(dead_code)]
    started_at: Instant,
    _config: WindowConfig,
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
        self.poll_gamepads();
        self.publish_window_diagnostics();
        if let Err(e) = self.apply_pending_runtime_settings() {
            eprintln!("runtime settings apply failed: {e:?}");
            std::process::exit(1);
        }
    }

    fn resumed(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        // Window already created
        self.request_redraw_for_window(self.primary_window);
    }

    fn window_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        let Some(window) = self.window_handle_for_winit_id(window_id) else {
            return;
        };
        self.dispatch_window_event(ShellWindowEvent { window, event });
    }

    fn device_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        _device_id: winit::event::DeviceId,
        event: winit::event::DeviceEvent,
    ) {
        if let winit::event::DeviceEvent::MouseMotion { delta: (dx, dy) } = event {
            if let Some(hub) = self.app_state.input_hub() {
                hub.on_raw_mouse_motion(glam::Vec2::new(dx as f32, dy as f32));
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        self.poll_gamepads();
        self.apply_pointer_lock();
        self.request_redraw_for_window(self.primary_window);
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<App: EngineApp> ShellApp<App>
where
    App::Error: std::fmt::Debug,
{
    fn poll_gamepads(&mut self) {
        if let Some(hub) = self.app_state.input_hub() {
            self.gamepad_backend.poll(hub);
        }
    }

    /// Apply or release the OS cursor grab based on what the app's InputHub requested.
    ///
    /// Tries `CursorGrabMode::Locked` first (supported on most desktops) and
    /// falls back to `CursorGrabMode::Confined` (constrains to window without
    /// hiding). Updates the hub's `pointer_locked` state to reflect whether the
    /// grab succeeded.
    fn apply_pointer_lock(&mut self) {
        // Read desired vs actual lock state from the hub (borrow ends after block).
        let (desired, current) = {
            let Some(hub) = self.app_state.input_hub() else {
                return;
            };
            (hub.pointer_lock_desired(), hub.is_pointer_locked())
        };

        if desired == current {
            return;
        }

        // Apply the OS cursor grab (borrows self.windows, not self.app_state).
        let lock_applied = if let Some(window) = self.window_for_handle(self.primary_window) {
            use winit::window::CursorGrabMode;
            if desired {
                let ok = window
                    .set_cursor_grab(CursorGrabMode::Locked)
                    .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
                    .is_ok();
                if ok {
                    window.set_cursor_visible(false);
                }
                ok
            } else {
                let _ = window.set_cursor_grab(CursorGrabMode::None);
                window.set_cursor_visible(true);
                true
            }
        } else {
            false
        };

        // Write the result back to the hub (separate borrow of self.app_state).
        if let Some(hub) = self.app_state.input_hub() {
            hub.set_pointer_locked(desired && lock_applied);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct GamepadBackend {
    gilrs: Option<gilrs::Gilrs>,
}

#[cfg(not(target_arch = "wasm32"))]
impl GamepadBackend {
    fn new() -> Self {
        match gilrs::Gilrs::new() {
            Ok(gilrs) => Self { gilrs: Some(gilrs) },
            Err(gilrs::Error::NotImplemented(gilrs)) => Self { gilrs: Some(gilrs) },
            Err(error) => {
                eprintln!("gamepad backend disabled: {error}");
                Self { gilrs: None }
            }
        }
    }

    fn poll(&mut self, hub: &mut InputHub) {
        let Some(gilrs) = &mut self.gilrs else {
            return;
        };
        while let Some(event) = gilrs.next_event() {
            let gamepad = GamepadId(usize::from(event.id) as u32);
            match event.event {
                gilrs::EventType::ButtonPressed(button, _) => {
                    hub.on_gamepad_button(gamepad, map_gilrs_button(button), true);
                }
                gilrs::EventType::ButtonReleased(button, _) => {
                    hub.on_gamepad_button(gamepad, map_gilrs_button(button), false);
                }
                gilrs::EventType::ButtonChanged(button, value, _) => {
                    hub.on_gamepad_button(gamepad, map_gilrs_button(button), value >= 0.5);
                }
                gilrs::EventType::AxisChanged(axis, value, _) => {
                    if let Some(axis) = map_gilrs_axis(axis) {
                        hub.on_gamepad_axis(gamepad, axis, value);
                    }
                }
                gilrs::EventType::Disconnected => {
                    hub.clear_gamepad(gamepad);
                }
                gilrs::EventType::Connected
                | gilrs::EventType::ButtonRepeated(_, _)
                | gilrs::EventType::Dropped
                | gilrs::EventType::ForceFeedbackEffectCompleted => {}
                _ => {}
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn map_gilrs_button(button: gilrs::Button) -> GamepadButton {
    match button {
        gilrs::Button::South => GamepadButton::South,
        gilrs::Button::East => GamepadButton::East,
        gilrs::Button::West => GamepadButton::West,
        gilrs::Button::North => GamepadButton::North,
        gilrs::Button::LeftTrigger => GamepadButton::LeftBumper,
        gilrs::Button::RightTrigger => GamepadButton::RightBumper,
        gilrs::Button::LeftTrigger2 => GamepadButton::LeftTrigger,
        gilrs::Button::RightTrigger2 => GamepadButton::RightTrigger,
        gilrs::Button::Select => GamepadButton::Select,
        gilrs::Button::Start => GamepadButton::Start,
        gilrs::Button::Mode => GamepadButton::Guide,
        gilrs::Button::LeftThumb => GamepadButton::LeftStick,
        gilrs::Button::RightThumb => GamepadButton::RightStick,
        gilrs::Button::DPadUp => GamepadButton::DPadUp,
        gilrs::Button::DPadDown => GamepadButton::DPadDown,
        gilrs::Button::DPadLeft => GamepadButton::DPadLeft,
        gilrs::Button::DPadRight => GamepadButton::DPadRight,
        other => GamepadButton::Other(other as u16),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn map_gilrs_axis(axis: gilrs::Axis) -> Option<GamepadAxis> {
    match axis {
        gilrs::Axis::LeftStickX => Some(GamepadAxis::LeftStickX),
        gilrs::Axis::LeftStickY => Some(GamepadAxis::LeftStickY),
        gilrs::Axis::RightStickX => Some(GamepadAxis::RightStickX),
        gilrs::Axis::RightStickY => Some(GamepadAxis::RightStickY),
        gilrs::Axis::LeftZ => Some(GamepadAxis::LeftTrigger),
        gilrs::Axis::RightZ => Some(GamepadAxis::RightTrigger),
        gilrs::Axis::DPadX | gilrs::Axis::DPadY | gilrs::Axis::Unknown => None,
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct ShellWindowEvent {
    window: WindowHandle,
    event: winit::event::WindowEvent,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShellWindowCloseAction {
    ExitApplication,
    RemoveWindow(WindowHandle),
    IgnoreUnknown,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct SafeAreaInsets {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug)]
struct ShellWindowState {
    scale_factor: f64,
    surface_size: SurfaceSize,
    safe_area: SafeAreaInsets,
    cursor_pos: clay_ui::WindowLogicalPx,
    primary_held: bool,
    focused: bool,
    hovered: bool,
    dirty: bool,
    waiting_for_surface_recreation: bool,
    appearance: WindowAppearance,
    native_appearance_report: NativeWindowAppearanceApplyReport,
}

#[cfg(not(target_arch = "wasm32"))]
struct ShellWindow {
    window: winit::window::Window,
    state: ShellWindowState,
}

#[cfg(not(target_arch = "wasm32"))]
impl ShellWindow {
    fn new(
        window: winit::window::Window,
        appearance: WindowAppearance,
        native_appearance_report: NativeWindowAppearanceApplyReport,
    ) -> Self {
        let size = window.inner_size();
        let state = ShellWindowState {
            scale_factor: window.scale_factor(),
            surface_size: SurfaceSize {
                width: size.width.max(1),
                height: size.height.max(1),
            },
            safe_area: SafeAreaInsets::default(),
            cursor_pos: clay_ui::WindowLogicalPx::ZERO,
            primary_held: false,
            focused: false,
            hovered: false,
            dirty: true,
            waiting_for_surface_recreation: false,
            appearance,
            native_appearance_report,
        };
        Self { window, state }
    }

    fn window(&self) -> &winit::window::Window {
        &self.window
    }

    fn state(&self) -> &ShellWindowState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut ShellWindowState {
        &mut self.state
    }

    fn refresh_surface_state(&mut self) {
        let size = self.window.inner_size();
        self.state.scale_factor = self.window.scale_factor();
        self.state.surface_size = SurfaceSize {
            width: size.width.max(1),
            height: size.height.max(1),
        };
        self.state.dirty = true;
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<App: EngineApp> ShellApp<App>
where
    App::Error: std::fmt::Debug,
{
    fn primary_window(&self) -> Option<&winit::window::Window> {
        self.windows
            .get(self.primary_window)
            .map(ShellWindow::window)
    }

    fn window_for_handle(&self, handle: WindowHandle) -> Option<&winit::window::Window> {
        self.windows.get(handle).map(ShellWindow::window)
    }

    fn window_state_for_handle(&self, handle: WindowHandle) -> Option<&ShellWindowState> {
        self.windows.get(handle).map(ShellWindow::state)
    }

    fn window_context_for_handle_mut(&mut self, handle: WindowHandle) -> Option<&mut ShellWindow> {
        self.windows.get_mut(handle)
    }

    fn close_action_for_window(&self, handle: WindowHandle) -> ShellWindowCloseAction {
        close_action_for_window(self.primary_window, self.windows.contains(handle), handle)
    }

    fn close_non_primary_window(&mut self, handle: WindowHandle) {
        if handle == self.primary_window {
            return;
        }
        let Some(winit_id) = self.windows.get(handle).map(|window| window.window().id()) else {
            return;
        };
        self.windows.remove(handle);
        self.winit_windows.remove(&winit_id);
        self.publish_window_diagnostics();
    }

    fn request_redraw_for_window(&mut self, handle: WindowHandle) {
        if let Some(window) = self.window_context_for_handle_mut(handle) {
            window.state_mut().dirty = true;
            window.window().request_redraw();
        }
    }

    fn publish_window_diagnostics(&self) {
        let windows = self.window_diagnostics();
        self.runtime.controller().update_diagnostics(|diagnostics| {
            diagnostics.windows = windows;
        });
    }

    fn window_diagnostics(&self) -> RuntimeWindowDiagnostics {
        let mut diagnostics = RuntimeWindowDiagnostics {
            live_count: self.windows.live_count(),
            ..RuntimeWindowDiagnostics::default()
        };
        for (handle, window) in self.windows.iter() {
            let state = window.state();
            let id = handle.id().raw();
            if state.focused {
                diagnostics.focused_window = Some(id);
            }
            if state.hovered {
                diagnostics.hovered_window = Some(id);
            }
            if state.dirty {
                diagnostics.dirty_count += 1;
            }
            if state.waiting_for_surface_recreation {
                diagnostics.waiting_for_surface_recreation_count += 1;
            }
        }
        diagnostics
    }

    fn window_handle_for_winit_id(
        &self,
        window_id: winit::window::WindowId,
    ) -> Option<WindowHandle> {
        self.winit_windows.get(&window_id).copied()
    }

    fn dispatch_window_event(&mut self, event: ShellWindowEvent) {
        let window_handle = event.window;
        match event {
            ShellWindowEvent {
                event: winit::event::WindowEvent::CloseRequested,
                ..
            } => match self.close_action_for_window(window_handle) {
                ShellWindowCloseAction::ExitApplication => {
                    std::process::exit(0);
                }
                ShellWindowCloseAction::RemoveWindow(window) => {
                    self.close_non_primary_window(window);
                }
                ShellWindowCloseAction::IgnoreUnknown => {}
            },
            ShellWindowEvent {
                event: winit::event::WindowEvent::Resized(size),
                ..
            } => {
                if size.width > 0 && size.height > 0 {
                    if let Some(window) = self.window_context_for_handle_mut(window_handle) {
                        window.refresh_surface_state();
                    }
                    if let Err(e) = self.app_state.resize(size.width, size.height) {
                        eprintln!("resize failed: {e:?}");
                        std::process::exit(1);
                    }
                    let resize_result = self.runtime.surface_mut().resize(SurfaceSize {
                        width: size.width.max(1),
                        height: size.height.max(1),
                    });
                    if let Some(window) = self.window_context_for_handle_mut(window_handle) {
                        window.state_mut().waiting_for_surface_recreation = resize_result.is_err();
                    }
                    if let Err(e) = resize_result {
                        eprintln!("surface resize failed: {e:?}");
                    }
                    if let Some(window) = self.window_for_handle(window_handle) {
                        let snapshot = window_settings_snapshot(
                            window,
                            &self._config,
                            self.runtime.controller(),
                        );
                        self.runtime.controller().set_settings(snapshot);
                        let native_appearance_report = apply_window_appearance_from_settings(
                            window,
                            self.runtime.controller(),
                        );
                        let appearance = window_appearance_from_settings(self.runtime.controller());
                        if let Some(window) = self.window_context_for_handle_mut(window_handle) {
                            window.state_mut().appearance = appearance;
                            window.state_mut().native_appearance_report = native_appearance_report;
                        }
                        self.request_redraw_for_window(window_handle);
                    }
                }
            }
            ShellWindowEvent {
                event: winit::event::WindowEvent::ScaleFactorChanged { .. },
                ..
            } => {
                if let Some(window) = self.window_context_for_handle_mut(window_handle) {
                    window.refresh_surface_state();
                }
            }
            ShellWindowEvent {
                event: winit::event::WindowEvent::Focused(focused),
                ..
            } => {
                if let Some(window) = self.window_context_for_handle_mut(window_handle) {
                    window.state_mut().focused = focused;
                }
            }
            ShellWindowEvent {
                event: winit::event::WindowEvent::Moved(position),
                ..
            } => {
                if let Some(window) = self.window_for_handle(window_handle) {
                    let mut snapshot =
                        window_settings_snapshot(window, &self._config, self.runtime.controller());
                    snapshot.window_position = Some((position.x, position.y));
                    self.runtime.controller().set_settings(snapshot);
                }
            }
            ShellWindowEvent {
                event: winit::event::WindowEvent::ModifiersChanged(modifiers),
                ..
            } => {
                self.modifiers = crate::input::key_modifiers_from_winit(modifiers.state());
            }
            ShellWindowEvent {
                event: winit::event::WindowEvent::KeyboardInput { event, .. },
                ..
            } => {
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
            ShellWindowEvent {
                event: winit::event::WindowEvent::CursorMoved { position, .. },
                ..
            } => {
                // winit CursorMoved delivers top-left/Y-down physical pixels.
                // Keep shell UI input in the same space as the surface/debug
                // overlay so hit testing and displayed geometry share one rect.
                let pos = clay_ui::WindowLogicalPx::new(position.x as f32, position.y as f32);
                if let Some(window) = self.window_context_for_handle_mut(window_handle) {
                    window.state_mut().cursor_pos = pos;
                    window.state_mut().hovered = true;
                }
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
            ShellWindowEvent {
                event: winit::event::WindowEvent::CursorLeft { .. },
                ..
            } => {
                if let Some(window) = self.window_context_for_handle_mut(window_handle) {
                    window.state_mut().hovered = false;
                    window.state_mut().primary_held = false;
                }
            }
            ShellWindowEvent {
                event: winit::event::WindowEvent::MouseInput { state, button, .. },
                ..
            } => {
                use winit::event::{ElementState, MouseButton};
                let btn: u8 = match button {
                    MouseButton::Left => 0,
                    MouseButton::Right => 1,
                    MouseButton::Middle => 2,
                    _ => 3,
                };
                let pressed = state == ElementState::Pressed;
                let pos = if let Some(window) = self.window_context_for_handle_mut(window_handle) {
                    if btn == 0 {
                        window.state_mut().primary_held = pressed;
                    }
                    window.state().cursor_pos
                } else {
                    clay_ui::WindowLogicalPx::ZERO
                };
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
            ShellWindowEvent {
                event: winit::event::WindowEvent::MouseWheel { delta, .. },
                ..
            } => {
                use winit::event::MouseScrollDelta;
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x * 20.0, -y * 20.0),
                    MouseScrollDelta::PixelDelta(pos) => (pos.x as f32, pos.y as f32),
                };
                let pos = self
                    .window_state_for_handle(window_handle)
                    .map(|state| state.cursor_pos)
                    .unwrap_or(clay_ui::WindowLogicalPx::ZERO);
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
            ShellWindowEvent {
                event: winit::event::WindowEvent::RedrawRequested,
                ..
            } => {
                let (window_scale_factor, window_logical_size, _window_safe_area) =
                    if let Some(state) = self.window_state_for_handle(window_handle) {
                        (
                            state.scale_factor as f32,
                            Some([
                                state.surface_size.width as f32,
                                state.surface_size.height as f32,
                            ]),
                            state.safe_area,
                        )
                    } else {
                        (1.0, None, SafeAreaInsets::default())
                    };
                let mut runtime_frame = match self.runtime.acquire_frame() {
                    Ok(frame) => frame,
                    Err(e) => {
                        eprintln!("failed to acquire runtime frame: {e:?}");
                        std::process::exit(1);
                    }
                };

                let mut render_frame = runtime_frame.shell_frame();
                render_frame.set_window_scale_factor(window_scale_factor);
                if let Some(size) = window_logical_size {
                    render_frame.set_window_logical_size(size);
                }

                if let Err(e) = self
                    .app_state
                    .render(&mut render_frame, runtime_frame.surface_image())
                {
                    eprintln!("render failed: {e:?}");
                    std::process::exit(1);
                }

                // Present — explicit call so errors surface here rather than in Drop.
                if let Err(e) = runtime_frame.finish_and_present() {
                    eprintln!("present failed: {e:?}");
                    std::process::exit(1);
                }
                // Drop the frame explicitly to release the `&mut AppRuntime` borrow
                // before the subsequent `self` accesses below.
                drop(runtime_frame);

                if let Some(window) = self.window_context_for_handle_mut(window_handle) {
                    window.state_mut().dirty = false;
                }
                self.request_redraw_for_window(window_handle);
            }
            _ => {}
        }
        self.publish_window_diagnostics();
    }

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
        if let Some(window) = self.primary_window() {
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
        let mut native_appearance_report = None;
        if affects_native_window {
            if let Some(window) = self.window_for_handle(self.primary_window) {
                let apply_report = apply_window_appearance_from_settings(window, controller);
                let appearance = window_appearance_from_settings(controller);
                let snapshot =
                    window_settings_snapshot(window, &self._config, self.runtime.controller());
                self.runtime.controller().set_settings(snapshot);
                if let Some(window) = self.window_context_for_handle_mut(self.primary_window) {
                    window.state_mut().appearance = appearance;
                    window.state_mut().native_appearance_report = apply_report.clone();
                }
                native_appearance_report = Some(apply_report);
            }
        }
        let window_report =
            window_reconfigure_apply_report(changes, native_appearance_report.as_ref());
        self.runtime
            .controller()
            .record_runtime_apply_report(window_report);

        if !changes.iter().any(|change| {
            matches!(
                change.setting,
                RuntimeSettingId::Engine(RuntimeSettingKey::HdrMode)
                    | RuntimeSettingId::Engine(RuntimeSettingKey::PresentMode)
                    | RuntimeSettingId::Engine(RuntimeSettingKey::PresentPolicy)
                    | RuntimeSettingId::Engine(RuntimeSettingKey::SurfaceTransparency)
            )
        }) {
            return;
        }

        if let Some(window) = self.window_context_for_handle_mut(self.primary_window) {
            window.state_mut().waiting_for_surface_recreation = true;
        }
        let surface_report = self.runtime.apply_surface_runtime_settings(changes);
        if let Some(window) = self.window_context_for_handle_mut(self.primary_window) {
            window.state_mut().waiting_for_surface_recreation = false;
        }
        for result in &surface_report.changes {
            if let RuntimeChangeResult::Failed {
                setting, reason, ..
            } = result
            {
                eprintln!("surface runtime setting apply failed for {setting}: {reason}");
            }
        }
        if let Some(window) = self.primary_window() {
            let snapshot =
                window_settings_snapshot(window, &self._config, self.runtime.controller());
            self.runtime.controller().set_settings(snapshot);
        }
    }
}

#[cfg(test)]
fn parse_present_policy_setting(
    value: &str,
    explicit_present_mode: Option<crate::SurfacePresentMode>,
) -> Option<crate::SurfacePresentMode> {
    match value {
        "Auto" => None,
        "NoTear" => Some(crate::SurfacePresentMode::Fifo),
        "LowLatencyNoTear" => Some(crate::SurfacePresentMode::Mailbox),
        "LowLatencyAllowTear" => Some(crate::SurfacePresentMode::RelaxedFifo),
        "Explicit" => explicit_present_mode,
        _ => None,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn close_action_for_window(
    primary_window: WindowHandle,
    window_known: bool,
    requested_window: WindowHandle,
) -> ShellWindowCloseAction {
    if !window_known {
        ShellWindowCloseAction::IgnoreUnknown
    } else if requested_window == primary_window {
        ShellWindowCloseAction::ExitApplication
    } else {
        ShellWindowCloseAction::RemoveWindow(requested_window)
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct CreatedShellWindow {
    desc: WindowDesc,
    window: winit::window::Window,
    native_appearance_report: NativeWindowAppearanceApplyReport,
}

#[cfg(not(target_arch = "wasm32"))]
fn drain_initial_window_commands(
    event_loop: &winit::event_loop::EventLoop<()>,
    commands: &mut ShellEventLoopCommandQueue,
) -> std::result::Result<Vec<CreatedShellWindow>, String> {
    let mut created = Vec::new();
    while let Some(command) = commands.pop_front() {
        match command {
            ShellEventLoopCommand::CreateWindow(desc) => {
                created.push(create_shell_window(event_loop, desc)?);
            }
        }
    }
    Ok(created)
}

#[cfg(not(target_arch = "wasm32"))]
fn create_shell_window(
    event_loop: &winit::event_loop::EventLoop<()>,
    desc: WindowDesc,
) -> std::result::Result<CreatedShellWindow, String> {
    #[allow(deprecated)]
    let window = event_loop
        .create_window(window_attributes_from_desc(&desc))
        .map_err(|error| format!("failed to create window: {error}"))?;
    let native_appearance_report = apply_native_window_appearance_report_for_window(
        &window,
        Some((window.inner_size().width, window.inner_size().height)),
        desc.appearance,
    );
    Ok(CreatedShellWindow {
        desc,
        window,
        native_appearance_report,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn window_attributes_from_desc(desc: &WindowDesc) -> winit::window::WindowAttributes {
    use winit::{
        dpi::{LogicalPosition, LogicalSize},
        window::{Window, WindowLevel},
    };

    let mut attributes = Window::default_attributes()
        .with_title(&desc.title)
        .with_inner_size(LogicalSize::new(desc.width as f64, desc.height as f64))
        .with_resizable(desc.resizable)
        .with_decorations(desc.decorations)
        .with_maximized(desc.maximized)
        .with_window_level(if desc.always_on_top {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        })
        .with_fullscreen(fullscreen_for_mode(desc.window_mode));
    if let Some((x, y)) = desc.position {
        attributes = attributes.with_position(LogicalPosition::new(x as f64, y as f64));
    }
    apply_window_appearance_to_attributes(attributes, desc.appearance)
}

fn apply_window_appearance_to_attributes(
    mut attributes: winit::window::WindowAttributes,
    appearance: WindowAppearance,
) -> winit::window::WindowAttributes {
    let wants_transparent = appearance.transparency == SurfaceTransparency::Enabled;
    attributes = attributes.with_transparent(wants_transparent);

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        attributes = attributes.with_blur(appearance_wants_native_blur(appearance));
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
) -> NativeWindowAppearanceApplyReport {
    let appearance = window_appearance_from_settings(controller);
    let wants_transparency = appearance.transparency == SurfaceTransparency::Enabled;

    window.set_transparent(wants_transparency);

    let native_report = apply_native_window_appearance_report_for_window(
        window,
        Some((window.inner_size().width, window.inner_size().height)),
        appearance,
    );
    if native_report.is_degraded() || native_report.is_failed() {
        if native_report.is_degraded() {
            eprintln!(
                "native window appearance apply degraded: {}",
                native_report.diagnostic_string()
            );
        } else {
            eprintln!(
                "native window appearance apply fell back to winit: {}",
                native_report.diagnostic_string()
            );
        }

        #[cfg(target_os = "windows")]
        {
            window.set_system_backdrop(windows_backdrop_for_appearance(appearance));
        }

        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            window.set_blur(appearance_wants_native_blur(appearance));
        }
    }

    controller.update_diagnostics(|diagnostics| {
        diagnostics.native_window_appearance = Some(native_report.diagnostic_string());
    });
    native_report
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

fn window_reconfigure_apply_report(
    changes: &[RuntimeSettingChange],
    native_appearance_report: Option<&NativeWindowAppearanceApplyReport>,
) -> RuntimeApplyReport {
    let mut report = RuntimeApplyReport::default();
    for change in changes {
        if !is_window_reconfigure_setting(&change.setting) {
            continue;
        }
        report.changes.push(
            match (
                is_native_window_appearance_setting(&change.setting),
                native_appearance_report,
            ) {
                (true, Some(native_report)) => {
                    native_window_appearance_change_result(change.setting.clone(), native_report)
                }
                _ => RuntimeChangeResult::Applied {
                    setting: change.setting.clone(),
                    path: RuntimeApplyPath::WindowReconfigure,
                },
            },
        );
    }
    report
}

fn native_window_appearance_change_result(
    setting: RuntimeSettingId,
    native_report: &NativeWindowAppearanceApplyReport,
) -> RuntimeChangeResult {
    match native_report.status {
        NativeWindowAppearanceStatus::Applied => RuntimeChangeResult::Applied {
            setting,
            path: RuntimeApplyPath::WindowReconfigure,
        },
        NativeWindowAppearanceStatus::Degraded => RuntimeChangeResult::Degraded {
            setting,
            path: RuntimeApplyPath::WindowReconfigure,
            reason: native_report.diagnostic_string(),
        },
        NativeWindowAppearanceStatus::Failed => RuntimeChangeResult::Failed {
            setting,
            path: RuntimeApplyPath::WindowReconfigure,
            reason: native_report.diagnostic_string(),
        },
    }
}

fn is_window_reconfigure_setting(setting: &RuntimeSettingId) -> bool {
    matches!(
        setting,
        RuntimeSettingId::Engine(
            RuntimeSettingKey::WindowTitle
                | RuntimeSettingKey::WindowWidth
                | RuntimeSettingKey::WindowHeight
                | RuntimeSettingKey::WindowPositionX
                | RuntimeSettingKey::WindowPositionY
                | RuntimeSettingKey::WindowMode
                | RuntimeSettingKey::WindowDecorations
                | RuntimeSettingKey::WindowResizable
                | RuntimeSettingKey::WindowMaximized
                | RuntimeSettingKey::WindowAlwaysOnTop
                | RuntimeSettingKey::WindowCornerStyle
                | RuntimeSettingKey::WindowBackgroundEffect
        )
    )
}

fn is_native_window_appearance_setting(setting: &RuntimeSettingId) -> bool {
    matches!(
        setting,
        RuntimeSettingId::Engine(
            RuntimeSettingKey::WindowCornerStyle | RuntimeSettingKey::WindowBackgroundEffect
        )
    )
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

#[cfg(test)]
mod tests {
    use crate::{RuntimeSettingValue, SurfacePresentMode};

    use super::*;

    #[test]
    fn window_desc_captures_window_config() {
        let config = WindowConfig::new("primary", 1280, 720)
            .with_position(10, 20)
            .with_resizable(true)
            .with_decorations(false)
            .with_maximized(true)
            .with_always_on_top(true)
            .with_borderless_fullscreen(true)
            .with_hdr(true)
            .with_window_appearance_preset(WindowAppearancePreset::Blur);

        let desc = WindowDesc::from_config(&config);

        assert_eq!(desc.title, "primary");
        assert_eq!(desc.width, 1280);
        assert_eq!(desc.height, 720);
        assert_eq!(desc.position, Some((10, 20)));
        assert!(desc.resizable);
        assert!(!desc.decorations);
        assert!(desc.maximized);
        assert!(desc.always_on_top);
        assert_eq!(desc.window_mode, WindowMode::BorderlessFullscreen);
        assert!(desc.prefer_hdr);
        assert_eq!(
            desc.appearance,
            WindowAppearance::from_preset(WindowAppearancePreset::Blur)
        );
    }

    #[test]
    fn shell_event_loop_command_queue_preserves_create_window_order() {
        let first = WindowDesc::from_config(&WindowConfig::new("first", 100, 100));
        let second = WindowDesc::from_config(&WindowConfig::new("second", 200, 200));
        let mut queue = ShellEventLoopCommandQueue::new();

        queue.create_window(first.clone());
        queue.create_window(second.clone());

        assert_eq!(queue.len(), 2);
        assert_eq!(
            queue.pop_front(),
            Some(ShellEventLoopCommand::CreateWindow(first))
        );
        assert_eq!(
            queue.pop_front(),
            Some(ShellEventLoopCommand::CreateWindow(second))
        );
        assert_eq!(queue.pop_front(), None);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn close_action_exits_for_primary_window() {
        let mut windows = WindowRegistry::new();
        let primary = windows.insert("primary");

        assert_eq!(
            close_action_for_window(primary, windows.contains(primary), primary),
            ShellWindowCloseAction::ExitApplication
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn close_action_removes_non_primary_window() {
        let mut windows = WindowRegistry::new();
        let primary = windows.insert("primary");
        let secondary = windows.insert("secondary");

        assert_eq!(
            close_action_for_window(primary, windows.contains(secondary), secondary),
            ShellWindowCloseAction::RemoveWindow(secondary)
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn close_action_ignores_unknown_window() {
        let mut windows = WindowRegistry::new();
        let primary = windows.insert("primary");
        let stale = windows.insert("stale");
        assert_eq!(windows.remove(stale), Some("stale"));

        assert_eq!(
            close_action_for_window(primary, windows.contains(stale), stale),
            ShellWindowCloseAction::IgnoreUnknown
        );
    }

    #[test]
    fn present_policy_maps_to_preferred_present_mode() {
        assert_eq!(parse_present_policy_setting("Auto", None), None);
        assert_eq!(
            parse_present_policy_setting("NoTear", None),
            Some(SurfacePresentMode::Fifo)
        );
        assert_eq!(
            parse_present_policy_setting("LowLatencyNoTear", None),
            Some(SurfacePresentMode::Mailbox)
        );
        assert_eq!(
            parse_present_policy_setting("LowLatencyAllowTear", None),
            Some(SurfacePresentMode::RelaxedFifo)
        );
        assert_eq!(
            parse_present_policy_setting("Explicit", Some(SurfacePresentMode::RelaxedFifo)),
            Some(SurfacePresentMode::RelaxedFifo)
        );
    }

    #[test]
    fn window_reconfigure_report_marks_basic_window_changes_applied() {
        let changes = vec![RuntimeSettingChange {
            setting: RuntimeSettingId::from(RuntimeSettingKey::WindowTitle),
            value: RuntimeSettingValue::Text("new title".to_string()),
            path: RuntimeApplyPath::WindowReconfigure,
            revision: 1,
        }];

        let report = window_reconfigure_apply_report(&changes, None);

        assert_eq!(
            report.changes,
            vec![RuntimeChangeResult::Applied {
                setting: RuntimeSettingId::from(RuntimeSettingKey::WindowTitle),
                path: RuntimeApplyPath::WindowReconfigure,
            }]
        );
    }

    #[test]
    fn window_reconfigure_report_surfaces_native_appearance_degradation() {
        let changes = vec![
            RuntimeSettingChange {
                setting: RuntimeSettingId::from(RuntimeSettingKey::WindowBackgroundEffect),
                value: RuntimeSettingValue::Text("Blur".to_string()),
                path: RuntimeApplyPath::WindowReconfigure,
                revision: 1,
            },
            RuntimeSettingChange {
                setting: RuntimeSettingId::from(RuntimeSettingKey::SurfaceTransparency),
                value: RuntimeSettingValue::Bool(true),
                path: RuntimeApplyPath::SurfaceRecreate,
                revision: 2,
            },
        ];
        let native_report = NativeWindowAppearanceApplyReport {
            requested: "blur",
            protocol: "test-protocol",
            status: NativeWindowAppearanceStatus::Degraded,
            fallback: Some("winit"),
            reason: Some("blur protocol unavailable".to_string()),
        };

        let report = window_reconfigure_apply_report(&changes, Some(&native_report));

        assert_eq!(report.changes.len(), 1);
        assert!(matches!(
            &report.changes[0],
            RuntimeChangeResult::Degraded {
                setting,
                path: RuntimeApplyPath::WindowReconfigure,
                reason,
            } if setting == &RuntimeSettingId::from(RuntimeSettingKey::WindowBackgroundEffect)
                && reason.contains("status=degraded")
                && reason.contains("blur protocol unavailable")
        ));
    }
}
