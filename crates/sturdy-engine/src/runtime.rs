//! First-party application runtime surface.
//!
//! This module defines the initial public API shape for the runtime shell work
//! described in the roadmap. The current implementation is intentionally thin:
//! it establishes the engine-owned types and access patterns without changing
//! the existing application shell behavior yet.

use std::{
    collections::HashMap,
    fmt,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

use crate::{
    BackendKind, Engine, Error, Format, FrameClock, FrameTime, GraphImage, GraphReport,
    MotionVectorDebugPass, PlatformCapabilityState, RenderFrame, Result, Surface,
    SurfaceCapabilities, SurfaceColorSpace, SurfaceHdrPreference, SurfaceImage, SurfacePresentMode,
    SurfaceRecreateDesc, SurfaceSize, WindowCornerStyle, WindowMaterialKind,
    current_window_appearance_caps,
};

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum WindowMode {
    #[default]
    Windowed,
    BorderlessFullscreen,
}

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
    motion_debug_pass: MotionVectorDebugPass,
    frame_clock: FrameClock,
    frame_start: Option<Instant>,
}

impl AppRuntime {
    /// Create a runtime shell from an engine and surface.
    pub fn new(engine: Engine, surface: Surface) -> Self {
        //panic allowed, reason = "motion vector debug shader is built-in and should compile during runtime initialization"
        let motion_debug_pass = MotionVectorDebugPass::new(&engine)
            .expect("failed to compile motion vector debug shader");
        let runtime = Self {
            default_scene_target: DefaultSceneTargetConfig::new(&engine),
            debug_images: DebugImageRegistry::default(),
            motion_debug_pass,
            frame_clock: FrameClock::new(),
            frame_start: None,
            controller: RuntimeController::new(RuntimeSettingsSnapshot {
                backend: engine.backend_kind(),
                adapter_name: engine.adapter_name(),
                hdr_enabled: surface_is_hdr(surface.info().color_space),
                present_mode: None,
                surface_size: surface.info().size,
                window_title: "Sturdy Engine".to_string(),
                window_size: surface.info().size,
                window_position: None,
                window_mode: WindowMode::Windowed,
                window_decorations: true,
                window_resizable: false,
                window_maximized: false,
                window_always_on_top: false,
                window_corner_style: WindowCornerStyle::Default,
                ..RuntimeSettingsSnapshot::default()
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

    /// Return the runtime-owned frame clock.
    pub fn frame_clock(&self) -> &FrameClock {
        &self.frame_clock
    }

    /// Return the runtime-owned frame clock mutably.
    pub fn frame_clock_mut(&mut self) -> &mut FrameClock {
        &mut self.frame_clock
    }

    /// Return timing for the most recently acquired frame.
    pub fn frame_time(&self) -> FrameTime {
        self.frame_clock.time()
    }

    /// Refresh runtime settings/diagnostics snapshots from the current engine and surface state.
    pub fn refresh_controller_state(&self) {
        let surface_info = self.surface.info();
        let hdr_caps = self.surface.hdr_caps().ok();
        let surface_caps = self.surface.capabilities().ok();
        self.controller.set_settings(RuntimeSettingsSnapshot {
            backend: self.engine.backend_kind(),
            adapter_name: self.engine.adapter_name(),
            hdr_enabled: surface_is_hdr(surface_info.color_space),
            present_mode: None,
            surface_size: surface_info.size,
            ..self.controller.settings()
        });
        self.controller
            .sync_engine_capabilities(hdr_caps, surface_caps);
        self.controller.update_diagnostics(|diagnostics| {
            diagnostics.backend = self.engine.backend_kind();
            diagnostics.adapter_name = self.engine.adapter_name();
            diagnostics.surface_format = surface_info.format;
            diagnostics.surface_color_space = surface_info.color_space;
            diagnostics.hdr_output = surface_is_hdr(surface_info.color_space);
            diagnostics.present_mode = None;
        });
    }

    /// Apply runtime settings that require presentation surface recreation.
    ///
    /// The transaction itself records requested setting values. This method is
    /// the runtime-owned execution point for the `SurfaceRecreate` path so the
    /// application shell does not need to know HDR, present-mode, or alpha
    /// policy details.
    pub(crate) fn apply_surface_runtime_settings(
        &mut self,
        changes: &[RuntimeSettingChange],
    ) -> RuntimeApplyReport {
        let surface_changes = changes
            .iter()
            .filter(|change| {
                matches!(
                    change.setting,
                    RuntimeSettingId::Engine(RuntimeSettingKey::HdrMode)
                        | RuntimeSettingId::Engine(RuntimeSettingKey::PresentMode)
                        | RuntimeSettingId::Engine(RuntimeSettingKey::PresentPolicy)
                        | RuntimeSettingId::Engine(RuntimeSettingKey::SurfaceTransparency)
                )
            })
            .collect::<Vec<_>>();

        if surface_changes.is_empty() {
            return RuntimeApplyReport::default();
        }

        let hdr_preference = if self
            .controller
            .bool_setting(RuntimeSettingKey::HdrMode)
            .unwrap_or(false)
        {
            match self.surface.hdr_caps() {
                Ok(caps) if caps.sc_rgb => Some(SurfaceHdrPreference::ScRgb),
                Ok(caps) if caps.hdr10 => Some(SurfaceHdrPreference::Hdr10),
                _ => Some(SurfaceHdrPreference::Sdr),
            }
        } else {
            Some(SurfaceHdrPreference::Sdr)
        };

        let explicit_present_mode = self
            .controller
            .text_setting(RuntimeSettingKey::PresentMode)
            .and_then(|value| parse_present_mode_setting(&value));
        let preferred_present_mode = self
            .controller
            .text_setting(RuntimeSettingKey::PresentPolicy)
            .and_then(|value| parse_present_policy_setting(&value, explicit_present_mode))
            .or(explicit_present_mode);
        let transparent = self
            .controller
            .bool_setting(RuntimeSettingKey::SurfaceTransparency);
        let surface_size = self.surface.size();

        let recreate_result = self.surface.recreate(SurfaceRecreateDesc {
            size: Some(surface_size),
            transparent,
            hdr: hdr_preference,
            preferred_present_mode,
            ..SurfaceRecreateDesc::default()
        });

        let mut report = RuntimeApplyReport::default();
        match recreate_result {
            Ok(()) => {
                for change in surface_changes {
                    report.changes.push(RuntimeChangeResult::Applied {
                        setting: change.setting.clone(),
                        path: RuntimeApplyPath::SurfaceRecreate,
                    });
                }
                let context = runtime_surface_apply_context(
                    &self.controller,
                    changes,
                    surface_size,
                    "applied",
                );
                self.controller.update_diagnostics(|diagnostics| {
                    diagnostics.runtime_setting_apply = Some(context);
                });
                self.refresh_controller_state();
            }
            Err(error) => {
                let context = runtime_surface_apply_context(
                    &self.controller,
                    changes,
                    surface_size,
                    "failed",
                );
                let detail = format!(
                    "{context} error_category={:?} reason={error}",
                    error.category()
                );
                for change in surface_changes {
                    report.changes.push(RuntimeChangeResult::Failed {
                        setting: change.setting.clone(),
                        path: RuntimeApplyPath::SurfaceRecreate,
                        reason: detail.clone(),
                    });
                }
                self.controller.update_diagnostics(|diagnostics| {
                    diagnostics.runtime_setting_apply = Some(detail);
                });
            }
        }

        self.controller.record_runtime_apply_report(report.clone());
        report
    }

    /// Acquire the current swapchain image and begin a render frame for it.
    pub fn acquire_frame(&mut self) -> Result<AppRuntimeFrame<'_>> {
        self.refresh_controller_state();
        self.debug_images.clear();
        self.controller.clear_overlay_lines();
        self.frame_start = Some(Instant::now());
        let frame_time = self.frame_clock.tick();
        let surface_image = self.surface.acquire_image()?;
        let render_frame = self.engine.begin_render_frame_for(&surface_image)?;
        Ok(AppRuntimeFrame {
            runtime: self,
            surface_image,
            render_frame,
            frame_time,
            finished: false,
        })
    }

    /// Decompose the runtime into its current owned parts.
    pub fn into_parts(self) -> (Engine, Surface, RuntimeController) {
        (self.engine, self.surface, self.controller)
    }
}

fn runtime_surface_apply_context(
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
        "status={status} backend={:?} adapter={} surface={}x{} settings=[{}]",
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

fn parse_present_policy_setting(
    value: &str,
    explicit_present_mode: Option<SurfacePresentMode>,
) -> Option<SurfacePresentMode> {
    match value {
        "Auto" => None,
        "NoTear" => Some(SurfacePresentMode::Fifo),
        "LowLatencyNoTear" => Some(SurfacePresentMode::Mailbox),
        "LowLatencyAllowTear" => Some(SurfacePresentMode::RelaxedFifo),
        "Explicit" => explicit_present_mode,
        _ => None,
    }
}

/// Runtime-owned per-frame state for the currently acquired surface image.
pub struct AppRuntimeFrame<'a> {
    runtime: &'a mut AppRuntime,
    surface_image: SurfaceImage,
    render_frame: RenderFrame,
    frame_time: FrameTime,
    /// Set to `true` after `finish_and_present` completes to prevent the `Drop`
    /// impl from double-presenting when the user calls it explicitly.
    finished: bool,
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

    /// Timing for this acquired frame.
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

    /// Monotonic frame index for this runtime frame.
    pub fn frame_index(&self) -> u64 {
        self.frame_time.frame
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
        self.default_scene_target()
            .create(&self.render_frame, name, requested_msaa_samples)
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

    /// Look up a named debug image that was registered via [`DebugImageRegistry::register`].
    ///
    /// Returns `None` if the name was not registered in the current frame.
    /// Use the returned [`GraphImage`] with [`ScreenshotCapture::record_readback`] during
    /// frame recording, then call `flush()` + `wait()` before reading or saving pixels.
    pub fn find_debug_image(&self, name: &str) -> Option<GraphImage> {
        if !self.runtime.debug_images.names().iter().any(|n| n == name) {
            return None;
        }
        self.render_frame.find_image_by_name(name)
    }

    /// Create a shell-frame wrapper for compatibility with the existing app shell.
    pub(crate) fn shell_frame(&self) -> crate::application::ShellFrame<'_> {
        crate::application::ShellFrame::new(
            self.render_frame.clone(),
            self.default_scene_target().clone(),
            self.debug_images().clone(),
            self.runtime.controller.clone(),
            &self.runtime.motion_debug_pass,
            self.frame_time,
        )
    }

    /// Flush and present through the runtime-owned surface.
    ///
    /// Submits all queued GPU work and presents to the display. The CPU does not
    /// wait for the GPU to finish rendering — synchronisation is handled by the
    /// GPU's render-complete semaphore. The frames-in-flight fence is waited at
    /// the start of the *next* frame's submission, enabling CPU/GPU overlap.
    ///
    /// Records CPU-measured frame time into `RuntimeDiagnostics.timings`.
    pub fn finish_and_present(&mut self) -> Result<()> {
        self.finished = true;
        let flush_report = self
            .render_frame
            .flush_with_reason(crate::FrameSyncReason::FrameBoundaryPresent)?;
        self.runtime.surface.present()?;
        self.render_frame.mark_presented();
        self.runtime.controller.update_diagnostics(|d| {
            d.frame_sync = Some(format!(
                "reason={:?} submitted={} waited=false presented=true submission={:?}",
                flush_report.reason, flush_report.submitted, flush_report.submission
            ));
        });
        if let Some(start) = self.runtime.frame_start.take() {
            let elapsed_ms = start.elapsed().as_secs_f32() * 1000.0;
            self.runtime.controller.update_diagnostics(|d| {
                d.timings.available = true;
                d.timings.cpu_frame_time_ms = Some(elapsed_ms);
            });
        }
        Ok(())
    }
}

impl Drop for AppRuntimeFrame<'_> {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.finish_and_present();
        }
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
            shared: Arc::new(Mutex::new(RuntimeShared::new(settings))),
        }
    }

    /// Return the current runtime settings snapshot.
    pub fn settings(&self) -> RuntimeSettingsSnapshot {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
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

    /// Register an application-owned runtime setting.
    pub fn register_app_setting(
        &self,
        descriptor: RuntimeSettingDescriptor,
    ) -> Result<RuntimeSettingEntry> {
        let id = descriptor.id.clone();
        if !matches!(id, RuntimeSettingId::App(_)) {
            return Err(Error::InvalidInput(
                "application settings must use RuntimeSettingId::App".to_string(),
            ));
        }

        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut shared = self.shared.lock().expect("runtime controller poisoned");
        if shared.setting_entries.contains_key(&id) {
            return Err(Error::InvalidInput(format!(
                "runtime setting `{}` is already registered",
                id
            )));
        }
        let entry = RuntimeSettingEntry::new(RuntimeSettingSource::App, descriptor, 0);
        shared.setting_entries.insert(id.clone(), entry.clone());
        Ok(entry)
    }

    /// Return the current value of one runtime setting.
    pub fn setting_value(&self, id: impl Into<RuntimeSettingId>) -> Option<RuntimeSettingValue> {
        self.setting_entry(id).map(|entry| entry.value)
    }

    pub fn bool_setting(&self, id: impl Into<RuntimeSettingId>) -> Option<bool> {
        match self.setting_value(id)? {
            RuntimeSettingValue::Bool(value) => Some(value),
            _ => None,
        }
    }

    pub fn integer_setting(&self, id: impl Into<RuntimeSettingId>) -> Option<i64> {
        match self.setting_value(id)? {
            RuntimeSettingValue::Integer(value) => Some(value),
            _ => None,
        }
    }

    pub fn float_setting(&self, id: impl Into<RuntimeSettingId>) -> Option<f64> {
        match self.setting_value(id)? {
            RuntimeSettingValue::Float(value) => Some(value),
            _ => None,
        }
    }

    pub fn text_setting(&self, id: impl Into<RuntimeSettingId>) -> Option<String> {
        match self.setting_value(id)? {
            RuntimeSettingValue::Text(value) => Some(value),
            _ => None,
        }
    }

    /// Return one registered runtime setting, including menu metadata.
    pub fn setting_entry(&self, id: impl Into<RuntimeSettingId>) -> Option<RuntimeSettingEntry> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shared
            .lock()
            .expect("runtime controller poisoned")
            .setting_entries
            .get(&id.into())
            .cloned()
    }

    /// Return every registered runtime setting.
    pub fn setting_entries(&self) -> Vec<RuntimeSettingEntry> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut entries = self
            .shared
            .lock()
            .expect("runtime controller poisoned")
            .setting_entries
            .values()
            .cloned()
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.descriptor.label.cmp(&right.descriptor.label));
        entries
    }

    /// Return support/capability information for one runtime setting.
    pub fn setting_support(
        &self,
        id: impl Into<RuntimeSettingId>,
    ) -> Option<RuntimeSettingSupport> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shared
            .lock()
            .expect("runtime controller poisoned")
            .setting_entries
            .get(&id.into())
            .map(|entry| entry.support.clone())
    }

    /// Return support/capability information for all runtime settings.
    pub fn setting_supports(&self) -> Vec<(RuntimeSettingId, RuntimeSettingSupport)> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut supports = self
            .shared
            .lock()
            .expect("runtime controller poisoned")
            .setting_entries
            .iter()
            .map(|(id, entry)| (id.clone(), entry.support.clone()))
            .collect::<Vec<_>>();
        supports.sort_by(|left, right| left.0.label().cmp(&right.0.label()));
        supports
    }

    /// Return the current settings change serial.
    pub fn settings_revision(&self) -> u64 {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shared
            .lock()
            .expect("runtime controller poisoned")
            .settings_revision
    }

    /// Return every settings change recorded after `revision`.
    pub fn setting_changes_since(&self, revision: u64) -> Vec<RuntimeSettingChange> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shared
            .lock()
            .expect("runtime controller poisoned")
            .change_log
            .iter()
            .filter(|change| change.revision > revision)
            .cloned()
            .collect()
    }

    /// Return the current apply-notification revision.
    pub fn apply_notifications_revision(&self) -> u64 {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shared
            .lock()
            .expect("runtime controller poisoned")
            .apply_notifications_revision
    }

    /// Return apply notifications recorded after `revision`.
    ///
    /// Unlike `setting_changes_since`, this includes rejected requests and
    /// no-op accepted requests so applications can surface exact outcomes.
    pub fn apply_notifications_since(&self, revision: u64) -> Vec<RuntimeApplyNotification> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shared
            .lock()
            .expect("runtime controller poisoned")
            .apply_notifications
            .iter()
            .filter(|notification| notification.revision > revision)
            .cloned()
            .collect()
    }

    /// Return the most recent runtime apply report, if any transaction has run.
    pub fn last_apply_report(&self) -> Option<RuntimeApplyReport> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shared
            .lock()
            .expect("runtime controller poisoned")
            .last_apply_report
            .clone()
    }

    /// Return the current runtime diagnostics snapshot.
    ///
    /// The returned snapshot includes any active shader compile errors and asset
    /// diagnostics reported via `report_shader_compile_error` / `report_asset_state`.
    pub fn diagnostics(&self) -> RuntimeDiagnostics {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let shared = self.shared.lock().expect("runtime controller poisoned");
        let mut diag = shared.diagnostics.clone();
        diag.shader_compile_errors = shared
            .shader_compile_errors
            .iter()
            .map(|(path, msg)| ShaderCompileError {
                path: path.clone(),
                message: msg.clone(),
            })
            .collect();
        diag.shader_compile_errors
            .sort_by(|a, b| a.path.cmp(&b.path));
        diag.asset_diagnostics = shared
            .asset_states
            .iter()
            .filter(|(_, state)| !state.is_ok())
            .map(|(path, state)| AssetDiagnostic {
                path: path.clone(),
                state: state.clone(),
            })
            .collect();
        diag.asset_diagnostics.sort_by(|a, b| a.path.cmp(&b.path));
        diag
    }

    /// Report a shader compile error so it appears in `RuntimeDiagnostics`.
    ///
    /// Calling this with the same path replaces the previous error for that file.
    pub fn report_shader_compile_error(
        &self,
        path: impl Into<PathBuf>,
        message: impl Into<String>,
    ) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let _ = self
            .shared
            .lock()
            .expect("runtime controller poisoned")
            .shader_compile_errors
            .insert(path.into(), message.into());
    }

    /// Clear a previously-reported shader compile error after a successful reload.
    pub fn clear_shader_compile_error(&self, path: impl Into<PathBuf>) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let _ = self
            .shared
            .lock()
            .expect("runtime controller poisoned")
            .shader_compile_errors
            .remove(&path.into());
    }

    /// Clear all shader compile errors.
    pub fn clear_all_shader_compile_errors(&self) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shared
            .lock()
            .expect("runtime controller poisoned")
            .shader_compile_errors
            .clear();
    }

    /// Report or update the health state for a monitored asset path.
    ///
    /// `AssetState::Ok` entries are tracked internally but excluded from
    /// `diagnostics().asset_diagnostics` so only problems are surfaced.
    pub fn report_asset_state(&self, path: impl Into<PathBuf>, state: AssetState) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let _ = self
            .shared
            .lock()
            .expect("runtime controller poisoned")
            .asset_states
            .insert(path.into(), state);
    }

    /// Check whether a file path exists and report `Missing` or `Ok` accordingly.
    ///
    /// Call this on startup for every file asset your app depends on to get
    /// immediate feedback when a required file is absent.
    pub fn check_asset_path(&self, path: impl Into<PathBuf>) {
        let path = path.into();
        let state = if path.exists() {
            AssetState::Ok
        } else {
            AssetState::Missing
        };
        self.report_asset_state(path, state);
    }

    /// Check every path in `paths` and report their state.
    pub fn check_asset_paths<'a>(&self, paths: impl IntoIterator<Item = &'a std::path::Path>) {
        for path in paths {
            self.check_asset_path(path);
        }
    }

    /// Remove the tracked state for an asset path (stops monitoring it).
    pub fn unregister_asset_path(&self, path: impl Into<PathBuf>) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let _ = self
            .shared
            .lock()
            .expect("runtime controller poisoned")
            .asset_states
            .remove(&path.into());
    }

    /// Format a `GraphReport` as a multi-line human-readable string for debugging.
    ///
    /// Each pass is listed with its read and write image names. Use this with
    /// the text overlay or log output to inspect the current frame's render graph
    /// without launching an external tool.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let report = render_frame.describe();
    /// let text = controller.format_graph_report(&report);
    /// println!("{text}");
    /// ```
    pub fn format_graph_report(report: &GraphReport) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "Frame graph: {} passes, {} images\n",
            report.passes.len(),
            report.images.len()
        ));
        for (i, pass) in report.passes.iter().enumerate() {
            out.push_str(&format!("  [{i:02}] {:?} \"{}\"\n", pass.kind, pass.name));
            if !pass.reads.is_empty() {
                out.push_str(&format!("       reads:  {}\n", pass.reads.join(", ")));
            }
            if !pass.writes.is_empty() {
                out.push_str(&format!("       writes: {}\n", pass.writes.join(", ")));
            }
        }
        if !report.images.is_empty() {
            out.push_str("Images:\n");
            for img in &report.images {
                out.push_str(&format!(
                    "  {} {}x{}x{} {:?}  (w={}, r={})\n",
                    img.name,
                    img.extent.width,
                    img.extent.height,
                    img.extent.depth,
                    img.format,
                    img.write_count,
                    img.read_count
                ));
            }
        }
        out
    }

    /// Format a compact render-graph summary suitable for an in-app overlay.
    pub fn graph_inspection_lines(
        report: &GraphReport,
        max_passes: usize,
        max_images: usize,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(format!(
            "frame graph: {} passes, {} images",
            report.passes.len(),
            report.images.len()
        ));
        for (index, pass) in report.passes.iter().take(max_passes).enumerate() {
            let reads = if pass.reads.is_empty() {
                "-".to_string()
            } else {
                pass.reads.join(",")
            };
            let writes = if pass.writes.is_empty() {
                "-".to_string()
            } else {
                pass.writes.join(",")
            };
            lines.push(format!(
                "  pass {index:02}: {:?} {}  r=[{}] w=[{}]",
                pass.kind, pass.name, reads, writes
            ));
        }
        if report.passes.len() > max_passes {
            lines.push(format!(
                "  ... {} more passes",
                report.passes.len() - max_passes
            ));
        }
        for image in report.images.iter().take(max_images) {
            lines.push(format!(
                "  image: {} {}x{} {:?} w={} r={}",
                image.name,
                image.extent.width,
                image.extent.height,
                image.format,
                image.write_count,
                image.read_count
            ));
        }
        if report.images.len() > max_images {
            lines.push(format!(
                "  ... {} more images",
                report.images.len() - max_images
            ));
        }
        lines
    }

    /// Return the current overlay text lines.
    pub fn overlay_lines(&self) -> Vec<String> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shared
            .lock()
            .expect("runtime controller poisoned")
            .overlay_lines
            .clone()
    }

    pub(crate) fn set_settings(&self, settings: RuntimeSettingsSnapshot) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut shared = self.shared.lock().expect("runtime controller poisoned");
        shared.sync_engine_snapshot(&settings);
        shared.settings = settings;
    }

    pub(crate) fn sync_engine_capabilities(
        &self,
        hdr_caps: Option<crate::SurfaceHdrCaps>,
        surface_caps: Option<SurfaceCapabilities>,
    ) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shared
            .lock()
            .expect("runtime controller poisoned")
            .sync_engine_capabilities(hdr_caps, surface_caps.as_ref());
    }

    pub(crate) fn update_diagnostics(&self, f: impl FnOnce(&mut RuntimeDiagnostics)) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut shared = self.shared.lock().expect("runtime controller poisoned");
        f(&mut shared.diagnostics);
    }

    pub(crate) fn set_overlay_lines(&self, lines: Vec<String>) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shared
            .lock()
            .expect("runtime controller poisoned")
            .overlay_lines = lines;
    }

    pub(crate) fn clear_overlay_lines(&self) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shared
            .lock()
            .expect("runtime controller poisoned")
            .overlay_lines
            .clear();
    }

    pub(crate) fn record_runtime_apply_report(&self, report: RuntimeApplyReport) {
        if report.changes.is_empty() {
            return;
        }
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shared
            .lock()
            .expect("runtime controller poisoned")
            .record_apply_report(report);
    }
}

/// Snapshot of the current runtime settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSettingsSnapshot {
    pub backend: BackendKind,
    pub browser_backend: String,
    pub adapter_name: Option<String>,
    pub hdr_enabled: bool,
    pub present_mode: Option<SurfacePresentMode>,
    pub present_policy: String,
    pub latency_mode: String,
    pub frame_pacing_mode: String,
    pub max_frames_in_flight: u32,
    pub threaded_input_mode: String,
    pub render_threading_mode: String,
    pub surface_size: SurfaceSize,
    pub window_title: String,
    pub window_size: SurfaceSize,
    pub window_position: Option<(i32, i32)>,
    pub window_mode: WindowMode,
    pub window_decorations: bool,
    pub window_resizable: bool,
    pub window_maximized: bool,
    pub window_always_on_top: bool,
    pub window_corner_style: WindowCornerStyle,
}

impl Default for RuntimeSettingsSnapshot {
    fn default() -> Self {
        Self {
            backend: BackendKind::Auto,
            browser_backend: "Auto".to_string(),
            adapter_name: None,
            hdr_enabled: false,
            present_mode: None,
            present_policy: "Auto".to_string(),
            latency_mode: "Balanced".to_string(),
            frame_pacing_mode: "Auto".to_string(),
            max_frames_in_flight: 2,
            threaded_input_mode: "Auto".to_string(),
            render_threading_mode: "Auto".to_string(),
            surface_size: SurfaceSize {
                width: 1,
                height: 1,
            },
            window_title: "Sturdy Engine".to_string(),
            window_size: SurfaceSize {
                width: 1,
                height: 1,
            },
            window_position: None,
            window_mode: WindowMode::Windowed,
            window_decorations: true,
            window_resizable: false,
            window_maximized: false,
            window_always_on_top: false,
            window_corner_style: WindowCornerStyle::Default,
        }
    }
}

/// A shader compile error reported to the runtime for in-app display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderCompileError {
    /// The source file that failed to compile.
    pub path: PathBuf,
    /// The compiler diagnostic message.
    pub message: String,
}

/// The observed health of a monitored asset path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetState {
    /// The file exists and was loaded or verified successfully.
    Ok,
    /// The file does not exist on disk.
    Missing,
    /// The file exists but loading or reloading it failed.
    Stale(String),
}

impl AssetState {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok)
    }
    pub fn is_problem(&self) -> bool {
        !self.is_ok()
    }
}

/// Asset health report for one monitored path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetDiagnostic {
    /// The asset file path that was registered for monitoring.
    pub path: PathBuf,
    /// Current health of that asset.
    pub state: AssetState,
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
    pub motion_warning: Option<String>,
    pub native_window_appearance: Option<String>,
    pub windows: RuntimeWindowDiagnostics,
    pub runtime_setting_apply: Option<String>,
    pub frame_sync: Option<String>,
    pub user_diagnostics: Vec<RuntimeUserDiagnostic>,
    pub camera_locked_passes: Vec<String>,
    pub debug_images: Vec<String>,
    pub graph: RuntimeGraphDiagnostics,
    pub timings: RuntimeTimingSummary,
    /// Active shader compile errors reported via `RuntimeController::report_shader_compile_error`.
    pub shader_compile_errors: Vec<ShaderCompileError>,
    /// Asset paths that are missing or stale, surfaced via `RuntimeController::report_asset_state`.
    pub asset_diagnostics: Vec<AssetDiagnostic>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeWindowDiagnostics {
    pub live_count: usize,
    pub focused_window: Option<u64>,
    pub hovered_window: Option<u64>,
    pub dirty_count: usize,
    pub waiting_for_surface_recreation_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeUserDiagnostic {
    pub message: String,
    pub detail: Option<String>,
    pub setting: Option<RuntimeSettingId>,
}

/// Summary information about the currently recorded render graph.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeGraphDiagnostics {
    pub pass_count: usize,
    pub image_count: usize,
    pub warning_count: usize,
    pub error_count: usize,
}

/// Frame timing summary surfaced through runtime diagnostics.
///
/// `cpu_frame_time_ms` is populated automatically by `AppRuntimeFrame::finish_and_present`.
/// `gpu_frame_time_ms` and `pass_timings` require backend timer-query support (future work).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuntimeTimingSummary {
    pub available: bool,
    /// CPU-measured time from `acquire_frame` to `finish_and_present` in milliseconds.
    pub cpu_frame_time_ms: Option<f32>,
    /// GPU-measured whole-frame time (requires timer query backend support).
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
    setting_entries: HashMap<RuntimeSettingId, RuntimeSettingEntry>,
    settings_revision: u64,
    change_log: Vec<RuntimeSettingChange>,
    apply_notifications_revision: u64,
    apply_notifications: Vec<RuntimeApplyNotification>,
    last_apply_report: Option<RuntimeApplyReport>,
    shader_compile_errors: HashMap<PathBuf, String>,
    asset_states: HashMap<PathBuf, AssetState>,
}

impl RuntimeShared {
    fn new(settings: RuntimeSettingsSnapshot) -> Self {
        let mut shared = Self {
            settings: settings.clone(),
            diagnostics: RuntimeDiagnostics::default(),
            overlay_lines: Vec::new(),
            setting_entries: default_setting_entries(&settings),
            settings_revision: 0,
            change_log: Vec::new(),
            apply_notifications_revision: 0,
            apply_notifications: Vec::new(),
            last_apply_report: None,
            shader_compile_errors: HashMap::new(),
            asset_states: HashMap::new(),
        };
        shared.sync_engine_snapshot(&settings);
        shared.sync_engine_capabilities(None, None);
        shared
    }

    fn sync_engine_snapshot(&mut self, settings: &RuntimeSettingsSnapshot) {
        self.sync_engine_value(
            RuntimeSettingKey::BackendSelection,
            RuntimeSettingValue::Text(format!("{:?}", settings.backend)),
        );
        self.sync_engine_value(
            RuntimeSettingKey::BrowserBackendSelection,
            RuntimeSettingValue::Text(settings.browser_backend.clone()),
        );
        self.sync_engine_value(
            RuntimeSettingKey::AdapterSelection,
            RuntimeSettingValue::Text(
                settings
                    .adapter_name
                    .clone()
                    .unwrap_or_else(|| "Auto".to_string()),
            ),
        );
        self.sync_engine_value(
            RuntimeSettingKey::HdrMode,
            RuntimeSettingValue::Bool(settings.hdr_enabled),
        );
        self.sync_engine_value(
            RuntimeSettingKey::PresentMode,
            RuntimeSettingValue::Text(
                settings
                    .present_mode
                    .map(|mode| format!("{mode:?}"))
                    .unwrap_or_else(|| "Auto".to_string()),
            ),
        );
        self.sync_engine_value(
            RuntimeSettingKey::PresentPolicy,
            RuntimeSettingValue::Text(settings.present_policy.clone()),
        );
        self.sync_engine_value(
            RuntimeSettingKey::LatencyMode,
            RuntimeSettingValue::Text(settings.latency_mode.clone()),
        );
        self.sync_engine_value(
            RuntimeSettingKey::FramePacingMode,
            RuntimeSettingValue::Text(settings.frame_pacing_mode.clone()),
        );
        self.sync_engine_value(
            RuntimeSettingKey::MaxFramesInFlight,
            RuntimeSettingValue::Integer(settings.max_frames_in_flight as i64),
        );
        self.sync_engine_value(
            RuntimeSettingKey::ThreadedInputMode,
            RuntimeSettingValue::Text(settings.threaded_input_mode.clone()),
        );
        self.sync_engine_value(
            RuntimeSettingKey::RenderThreadingMode,
            RuntimeSettingValue::Text(settings.render_threading_mode.clone()),
        );
        self.sync_engine_value(
            RuntimeSettingKey::WindowTitle,
            RuntimeSettingValue::Text(settings.window_title.clone()),
        );
        self.sync_engine_value(
            RuntimeSettingKey::WindowWidth,
            RuntimeSettingValue::Integer(settings.window_size.width as i64),
        );
        self.sync_engine_value(
            RuntimeSettingKey::WindowHeight,
            RuntimeSettingValue::Integer(settings.window_size.height as i64),
        );
        self.sync_engine_value(
            RuntimeSettingKey::WindowPositionX,
            RuntimeSettingValue::Integer(
                settings.window_position.map(|(x, _)| x as i64).unwrap_or(0),
            ),
        );
        self.sync_engine_value(
            RuntimeSettingKey::WindowPositionY,
            RuntimeSettingValue::Integer(
                settings.window_position.map(|(_, y)| y as i64).unwrap_or(0),
            ),
        );
        self.sync_engine_value(
            RuntimeSettingKey::WindowMode,
            RuntimeSettingValue::Text(window_mode_setting_name(settings.window_mode).to_string()),
        );
        self.sync_engine_value(
            RuntimeSettingKey::WindowDecorations,
            RuntimeSettingValue::Bool(settings.window_decorations),
        );
        self.sync_engine_value(
            RuntimeSettingKey::WindowResizable,
            RuntimeSettingValue::Bool(settings.window_resizable),
        );
        self.sync_engine_value(
            RuntimeSettingKey::WindowMaximized,
            RuntimeSettingValue::Bool(settings.window_maximized),
        );
        self.sync_engine_value(
            RuntimeSettingKey::WindowAlwaysOnTop,
            RuntimeSettingValue::Bool(settings.window_always_on_top),
        );
        self.sync_engine_value(
            RuntimeSettingKey::WindowCornerStyle,
            RuntimeSettingValue::Text(
                window_corner_style_setting_name(settings.window_corner_style).to_string(),
            ),
        );
    }

    fn sync_engine_capabilities(
        &mut self,
        hdr_caps: Option<crate::SurfaceHdrCaps>,
        surface_caps: Option<&SurfaceCapabilities>,
    ) {
        if let Some(entry) = self
            .setting_entries
            .get_mut(&RuntimeSettingId::from(RuntimeSettingKey::HdrMode))
        {
            let hdr_available = hdr_caps
                .map(|caps| caps.hdr10 || caps.sc_rgb)
                .unwrap_or(false);
            entry.descriptor.options = bool_options(if hdr_available {
                &[false, true]
            } else {
                &[false]
            });
            entry.support = if hdr_available {
                RuntimeSettingSupport::supported()
            } else {
                RuntimeSettingSupport::unsupported(
                    "HDR output is unavailable on the current surface".to_string(),
                )
            };
        }

        if let Some(entry) = self
            .setting_entries
            .get_mut(&RuntimeSettingId::from(RuntimeSettingKey::PresentMode))
        {
            if let Some(surface_caps) = surface_caps {
                let mut options = vec![RuntimeSettingOption {
                    value: RuntimeSettingValue::Text("Auto".to_string()),
                    label: "Auto".to_string(),
                }];
                options.extend(surface_caps.present_modes.iter().map(|mode| {
                    RuntimeSettingOption {
                        value: RuntimeSettingValue::Text(
                            surface_present_mode_name(*mode).to_string(),
                        ),
                        label: format!("{mode:?}"),
                    }
                }));
                entry.descriptor.options = options;
                entry.support = RuntimeSettingSupport::supported();
            } else {
                entry.support = RuntimeSettingSupport::unsupported(
                    "surface present modes could not be queried".to_string(),
                );
            }
        }

        let appearance_caps = current_window_appearance_caps();
        if let Some(entry) = self.setting_entries.get_mut(&RuntimeSettingId::from(
            RuntimeSettingKey::SurfaceTransparency,
        )) {
            entry.support = capability_state_to_support(
                appearance_caps.transparency,
                "runtime surface transparency changes are unavailable on this platform",
            );
        }
        if let Some(entry) = self.setting_entries.get_mut(&RuntimeSettingId::from(
            RuntimeSettingKey::WindowBackgroundEffect,
        )) {
            let mut options = vec![RuntimeSettingOption {
                value: RuntimeSettingValue::Text("None".to_string()),
                label: "None".to_string(),
            }];
            if appearance_caps
                .transparency
                .is_some_and(is_capability_supported)
            {
                options.push(RuntimeSettingOption {
                    value: RuntimeSettingValue::Text("Transparent".to_string()),
                    label: "Transparent".to_string(),
                });
            }
            if appearance_caps.blur.is_some_and(is_capability_supported) {
                options.push(RuntimeSettingOption {
                    value: RuntimeSettingValue::Text("Blur".to_string()),
                    label: "Blur".to_string(),
                });
            }
            for material in &appearance_caps.materials {
                options.push(RuntimeSettingOption {
                    value: RuntimeSettingValue::Text(
                        window_material_setting_name(material.kind).to_string(),
                    ),
                    label: window_material_setting_name(material.kind).to_string(),
                });
            }
            entry.descriptor.options = options;
            let has_effects = appearance_caps.blur.is_some_and(is_capability_supported)
                || !appearance_caps.materials.is_empty();
            entry.support = if has_effects {
                RuntimeSettingSupport::supported()
            } else {
                RuntimeSettingSupport::unsupported(
                    "window background effects are unavailable on this platform".to_string(),
                )
            };
        }
        if let Some(entry) = self.setting_entries.get_mut(&RuntimeSettingId::from(
            RuntimeSettingKey::WindowCornerStyle,
        )) {
            entry.support = capability_state_to_support(
                appearance_caps.corner_style,
                "window corner style changes are unavailable on this platform",
            );
        }

        self.set_unsupported(
            RuntimeSettingKey::BackendSelection,
            "live backend migration is not implemented yet",
        );
        if let Some(entry) = self.setting_entries.get_mut(&RuntimeSettingId::from(
            RuntimeSettingKey::BrowserBackendSelection,
        )) {
            entry.support = if cfg!(target_arch = "wasm32") {
                RuntimeSettingSupport::supported()
            } else {
                RuntimeSettingSupport::unsupported(
                    "browser backend selection is only available on browser/WebAssembly targets"
                        .to_string(),
                )
            };
        }
        self.set_unsupported(
            RuntimeSettingKey::AdapterSelection,
            "live adapter migration is not implemented yet",
        );
        self.set_unsupported(
            RuntimeSettingKey::ShaderHotReloadPolicy,
            "shader hot reload policy changes are not implemented yet",
        );
        self.set_unsupported(
            RuntimeSettingKey::AssetHotReloadPolicy,
            "asset hot reload policy changes are not implemented yet",
        );
    }

    fn set_unsupported(&mut self, setting: RuntimeSettingKey, reason: &str) {
        if let Some(entry) = self
            .setting_entries
            .get_mut(&RuntimeSettingId::from(setting))
        {
            entry.support = RuntimeSettingSupport::unsupported(reason.to_string());
        }
    }

    fn sync_engine_value(&mut self, setting: RuntimeSettingKey, value: RuntimeSettingValue) {
        let id = RuntimeSettingId::from(setting);
        if let Some(entry) = self.setting_entries.get_mut(&id) {
            entry.value = value;
        }
    }

    fn apply_value(
        &mut self,
        id: RuntimeSettingId,
        value: RuntimeSettingValue,
    ) -> RuntimeChangeResult {
        let Some(entry) = self.setting_entries.get_mut(&id) else {
            return RuntimeChangeResult::Rejected {
                setting: id,
                reason: "setting is not registered".to_string(),
            };
        };

        if !entry.support.is_supported {
            return RuntimeChangeResult::Unavailable {
                setting: id,
                path: Some(entry.descriptor.apply_path),
                reason: entry
                    .support
                    .reason
                    .clone()
                    .unwrap_or_else(|| "setting is unsupported on the current runtime".to_string()),
            };
        }

        if !entry.descriptor.accepts_value(&value) {
            return RuntimeChangeResult::Rejected {
                setting: id,
                reason: format!(
                    "value `{}` does not match setting schema",
                    value.serialized()
                ),
            };
        }

        let path = entry.descriptor.apply_path;
        let (applied_value, clamp_reason) = clamp_runtime_setting_value(&id, value);

        if entry.value == applied_value {
            return match clamp_reason {
                Some(reason) => RuntimeChangeResult::Clamped {
                    setting: id,
                    path,
                    value: entry.value.serialized(),
                    reason,
                },
                None => RuntimeChangeResult::Exact { setting: id, path },
            };
        }

        entry.value = applied_value.clone();
        self.settings_revision += 1;
        entry.revision = self.settings_revision;
        self.change_log.push(RuntimeSettingChange {
            setting: id.clone(),
            value: applied_value.clone(),
            path,
            revision: self.settings_revision,
        });
        if self.change_log.len() > 256 {
            let excess = self.change_log.len() - 256;
            self.change_log.drain(0..excess);
        }

        match clamp_reason {
            Some(reason) => RuntimeChangeResult::Clamped {
                setting: id,
                path,
                value: applied_value.serialized(),
                reason,
            },
            None => RuntimeChangeResult::Exact { setting: id, path },
        }
    }

    fn record_apply_report(&mut self, report: RuntimeApplyReport) {
        for result in &report.changes {
            self.apply_notifications_revision += 1;
            self.apply_notifications.push(RuntimeApplyNotification {
                revision: self.apply_notifications_revision,
                result: result.clone(),
            });
            if let Some(diagnostic) = result.user_diagnostic() {
                self.diagnostics.user_diagnostics.push(diagnostic);
            }
        }
        if self.apply_notifications.len() > 256 {
            let excess = self.apply_notifications.len() - 256;
            self.apply_notifications.drain(0..excess);
        }
        if self.diagnostics.user_diagnostics.len() > 64 {
            let excess = self.diagnostics.user_diagnostics.len() - 64;
            self.diagnostics.user_diagnostics.drain(0..excess);
        }
        self.last_apply_report = Some(report);
    }
}

fn clamp_runtime_setting_value(
    id: &RuntimeSettingId,
    value: RuntimeSettingValue,
) -> (RuntimeSettingValue, Option<String>) {
    match (id, value) {
        (
            RuntimeSettingId::Engine(RuntimeSettingKey::MaxFramesInFlight),
            RuntimeSettingValue::Integer(requested),
        ) => {
            const MIN_FRAMES_IN_FLIGHT: i64 = 1;
            const MAX_FRAMES_IN_FLIGHT: i64 = 8;
            let clamped = requested.clamp(MIN_FRAMES_IN_FLIGHT, MAX_FRAMES_IN_FLIGHT);
            if clamped == requested {
                (RuntimeSettingValue::Integer(requested), None)
            } else {
                (
                    RuntimeSettingValue::Integer(clamped),
                    Some(format!(
                        "requested {requested}, allowed range is {MIN_FRAMES_IN_FLIGHT}..={MAX_FRAMES_IN_FLIGHT}"
                    )),
                )
            }
        }
        (_, value) => (value, None),
    }
}

/// Runtime-owned registry of named graph images exposed for inspection/debugging.
#[derive(Clone, Debug, Default)]
pub struct DebugImageRegistry {
    names: Arc<Mutex<Vec<String>>>,
}

impl DebugImageRegistry {
    pub fn clear(&self) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.names
            .lock()
            .expect("debug image registry poisoned")
            .clear();
    }

    pub fn register(&self, image: &GraphImage, name: impl Into<String>) {
        let name = name.into();
        image.register_as(name.clone());
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut names = self.names.lock().expect("debug image registry poisoned");
        if !names.iter().any(|existing| existing == &name) {
            names.push(name);
        }
    }

    pub fn names(&self) -> Vec<String> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
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

impl RuntimeApplyPath {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Immediate => "immediate",
            Self::GraphRebuild => "graph_rebuild",
            Self::SurfaceRecreate => "surface_recreate",
            Self::WindowReconfigure => "window_reconfigure",
            Self::DeviceMigration => "device_migration",
        }
    }
}

impl fmt::Display for RuntimeApplyPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Identifier for one runtime setting, including app-defined settings.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RuntimeSettingId {
    Engine(RuntimeSettingKey),
    App(String),
}

impl RuntimeSettingId {
    pub fn app(name: impl Into<String>) -> Self {
        Self::App(name.into())
    }

    pub fn label(&self) -> String {
        match self {
            Self::Engine(setting) => setting.label().to_string(),
            Self::App(name) => name.clone(),
        }
    }
}

impl fmt::Display for RuntimeSettingId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Engine(setting) => write!(f, "engine:{}", setting.name()),
            Self::App(name) => write!(f, "app:{name}"),
        }
    }
}

impl From<RuntimeSettingKey> for RuntimeSettingId {
    fn from(value: RuntimeSettingKey) -> Self {
        Self::Engine(value)
    }
}

/// Serialized value used by both engine and application-defined settings.
#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeSettingValue {
    Bool(bool),
    Integer(i64),
    Float(f64),
    Text(String),
}

impl RuntimeSettingValue {
    pub fn serialized(&self) -> String {
        match self {
            Self::Bool(value) => value.to_string(),
            Self::Integer(value) => value.to_string(),
            Self::Float(value) => value.to_string(),
            Self::Text(value) => value.clone(),
        }
    }
}

impl fmt::Display for RuntimeSettingValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.serialized())
    }
}

impl From<bool> for RuntimeSettingValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for RuntimeSettingValue {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<f64> for RuntimeSettingValue {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<String> for RuntimeSettingValue {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for RuntimeSettingValue {
    fn from(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}

/// Optional menu metadata for enumerated settings.
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeSettingOption {
    pub value: RuntimeSettingValue,
    pub label: String,
}

/// Setting definition shared by engine and application settings.
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeSettingDescriptor {
    pub id: RuntimeSettingId,
    pub label: String,
    pub description: Option<String>,
    pub apply_path: RuntimeApplyPath,
    pub default_value: RuntimeSettingValue,
    pub options: Vec<RuntimeSettingOption>,
}

impl RuntimeSettingDescriptor {
    pub fn new(
        id: impl Into<RuntimeSettingId>,
        label: impl Into<String>,
        apply_path: RuntimeApplyPath,
        default_value: impl Into<RuntimeSettingValue>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
            apply_path,
            default_value: default_value.into(),
            options: Vec::new(),
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_options(mut self, options: Vec<RuntimeSettingOption>) -> Self {
        self.options = options;
        self
    }

    fn accepts_value(&self, value: &RuntimeSettingValue) -> bool {
        let same_kind =
            std::mem::discriminant(&self.default_value) == std::mem::discriminant(value);
        same_kind
            && (self.options.is_empty() || self.options.iter().any(|option| option.value == *value))
    }
}

/// Runtime-visible state for one registered setting.
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeSettingEntry {
    pub descriptor: RuntimeSettingDescriptor,
    pub value: RuntimeSettingValue,
    pub source: RuntimeSettingSource,
    pub support: RuntimeSettingSupport,
    pub revision: u64,
}

impl RuntimeSettingEntry {
    fn new(
        source: RuntimeSettingSource,
        descriptor: RuntimeSettingDescriptor,
        revision: u64,
    ) -> Self {
        Self {
            value: descriptor.default_value.clone(),
            descriptor,
            source,
            support: RuntimeSettingSupport::supported(),
            revision,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSettingSupport {
    pub is_supported: bool,
    pub reason: Option<String>,
}

impl RuntimeSettingSupport {
    pub fn supported() -> Self {
        Self {
            is_supported: true,
            reason: None,
        }
    }

    pub fn unsupported(reason: String) -> Self {
        Self {
            is_supported: false,
            reason: Some(reason),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RuntimeSettingSource {
    Engine,
    App,
}

/// Recorded runtime setting change that systems can poll and react to incrementally.
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeSettingChange {
    pub setting: RuntimeSettingId,
    pub value: RuntimeSettingValue,
    pub path: RuntimeApplyPath,
    pub revision: u64,
}

/// Identifier for a runtime-facing setting.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum RuntimeSettingKey {
    BackendSelection,
    BrowserBackendSelection,
    AdapterSelection,
    HdrMode,
    PresentMode,
    PresentPolicy,
    LatencyMode,
    FramePacingMode,
    MaxFramesInFlight,
    ThreadedInputMode,
    RenderThreadingMode,
    WindowTitle,
    WindowWidth,
    WindowHeight,
    WindowPositionX,
    WindowPositionY,
    WindowMode,
    WindowDecorations,
    WindowResizable,
    WindowMaximized,
    WindowAlwaysOnTop,
    WindowCornerStyle,
    SurfaceTransparency,
    WindowBackgroundEffect,
    AntiAliasingMode,
    AntiAliasingDial,
    BloomEnabled,
    BloomOnly,
    ToneMappingOperator,
    ToneMappingDial,
    MotionDebugView,
    OverlayVisibility,
    ShaderHotReloadPolicy,
    AssetHotReloadPolicy,
}

impl RuntimeSettingKey {
    pub const fn known_settings() -> &'static [RuntimeSettingKey] {
        &[
            Self::BackendSelection,
            Self::BrowserBackendSelection,
            Self::AdapterSelection,
            Self::HdrMode,
            Self::PresentMode,
            Self::PresentPolicy,
            Self::LatencyMode,
            Self::FramePacingMode,
            Self::MaxFramesInFlight,
            Self::ThreadedInputMode,
            Self::RenderThreadingMode,
            Self::WindowTitle,
            Self::WindowWidth,
            Self::WindowHeight,
            Self::WindowPositionX,
            Self::WindowPositionY,
            Self::WindowMode,
            Self::WindowDecorations,
            Self::WindowResizable,
            Self::WindowMaximized,
            Self::WindowAlwaysOnTop,
            Self::WindowCornerStyle,
            Self::SurfaceTransparency,
            Self::WindowBackgroundEffect,
            Self::AntiAliasingMode,
            Self::AntiAliasingDial,
            Self::BloomEnabled,
            Self::BloomOnly,
            Self::ToneMappingOperator,
            Self::ToneMappingDial,
            Self::MotionDebugView,
            Self::OverlayVisibility,
            Self::ShaderHotReloadPolicy,
            Self::AssetHotReloadPolicy,
        ]
    }

    pub const fn apply_path(self) -> RuntimeApplyPath {
        match self {
            Self::BackendSelection | Self::BrowserBackendSelection | Self::AdapterSelection => {
                RuntimeApplyPath::DeviceMigration
            }
            Self::HdrMode | Self::PresentMode | Self::SurfaceTransparency => {
                RuntimeApplyPath::SurfaceRecreate
            }
            Self::PresentPolicy => RuntimeApplyPath::SurfaceRecreate,
            Self::WindowTitle
            | Self::WindowWidth
            | Self::WindowHeight
            | Self::WindowPositionX
            | Self::WindowPositionY
            | Self::WindowMode
            | Self::WindowDecorations
            | Self::WindowResizable
            | Self::WindowMaximized
            | Self::WindowAlwaysOnTop
            | Self::WindowCornerStyle
            | Self::WindowBackgroundEffect => RuntimeApplyPath::WindowReconfigure,
            Self::AntiAliasingMode | Self::ShaderHotReloadPolicy | Self::AssetHotReloadPolicy => {
                RuntimeApplyPath::GraphRebuild
            }
            Self::AntiAliasingDial
            | Self::BloomEnabled
            | Self::BloomOnly
            | Self::ToneMappingOperator
            | Self::ToneMappingDial
            | Self::MotionDebugView
            | Self::OverlayVisibility
            | Self::LatencyMode
            | Self::FramePacingMode
            | Self::MaxFramesInFlight
            | Self::ThreadedInputMode
            | Self::RenderThreadingMode => RuntimeApplyPath::Immediate,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::BackendSelection => "backend_selection",
            Self::BrowserBackendSelection => "browser_backend_selection",
            Self::AdapterSelection => "adapter_selection",
            Self::HdrMode => "hdr_mode",
            Self::PresentMode => "present_mode",
            Self::PresentPolicy => "present_policy",
            Self::LatencyMode => "latency_mode",
            Self::FramePacingMode => "frame_pacing_mode",
            Self::MaxFramesInFlight => "max_frames_in_flight",
            Self::ThreadedInputMode => "threaded_input_mode",
            Self::RenderThreadingMode => "render_threading_mode",
            Self::WindowTitle => "window_title",
            Self::WindowWidth => "window_width",
            Self::WindowHeight => "window_height",
            Self::WindowPositionX => "window_position_x",
            Self::WindowPositionY => "window_position_y",
            Self::WindowMode => "window_mode",
            Self::WindowDecorations => "window_decorations",
            Self::WindowResizable => "window_resizable",
            Self::WindowMaximized => "window_maximized",
            Self::WindowAlwaysOnTop => "window_always_on_top",
            Self::WindowCornerStyle => "window_corner_style",
            Self::SurfaceTransparency => "surface_transparency",
            Self::WindowBackgroundEffect => "window_background_effect",
            Self::AntiAliasingMode => "anti_aliasing_mode",
            Self::AntiAliasingDial => "anti_aliasing_dial",
            Self::BloomEnabled => "bloom_enabled",
            Self::BloomOnly => "bloom_only",
            Self::ToneMappingOperator => "tone_mapping_operator",
            Self::ToneMappingDial => "tone_mapping_dial",
            Self::MotionDebugView => "motion_debug_view",
            Self::OverlayVisibility => "overlay_visibility",
            Self::ShaderHotReloadPolicy => "shader_hot_reload_policy",
            Self::AssetHotReloadPolicy => "asset_hot_reload_policy",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::BackendSelection => "backend selection",
            Self::BrowserBackendSelection => "browser backend selection",
            Self::AdapterSelection => "adapter selection",
            Self::HdrMode => "hdr mode",
            Self::PresentMode => "present mode",
            Self::PresentPolicy => "present policy",
            Self::LatencyMode => "latency mode",
            Self::FramePacingMode => "frame pacing mode",
            Self::MaxFramesInFlight => "max frames in flight",
            Self::ThreadedInputMode => "threaded input mode",
            Self::RenderThreadingMode => "render threading mode",
            Self::WindowTitle => "window title",
            Self::WindowWidth => "window width",
            Self::WindowHeight => "window height",
            Self::WindowPositionX => "window position x",
            Self::WindowPositionY => "window position y",
            Self::WindowMode => "window mode",
            Self::WindowDecorations => "window decorations",
            Self::WindowResizable => "window resizable",
            Self::WindowMaximized => "window maximized",
            Self::WindowAlwaysOnTop => "window always on top",
            Self::WindowCornerStyle => "window corner style",
            Self::SurfaceTransparency => "surface transparency",
            Self::WindowBackgroundEffect => "window background effect",
            Self::AntiAliasingMode => "anti-aliasing mode",
            Self::AntiAliasingDial => "anti-aliasing dial",
            Self::BloomEnabled => "bloom enabled",
            Self::BloomOnly => "bloom only",
            Self::ToneMappingOperator => "tone-mapping operator",
            Self::ToneMappingDial => "tone-mapping dial",
            Self::MotionDebugView => "motion debug view",
            Self::OverlayVisibility => "overlay visibility",
            Self::ShaderHotReloadPolicy => "shader hot-reload policy",
            Self::AssetHotReloadPolicy => "asset hot-reload policy",
        }
    }
}

/// Result of applying a set of runtime changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeApplyReport {
    pub changes: Vec<RuntimeChangeResult>,
}

/// Pollable notification for one runtime apply outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeApplyNotification {
    pub revision: u64,
    pub result: RuntimeChangeResult,
}

/// Outcome for an individual runtime-setting request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeChangeResult {
    Exact {
        setting: RuntimeSettingId,
        path: RuntimeApplyPath,
    },
    Applied {
        setting: RuntimeSettingId,
        path: RuntimeApplyPath,
    },
    Clamped {
        setting: RuntimeSettingId,
        path: RuntimeApplyPath,
        value: String,
        reason: String,
    },
    Degraded {
        setting: RuntimeSettingId,
        path: RuntimeApplyPath,
        reason: String,
    },
    Rejected {
        setting: RuntimeSettingId,
        reason: String,
    },
    Unavailable {
        setting: RuntimeSettingId,
        path: Option<RuntimeApplyPath>,
        reason: String,
    },
    Failed {
        setting: RuntimeSettingId,
        path: RuntimeApplyPath,
        reason: String,
    },
}

impl RuntimeChangeResult {
    pub fn user_diagnostic(&self) -> Option<RuntimeUserDiagnostic> {
        match self {
            Self::Exact { .. } | Self::Applied { .. } => None,
            Self::Clamped {
                setting,
                value,
                reason,
                ..
            } => Some(RuntimeUserDiagnostic {
                message: format!("{} was clamped to {}.", setting.label(), value),
                detail: Some(reason.clone()),
                setting: Some(setting.clone()),
            }),
            Self::Degraded {
                setting, reason, ..
            } => Some(RuntimeUserDiagnostic {
                message: format!("{} was applied with a fallback.", setting.label()),
                detail: Some(reason.clone()),
                setting: Some(setting.clone()),
            }),
            Self::Rejected { setting, reason } => Some(RuntimeUserDiagnostic {
                message: format!(
                    "{} was not changed because the requested value is invalid.",
                    setting.label()
                ),
                detail: Some(reason.clone()),
                setting: Some(setting.clone()),
            }),
            Self::Unavailable {
                setting, reason, ..
            } => Some(RuntimeUserDiagnostic {
                message: format!("{} is unavailable in this runtime.", setting.label()),
                detail: Some(reason.clone()),
                setting: Some(setting.clone()),
            }),
            Self::Failed {
                setting, reason, ..
            } => Some(RuntimeUserDiagnostic {
                message: format!("{} could not be applied.", setting.label()),
                detail: Some(reason.clone()),
                setting: Some(setting.clone()),
            }),
        }
    }
}

/// Mutable transaction over runtime settings.
pub struct RuntimeSettingsTransaction<'a> {
    controller: &'a mut RuntimeController,
    pending: Vec<RuntimePendingSettingChange>,
}

#[derive(Clone, Debug)]
enum RuntimePendingSettingChange {
    Note(RuntimeSettingId),
    Set {
        setting: RuntimeSettingId,
        value: RuntimeSettingValue,
    },
}

impl<'a> RuntimeSettingsTransaction<'a> {
    /// Record a placeholder change request.
    pub fn note_change(mut self, setting: RuntimeSettingKey) -> Self {
        self.pending
            .push(RuntimePendingSettingChange::Note(setting.into()));
        self
    }

    /// Update an engine-owned runtime setting.
    pub fn set_engine_value(
        mut self,
        setting: RuntimeSettingKey,
        value: impl Into<RuntimeSettingValue>,
    ) -> Self {
        self.pending.push(RuntimePendingSettingChange::Set {
            setting: setting.into(),
            value: value.into(),
        });
        self
    }

    /// Update an application-owned runtime setting.
    pub fn set_app_value(
        mut self,
        setting: impl Into<String>,
        value: impl Into<RuntimeSettingValue>,
    ) -> Self {
        self.pending.push(RuntimePendingSettingChange::Set {
            setting: RuntimeSettingId::app(setting),
            value: value.into(),
        });
        self
    }

    /// Apply the pending transaction.
    pub fn apply(self) -> Result<RuntimeApplyReport> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut shared = self
            .controller
            .shared
            .lock()
            .expect("runtime controller poisoned");
        let mut report = RuntimeApplyReport::default();
        for pending in self.pending {
            let result = match pending {
                RuntimePendingSettingChange::Note(setting) => {
                    match shared.setting_entries.get(&setting) {
                        Some(entry) => RuntimeChangeResult::Exact {
                            setting,
                            path: entry.descriptor.apply_path,
                        },
                        None => RuntimeChangeResult::Rejected {
                            setting,
                            reason: "setting is not registered".to_string(),
                        },
                    }
                }
                RuntimePendingSettingChange::Set { setting, value } => {
                    shared.apply_value(setting, value)
                }
            };
            report.changes.push(result);
        }
        shared.record_apply_report(report.clone());
        Ok(report)
    }
}

fn default_setting_entries(
    settings: &RuntimeSettingsSnapshot,
) -> HashMap<RuntimeSettingId, RuntimeSettingEntry> {
    let descriptors = [
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::BackendSelection,
            "Graphics API",
            RuntimeSettingKey::BackendSelection.apply_path(),
            format!("{:?}", settings.backend),
        )
        .with_description("Select the runtime graphics backend.")
        .with_options(text_options(&["Auto", "Vulkan"])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::BrowserBackendSelection,
            "Browser Graphics API",
            RuntimeSettingKey::BrowserBackendSelection.apply_path(),
            settings.browser_backend.clone(),
        )
        .with_description("Select the browser graphics backend when targeting WebAssembly.")
        .with_options(text_options(&["Auto", "WebGPU"])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::AdapterSelection,
            "Graphics Adapter",
            RuntimeSettingKey::AdapterSelection.apply_path(),
            settings
                .adapter_name
                .clone()
                .unwrap_or_else(|| "Auto".to_string()),
        )
        .with_description("Select the physical adapter used by the runtime."),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::HdrMode,
            "HDR Output",
            RuntimeSettingKey::HdrMode.apply_path(),
            settings.hdr_enabled,
        )
        .with_description("Enable or disable HDR output when the surface supports it.")
        .with_options(bool_options(&[false, true])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::PresentMode,
            "Present Mode",
            RuntimeSettingKey::PresentMode.apply_path(),
            settings
                .present_mode
                .map(|mode| format!("{mode:?}"))
                .unwrap_or_else(|| "Auto".to_string()),
        )
        .with_options(vec![RuntimeSettingOption {
            value: RuntimeSettingValue::Text("Auto".to_string()),
            label: "Auto".to_string(),
        }]),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::PresentPolicy,
            "Present Policy",
            RuntimeSettingKey::PresentPolicy.apply_path(),
            settings.present_policy.clone(),
        )
        .with_description("Select a high-level presentation policy above raw present modes.")
        .with_options(text_options(&[
            "Auto",
            "NoTear",
            "LowLatencyNoTear",
            "LowLatencyAllowTear",
            "Explicit",
        ])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::LatencyMode,
            "Latency Mode",
            RuntimeSettingKey::LatencyMode.apply_path(),
            settings.latency_mode.clone(),
        )
        .with_description("Select the runtime latency preset.")
        .with_options(text_options(&[
            "Throughput",
            "Balanced",
            "LowLatency",
            "UltraLowLatency",
        ])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::FramePacingMode,
            "Frame Pacing Mode",
            RuntimeSettingKey::FramePacingMode.apply_path(),
            settings.frame_pacing_mode.clone(),
        )
        .with_description("Select the frame pacing policy.")
        .with_options(text_options(&[
            "Auto",
            "Unlimited",
            "FixedFps",
            "VsyncPaced",
        ])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::MaxFramesInFlight,
            "Max Frames In Flight",
            RuntimeSettingKey::MaxFramesInFlight.apply_path(),
            settings.max_frames_in_flight as i64,
        )
        .with_description("Limit the number of frames allowed to be queued concurrently."),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::ThreadedInputMode,
            "Threaded Input Mode",
            RuntimeSettingKey::ThreadedInputMode.apply_path(),
            settings.threaded_input_mode.clone(),
        )
        .with_description("Select how input work is scheduled.")
        .with_options(text_options(&["Auto", "MainThread", "WorkerThread"])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::RenderThreadingMode,
            "Render Threading Mode",
            RuntimeSettingKey::RenderThreadingMode.apply_path(),
            settings.render_threading_mode.clone(),
        )
        .with_description("Select how render preparation and command recording are threaded.")
        .with_options(text_options(&[
            "Auto",
            "SingleRenderThread",
            "ParallelPreparationOnly",
            "ParallelCommandRecording",
            "MultiQueueExperimental",
        ])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::WindowTitle,
            "Window Title",
            RuntimeSettingKey::WindowTitle.apply_path(),
            settings.window_title.clone(),
        ),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::WindowWidth,
            "Window Width",
            RuntimeSettingKey::WindowWidth.apply_path(),
            settings.window_size.width as i64,
        ),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::WindowHeight,
            "Window Height",
            RuntimeSettingKey::WindowHeight.apply_path(),
            settings.window_size.height as i64,
        ),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::WindowPositionX,
            "Window Position X",
            RuntimeSettingKey::WindowPositionX.apply_path(),
            settings.window_position.map(|(x, _)| x as i64).unwrap_or(0),
        ),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::WindowPositionY,
            "Window Position Y",
            RuntimeSettingKey::WindowPositionY.apply_path(),
            settings.window_position.map(|(_, y)| y as i64).unwrap_or(0),
        ),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::WindowMode,
            "Window Mode",
            RuntimeSettingKey::WindowMode.apply_path(),
            window_mode_setting_name(settings.window_mode),
        )
        .with_options(text_options(&["Windowed", "BorderlessFullscreen"])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::WindowDecorations,
            "Window Decorations",
            RuntimeSettingKey::WindowDecorations.apply_path(),
            settings.window_decorations,
        )
        .with_options(bool_options(&[false, true])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::WindowResizable,
            "Window Resizable",
            RuntimeSettingKey::WindowResizable.apply_path(),
            settings.window_resizable,
        )
        .with_options(bool_options(&[false, true])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::WindowMaximized,
            "Window Maximized",
            RuntimeSettingKey::WindowMaximized.apply_path(),
            settings.window_maximized,
        )
        .with_options(bool_options(&[false, true])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::WindowAlwaysOnTop,
            "Window Always On Top",
            RuntimeSettingKey::WindowAlwaysOnTop.apply_path(),
            settings.window_always_on_top,
        )
        .with_options(bool_options(&[false, true])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::WindowCornerStyle,
            "Window Corner Style",
            RuntimeSettingKey::WindowCornerStyle.apply_path(),
            window_corner_style_setting_name(settings.window_corner_style),
        )
        .with_options(text_options(&["Default", "Rounded", "Square"])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::SurfaceTransparency,
            "Surface Transparency",
            RuntimeSettingKey::SurfaceTransparency.apply_path(),
            false,
        )
        .with_options(bool_options(&[false, true])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::WindowBackgroundEffect,
            "Window Background Effect",
            RuntimeSettingKey::WindowBackgroundEffect.apply_path(),
            "None",
        ),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::AntiAliasingMode,
            "Anti-Aliasing Mode",
            RuntimeSettingKey::AntiAliasingMode.apply_path(),
            "Off",
        )
        .with_options(text_options(&["Off", "MSAA", "FXAA", "TAA", "FXAA+TAA"])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::AntiAliasingDial,
            "Anti-Aliasing Dial",
            RuntimeSettingKey::AntiAliasingDial.apply_path(),
            1.0_f64,
        ),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::BloomEnabled,
            "Bloom Enabled",
            RuntimeSettingKey::BloomEnabled.apply_path(),
            true,
        )
        .with_options(bool_options(&[false, true])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::BloomOnly,
            "Bloom Only",
            RuntimeSettingKey::BloomOnly.apply_path(),
            false,
        )
        .with_options(bool_options(&[false, true])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::ToneMappingOperator,
            "Tone Mapping Operator",
            RuntimeSettingKey::ToneMappingOperator.apply_path(),
            "Aces",
        )
        .with_options(text_options(&[
            "Aces",
            "Reinhard",
            "Hermite",
            "Linear",
            "PbrNeutral",
            "AgX",
        ])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::ToneMappingDial,
            "Tone Mapping Dial",
            RuntimeSettingKey::ToneMappingDial.apply_path(),
            1.0_f64,
        ),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::MotionDebugView,
            "Motion Debug View",
            RuntimeSettingKey::MotionDebugView.apply_path(),
            false,
        )
        .with_options(bool_options(&[false, true])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::OverlayVisibility,
            "Overlay Visibility",
            RuntimeSettingKey::OverlayVisibility.apply_path(),
            true,
        )
        .with_options(bool_options(&[false, true])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::ShaderHotReloadPolicy,
            "Shader Hot Reload Policy",
            RuntimeSettingKey::ShaderHotReloadPolicy.apply_path(),
            "Manual",
        )
        .with_options(text_options(&["Manual"])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::AssetHotReloadPolicy,
            "Asset Hot Reload Policy",
            RuntimeSettingKey::AssetHotReloadPolicy.apply_path(),
            "Manual",
        )
        .with_options(text_options(&["Manual"])),
    ];

    descriptors
        .into_iter()
        .map(|descriptor| {
            let id = descriptor.id.clone();
            (
                id,
                RuntimeSettingEntry::new(RuntimeSettingSource::Engine, descriptor, 0),
            )
        })
        .collect()
}

fn bool_options(values: &[bool]) -> Vec<RuntimeSettingOption> {
    values
        .iter()
        .copied()
        .map(|value| RuntimeSettingOption {
            value: RuntimeSettingValue::Bool(value),
            label: if value { "On" } else { "Off" }.to_string(),
        })
        .collect()
}

fn text_options(values: &[&str]) -> Vec<RuntimeSettingOption> {
    values
        .iter()
        .map(|value| RuntimeSettingOption {
            value: RuntimeSettingValue::Text((*value).to_string()),
            label: (*value).to_string(),
        })
        .collect()
}

fn surface_present_mode_name(mode: SurfacePresentMode) -> &'static str {
    match mode {
        SurfacePresentMode::Fifo => "Fifo",
        SurfacePresentMode::Mailbox => "Mailbox",
        SurfacePresentMode::Immediate => "Immediate",
        SurfacePresentMode::RelaxedFifo => "RelaxedFifo",
    }
}

const fn window_mode_setting_name(mode: WindowMode) -> &'static str {
    match mode {
        WindowMode::Windowed => "Windowed",
        WindowMode::BorderlessFullscreen => "BorderlessFullscreen",
    }
}

const fn window_corner_style_setting_name(style: WindowCornerStyle) -> &'static str {
    match style {
        WindowCornerStyle::Default => "Default",
        WindowCornerStyle::Rounded => "Rounded",
        WindowCornerStyle::Square => "Square",
    }
}

fn capability_state_to_support(
    state: Option<PlatformCapabilityState>,
    unsupported_reason: &str,
) -> RuntimeSettingSupport {
    if state.is_some_and(is_capability_supported) {
        RuntimeSettingSupport::supported()
    } else {
        RuntimeSettingSupport::unsupported(unsupported_reason.to_string())
    }
}

const fn is_capability_supported(state: PlatformCapabilityState) -> bool {
    !matches!(state, PlatformCapabilityState::Unsupported)
}

const fn window_material_setting_name(kind: WindowMaterialKind) -> &'static str {
    match kind {
        WindowMaterialKind::Auto => "Auto",
        WindowMaterialKind::ThinTranslucent => "ThinTranslucent",
        WindowMaterialKind::ThickTranslucent => "ThickTranslucent",
        WindowMaterialKind::NoiseTranslucent => "NoiseTranslucent",
        WindowMaterialKind::TitlebarTranslucent => "TitlebarTranslucent",
        WindowMaterialKind::Hud => "Hud",
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
