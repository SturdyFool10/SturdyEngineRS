//! First-party application runtime surface.
//!
//! This module defines the initial public API shape for the runtime shell work
//! described in the roadmap. The current implementation is intentionally thin:
//! it establishes the engine-owned types and access patterns without changing
//! the existing application shell behavior yet.

use crate::{Engine, Result, Surface};

/// Engine-owned runtime shell state.
///
/// This is the long-term home for the common application runtime loop and
/// related engine-owned systems. The initial slice only establishes the public
/// type and the basic ownership model.
pub struct AppRuntime {
    engine: Engine,
    surface: Surface,
    controller: RuntimeController,
}

impl AppRuntime {
    /// Create a runtime shell from an engine and surface.
    pub fn new(engine: Engine, surface: Surface) -> Self {
        Self {
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

    /// Decompose the runtime into its current owned parts.
    pub fn into_parts(self) -> (Engine, Surface, RuntimeController) {
        (self.engine, self.surface, self.controller)
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
