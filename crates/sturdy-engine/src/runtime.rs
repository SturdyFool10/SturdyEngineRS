//! First-party application runtime surface.
//!
//! This module defines the initial public API shape for the runtime shell work
//! described in the roadmap. The current implementation is intentionally thin:
//! it establishes the engine-owned types and access patterns without changing
//! the existing application shell behavior yet.

use std::sync::{Arc, Mutex};

use crate::{
    BackendKind, Engine, Format, GraphImage, RenderFrame, Result, Surface, SurfaceColorSpace,
    SurfaceImage, SurfacePresentMode, SurfaceSize,
};

/// Engine-owned runtime shell state.
///
/// This is the long-term home for the common application runtime loop and
/// related engine-owned systems. The initial slice only establishes the public
/// type and the basic ownership model.
pub struct AppRuntime {
    engine: Engine,
    surface: Surface,
    controller: RuntimeController,
    default_scene_target: DefaultSceneTargetConfig,
    debug_images: DebugImageRegistry,
}

impl AppRuntime {
    /// Create a runtime shell from an engine and surface.
    pub fn new(engine: Engine, surface: Surface) -> Self {
        let runtime = Self {
            default_scene_target: DefaultSceneTargetConfig::new(&engine),
            debug_images: DebugImageRegistry::default(),
            controller: RuntimeController::new(RuntimeSettingsSnapshot {
                backend: engine.backend_kind(),
                adapter_name: engine.adapter_name(),
                hdr_enabled: surface_is_hdr(surface.info().color_space),
                present_mode: None,
                surface_size: surface.info().size,
            }),
            engine,
            surface,
        };
        runtime.refresh_controller_state();
        runtime
    }

    /// Access the engine owned by the runtime.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Access the presentation surface owned by the runtime.
    pub fn surface(&self) -> &Surface {
        &self.surface
    }

    /// Access the presentation surface mutably.
    pub fn surface_mut(&mut self) -> &mut Surface {
        &mut self.surface
    }

    /// Access the runtime settings controller.
    pub fn controller(&self) -> &RuntimeController {
        &self.controller
    }

    /// Access the runtime settings controller mutably.
    pub fn controller_mut(&mut self) -> &mut RuntimeController {
        &mut self.controller
    }

    /// Return the current default HDR scene-target policy.
    pub fn default_scene_target(&self) -> &DefaultSceneTargetConfig {
        &self.default_scene_target
    }

    /// Return the runtime-owned debug image registry.
    pub fn debug_images(&self) -> &DebugImageRegistry {
        &self.debug_images
    }

    /// Refresh runtime settings/diagnostics snapshots from the current engine and surface state.
    pub fn refresh_controller_state(&self) {
        let surface_info = self.surface.info();
        self.controller.set_settings(RuntimeSettingsSnapshot {
            backend: self.engine.backend_kind(),
            adapter_name: self.engine.adapter_name(),
            hdr_enabled: surface_is_hdr(surface_info.color_space),
            present_mode: None,
            surface_size: surface_info.size,
        });
        self.controller.update_diagnostics(|diagnostics| {
            diagnostics.backend = self.engine.backend_kind();
            diagnostics.adapter_name = self.engine.adapter_name();
            diagnostics.surface_format = surface_info.format;
            diagnostics.surface_color_space = surface_info.color_space;
            diagnostics.hdr_output = surface_is_hdr(surface_info.color_space);
            diagnostics.present_mode = None;
        });
    }

    /// Acquire the current swapchain image and begin a render frame for it.
    pub fn acquire_frame(&mut self) -> Result<AppRuntimeFrame<'_>> {
        self.refresh_controller_state();
        self.debug_images.clear();
        self.controller.clear_overlay_lines();
        let surface_image = self.surface.acquire_image()?;
        let render_frame = self.engine.begin_render_frame_for(&surface_image)?;
        Ok(AppRuntimeFrame {
            runtime: self,
            surface_image,
            render_frame,
        })
    }

    /// Decompose the runtime into its current owned parts.
    pub fn into_parts(self) -> (Engine, Surface, RuntimeController) {
        (self.engine, self.surface, self.controller)
    }
}

/// Runtime-owned per-frame state for the currently acquired surface image.
pub struct AppRuntimeFrame<'a> {
    runtime: &'a mut AppRuntime,
    surface_image: SurfaceImage,
    render_frame: RenderFrame,
}

impl<'a> AppRuntimeFrame<'a> {
    /// Access the acquired surface image.
    pub fn surface_image(&self) -> &SurfaceImage {
        &self.surface_image
    }

    /// Access the render frame.
    pub fn render_frame(&self) -> &RenderFrame {
        &self.render_frame
    }

    /// Access the render frame mutably.
    pub fn render_frame_mut(&mut self) -> &mut RenderFrame {
        &mut self.render_frame
    }

    /// Return the runtime-owned default HDR scene-target policy for this frame.
    pub fn default_scene_target(&self) -> &DefaultSceneTargetConfig {
        self.runtime.default_scene_target()
    }

    /// Return the runtime-owned debug image registry for this frame.
    pub fn debug_images(&self) -> &DebugImageRegistry {
        self.runtime.debug_images()
    }

    /// Create the default HDR scene target for this frame.
    pub fn default_hdr_scene_target(
        &self,
        name: impl Into<String>,
        requested_msaa_samples: u8,
    ) -> Result<GraphImage> {
        self.default_scene_target().create(
            &self.render_frame,
            name,
            requested_msaa_samples,
        )
    }

    /// Resolve the default HDR scene target to the single-sample scene color used downstream.
    pub fn resolve_default_hdr_scene_target(
        &self,
        scene_target: &GraphImage,
        resolved_name: impl Into<String>,
    ) -> Result<GraphImage> {
        self.default_scene_target()
            .resolve(&self.render_frame, scene_target, resolved_name)
    }

    /// Create a shell-frame wrapper for compatibility with the existing app shell.
    pub(crate) fn shell_frame(&self) -> crate::application::ShellFrame<'_> {
        crate::application::ShellFrame::new(
            self.render_frame.clone(),
            &self.surface_image,
            self.default_scene_target().clone(),
            self.debug_images().clone(),
            self.runtime.controller.clone(),
        )
    }

    /// Flush, wait, and present through the runtime-owned surface.
    pub fn finish_and_present(&mut self) -> Result<()> {
        self.render_frame.flush()?;
        self.render_frame.wait()?;
        self.runtime.surface.present()
    }
}

/// App-provided content hooks that the runtime shell will drive.
pub trait AppLayer {
    /// Build or render the scene portion of the frame.
    fn render_scene(&mut self, _cx: &mut SceneRenderContext<'_>) -> Result<()> {
        Ok(())
    }

    /// Build UI or overlay content for the frame.
    fn build_ui(&mut self, _ui: &mut UiContext<'_>) -> Result<()> {
        Ok(())
    }
}

/// Runtime settings and diagnostics controller.
#[derive(Clone, Debug)]
pub struct RuntimeController {
    shared: Arc<Mutex<RuntimeShared>>,
}

impl RuntimeController {
    pub fn new(settings: RuntimeSettingsSnapshot) -> Self {
        Self {
            shared: Arc::new(Mutex::new(RuntimeShared {
                settings,
                diagnostics: RuntimeDiagnostics::default(),
                overlay_lines: Vec::new(),
            })),
        }
    }

    /// Return the current runtime settings snapshot.
    pub fn settings(&self) -> RuntimeSettingsSnapshot {
        self.shared
            .lock()
            .expect("runtime controller poisoned")
            .settings
            .clone()
    }

    /// Begin a transaction that can update runtime settings coherently.
    pub fn transact(&mut self) -> RuntimeSettingsTransaction<'_> {
        RuntimeSettingsTransaction {
            controller: self,
            pending: Vec::new(),
        }
    }

    /// Return the current runtime diagnostics snapshot.
    pub fn diagnostics(&self) -> RuntimeDiagnostics {
        self.shared
            .lock()
            .expect("runtime controller poisoned")
            .diagnostics
            .clone()
    }

    /// Return the current overlay text lines.
    pub fn overlay_lines(&self) -> Vec<String> {
        self.shared
            .lock()
            .expect("runtime controller poisoned")
            .overlay_lines
            .clone()
    }

    pub(crate) fn set_settings(&self, settings: RuntimeSettingsSnapshot) {
        self.shared
            .lock()
            .expect("runtime controller poisoned")
            .settings = settings;
    }

    pub(crate) fn update_diagnostics(&self, f: impl FnOnce(&mut RuntimeDiagnostics)) {
        let mut shared = self.shared.lock().expect("runtime controller poisoned");
        f(&mut shared.diagnostics);
    }

    pub(crate) fn set_overlay_lines(&self, lines: Vec<String>) {
        self.shared
            .lock()
            .expect("runtime controller poisoned")
            .overlay_lines = lines;
    }

    pub(crate) fn clear_overlay_lines(&self) {
        self.shared
            .lock()
            .expect("runtime controller poisoned")
            .overlay_lines
            .clear();
    }
}

/// Snapshot of the current runtime settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSettingsSnapshot {
    pub backend: BackendKind,
    pub adapter_name: Option<String>,
    pub hdr_enabled: bool,
    pub present_mode: Option<SurfacePresentMode>,
    pub surface_size: SurfaceSize,
}

impl Default for RuntimeSettingsSnapshot {
    fn default() -> Self {
        Self {
            backend: BackendKind::Auto,
            adapter_name: None,
            hdr_enabled: false,
            present_mode: None,
            surface_size: SurfaceSize { width: 1, height: 1 },
        }
    }
}

/// Snapshot of runtime diagnostics made visible to applications.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuntimeDiagnostics {
    pub backend: BackendKind,
    pub adapter_name: Option<String>,
    pub surface_format: Format,
    pub surface_color_space: SurfaceColorSpace,
    pub hdr_output: bool,
    pub present_mode: Option<SurfacePresentMode>,
    pub aa_mode_label: Option<String>,
    pub actual_msaa_samples: Option<u8>,
    pub bloom_enabled: Option<bool>,
    pub bloom_only: Option<bool>,
    pub motion_validation: Option<String>,
    pub debug_images: Vec<String>,
    pub graph: RuntimeGraphDiagnostics,
    pub timings: RuntimeTimingSummary,
}

/// Summary information about the currently recorded render graph.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeGraphDiagnostics {
    pub pass_count: usize,
    pub image_count: usize,
    pub warning_count: usize,
    pub error_count: usize,
}

/// Placeholder timing model for runtime diagnostics.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuntimeTimingSummary {
    pub available: bool,
    pub gpu_frame_time_ms: Option<f32>,
    pub pass_timings: Vec<RuntimePassTiming>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuntimePassTiming {
    pub name: String,
    pub gpu_time_ms: Option<f32>,
}

#[derive(Debug)]
struct RuntimeShared {
    settings: RuntimeSettingsSnapshot,
    diagnostics: RuntimeDiagnostics,
    overlay_lines: Vec<String>,
}

/// Runtime-owned registry of named graph images exposed for inspection/debugging.
#[derive(Clone, Debug, Default)]
pub struct DebugImageRegistry {
    names: Arc<Mutex<Vec<String>>>,
}

impl DebugImageRegistry {
    pub fn clear(&self) {
        self.names.lock().expect("debug image registry poisoned").clear();
    }

    pub fn register(&self, image: &GraphImage, name: impl Into<String>) {
        let name = name.into();
        image.register_as(name.clone());
        let mut names = self.names.lock().expect("debug image registry poisoned");
        if !names.iter().any(|existing| existing == &name) {
            names.push(name);
        }
    }

    pub fn names(&self) -> Vec<String> {
        self.names
            .lock()
            .expect("debug image registry poisoned")
            .clone()
    }
}

/// Runtime apply categories used by later reconfiguration work.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RuntimeApplyPath {
    Immediate,
    GraphRebuild,
    SurfaceRecreate,
    WindowReconfigure,
    DeviceMigration,
}

/// Identifier for a runtime-facing setting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSettingKey(pub &'static str);

/// Result of applying a set of runtime changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeApplyReport {
    pub changes: Vec<RuntimeChangeResult>,
}

/// Outcome for an individual runtime-setting request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeChangeResult {
    Applied {
        setting: RuntimeSettingKey,
        path: RuntimeApplyPath,
    },
    Degraded {
        setting: RuntimeSettingKey,
        path: RuntimeApplyPath,
        reason: String,
    },
    Rejected {
        setting: RuntimeSettingKey,
        reason: String,
    },
}

/// Mutable transaction over runtime settings.
pub struct RuntimeSettingsTransaction<'a> {
    controller: &'a mut RuntimeController,
    pending: Vec<RuntimeSettingKey>,
}

impl<'a> RuntimeSettingsTransaction<'a> {
    /// Record a placeholder change request.
    ///
    /// The initial runtime slice only establishes the transaction surface.
    pub fn note_change(mut self, setting: RuntimeSettingKey) -> Self {
        self.pending.push(setting);
        self
    }

    /// Apply the pending transaction.
    ///
    /// This is currently a no-op placeholder that returns an empty report while
    /// the runtime reconfiguration machinery is being built.
    pub fn apply(self) -> Result<RuntimeApplyReport> {
        let _ = self.controller;
        let _ = self.pending;
        Ok(RuntimeApplyReport::default())
    }
}

/// Scene-building context handed to [`AppLayer::render_scene`].
pub struct SceneRenderContext<'a> {
    runtime: &'a mut AppRuntime,
}

impl<'a> SceneRenderContext<'a> {
    pub fn new(runtime: &'a mut AppRuntime) -> Self {
        Self { runtime }
    }

    pub fn runtime(&mut self) -> &mut AppRuntime {
        self.runtime
    }
}

/// UI-building context handed to [`AppLayer::build_ui`].
pub struct UiContext<'a> {
    runtime: &'a mut AppRuntime,
}

impl<'a> UiContext<'a> {
    pub fn new(runtime: &'a mut AppRuntime) -> Self {
        Self { runtime }
    }

    pub fn runtime(&mut self) -> &mut AppRuntime {
        self.runtime
    }
}

/// Runtime-owned policy for the default HDR scene target used by app rendering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultSceneTargetConfig {
    format: Format,
    max_msaa_samples: u8,
}

impl DefaultSceneTargetConfig {
    pub fn new(engine: &Engine) -> Self {
        Self {
            format: Format::Rgba16Float,
            max_msaa_samples: engine.caps().max_color_sample_count.max(1).min(16),
        }
    }

    pub fn format(&self) -> Format {
        self.format
    }

    pub fn max_msaa_samples(&self) -> u8 {
        self.max_msaa_samples
    }

    pub fn create(
        &self,
        frame: &RenderFrame,
        name: impl Into<String>,
        requested_msaa_samples: u8,
    ) -> Result<GraphImage> {
        let samples = requested_msaa_samples.clamp(1, self.max_msaa_samples);
        let base_name = name.into();
        if samples > 1 {
            frame.hdr_color_image_with_samples(format!("{base_name}_msaa"), samples)
        } else {
            frame.hdr_color_image(base_name)
        }
    }

    pub fn resolve(
        &self,
        frame: &RenderFrame,
        scene_target: &GraphImage,
        resolved_name: impl Into<String>,
    ) -> Result<GraphImage> {
        let _ = self;
        scene_target.resolve_msaa(frame, resolved_name)
    }
}

impl Default for RuntimeController {
    fn default() -> Self {
        Self::new(RuntimeSettingsSnapshot::default())
    }
}

fn surface_is_hdr(color_space: SurfaceColorSpace) -> bool {
    matches!(
        color_space,
        SurfaceColorSpace::ExtendedSrgbLinear
            | SurfaceColorSpace::Hdr10St2084
            | SurfaceColorSpace::Hdr10Hlg
    )
}
