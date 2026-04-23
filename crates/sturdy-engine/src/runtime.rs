//! First-party application runtime surface.
//!
//! This module defines the initial public API shape for the runtime shell work
//! described in the roadmap. The current implementation is intentionally thin:
//! it establishes the engine-owned types and access patterns without changing
//! the existing application shell behavior yet.

use crate::{Engine, Format, GraphImage, RenderFrame, Result, Surface, SurfaceImage};

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
}

impl AppRuntime {
    /// Create a runtime shell from an engine and surface.
    pub fn new(engine: Engine, surface: Surface) -> Self {
        Self {
            default_scene_target: DefaultSceneTargetConfig::new(&engine),
            engine,
            surface,
            controller: RuntimeController::default(),
        }
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

    /// Acquire the current swapchain image and begin a render frame for it.
    pub fn acquire_frame(&mut self) -> Result<AppRuntimeFrame<'_>> {
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

    /// Create a shell-frame wrapper for compatibility with the existing app shell.
    pub(crate) fn shell_frame(&self) -> crate::application::ShellFrame<'_> {
        crate::application::ShellFrame::new(
            self.render_frame.clone(),
            &self.surface_image,
            self.default_scene_target().clone(),
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
#[derive(Clone, Debug, Default)]
pub struct RuntimeController {
    settings: RuntimeSettingsSnapshot,
    diagnostics: RuntimeDiagnostics,
}

impl RuntimeController {
    /// Return the current runtime settings snapshot.
    pub fn settings(&self) -> RuntimeSettingsSnapshot {
        self.settings.clone()
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
        self.diagnostics.clone()
    }
}

/// Snapshot of the current runtime settings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeSettingsSnapshot;

/// Snapshot of runtime diagnostics made visible to applications.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeDiagnostics;

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
}
