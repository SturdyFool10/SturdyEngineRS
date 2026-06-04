//! First-party application runtime surface.
//!
//! This module defines the initial public API shape for the runtime shell work
//! described in the roadmap. The current implementation is intentionally thin:
//! it establishes the engine-owned types and access patterns without changing
//! the existing application shell behavior yet.

use parking_lot::Mutex;
use std::{collections::HashMap, fmt, path::PathBuf, sync::Arc, time::Instant};

use crate::{
    BackendFeature, BackendKind, Engine, Error, Format, FrameClock, FrameTime, GraphImage,
    GraphReport, MotionVectorDebugPass, PlatformCapabilityState, RenderFrame, Result, Surface,
    SurfaceCapabilities, SurfaceColorSpace, SurfaceHdrPreference, SurfaceImage, SurfacePresentMode,
    SurfaceRecreateDesc, SurfaceSize, WindowCornerStyle, WindowMaterialKind,
    current_window_appearance_caps,
    render_strategy::{FrameRenderStrategy, RenderStrategySelector},
    scene::Scene,
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
    /// Rolling 128-frame CPU-time history for P95/P99 computation.
    cpu_time_history: FrameTimeHistory,
    /// Rolling 128-frame GPU-time history for P95/P99 computation.
    gpu_time_history: FrameTimeHistory,
    /// Clay UI layout context. Access via [`AppRuntime::clay_ui`].
    clay_ui: clay_ui::UiContext,
    /// Renderer for clay GpuWorkQueue commands — lazy-initialised.
    ui_renderer: Option<crate::ui_renderer::UiRenderer>,
    /// Active benchmark recording session; `None` when not benchmarking.
    benchmark_session: Option<BenchmarkSession>,
    /// Queued backend restart; applied at the next frame boundary.
    pending_backend_restart: Option<BackendFeatureChange>,
    /// Adaptive render quality selector. Updated each frame with the last GPU frame time.
    strategy_selector: RenderStrategySelector,
}

impl AppRuntime {
    /// Create a runtime shell from an engine and surface.
    pub fn new(engine: Engine, surface: Surface) -> Result<Self> {
        let motion_debug_pass = MotionVectorDebugPass::new(&engine)?;
        let runtime = Self {
            default_scene_target: DefaultSceneTargetConfig::new(&engine),
            debug_images: DebugImageRegistry::default(),
            motion_debug_pass,
            frame_clock: FrameClock::new(),
            frame_start: None,
            cpu_time_history: FrameTimeHistory::new(128),
            gpu_time_history: FrameTimeHistory::new(128),
            clay_ui: clay_ui::UiContext::new(),
            ui_renderer: None,
            benchmark_session: None,
            pending_backend_restart: None,
            strategy_selector: RenderStrategySelector::new(),
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
        Ok(runtime)
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
    pub fn clay_ui(&mut self) -> &mut clay_ui::UiContext {
        &mut self.clay_ui
    }

    /// Access the render strategy selector.
    ///
    /// The selector tracks GPU frame time history and adapts the render strategy
    /// automatically when a target frame time is configured.
    pub fn strategy_selector(&self) -> &RenderStrategySelector {
        &self.strategy_selector
    }

    /// Access the render strategy selector mutably.
    ///
    /// Use this to configure a target frame time budget:
    /// `runtime.strategy_selector_mut().set_target_frame_ms(Some(16.6));`
    pub fn strategy_selector_mut(&mut self) -> &mut RenderStrategySelector {
        &mut self.strategy_selector
    }

    pub(crate) fn apply_render_strategy_runtime_settings(
        &mut self,
        controller: &RuntimeController,
    ) {
        let target_frame_ms = controller
            .float_setting(RuntimeSettingKey::TargetFrameMs)
            .and_then(|ms| (ms > 0.0).then_some(ms as f32));
        self.strategy_selector.set_target_frame_ms(target_frame_ms);
    }

    /// Current adaptive render strategy for this frame.
    /// Return the current frame render strategy.
    ///
    /// Updated automatically each frame when a target frame time is set.
    /// Renderer systems read this to select LOD bias, shadow cascades, resolution scale, etc.
    pub fn current_render_strategy(&self) -> &FrameRenderStrategy {
        self.strategy_selector.strategy()
    }

    fn ui_renderer(&mut self) -> Result<&crate::ui_renderer::UiRenderer> {
        if self.ui_renderer.is_none() {
            self.ui_renderer = Some(crate::ui_renderer::UiRenderer::new(&self.engine)?);
        }
        self.ui_renderer.as_ref().ok_or_else(|| {
            crate::Error::ResourceStateCorruption(
                "UI renderer was not available after initialization".into(),
            )
        })
    }

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
        // Notify NVIDIA Reflex/AMD Anti-Lag that a new frame is starting.
        // Both calls are no-ops when the respective feature is unavailable.
        let _ = self.surface.latency_sleep();
        let _ = self.engine.device.anti_lag_frame_start();
        self.frame_start = Some(Instant::now());
        // Upload any textures that background workers finished decoding.
        let _ = self.engine.drain_pending_uploads();
        let frame_time = self.frame_clock.tick();
        let surface_image = self.surface.acquire_image()?;
        let render_frame = self.engine.begin_render_frame_for(&surface_image)?;
        let (device, handle) = self.surface.auto_present_info();
        render_frame.configure_auto_present(device, handle);
        Ok(AppRuntimeFrame {
            runtime: self,
            surface_image,
            render_frame,
            frame_time,
            window_scale_factor: 1.0,
            window_logical_size: None,
            wait_for_gpu_before_present: false,
            finished: false,
            fixed_alpha: 0.0,
        })
    }

    /// Start recording a benchmark session. Replaces any in-progress session.
    pub fn start_benchmark(&mut self) {
        self.benchmark_session = Some(BenchmarkSession::new());
    }

    /// Stop the benchmark session and return an aggregated report.
    ///
    /// Returns `None` if no session was started.
    pub fn stop_benchmark(&mut self) -> Option<BenchmarkReport> {
        self.benchmark_session.take().map(|s| s.finish())
    }

    /// `true` if a benchmark session is currently recording.
    pub fn is_benchmarking(&self) -> bool {
        self.benchmark_session.is_some()
    }

    /// Decompose the runtime into its current owned parts.
    pub fn into_parts(self) -> (Engine, Surface, RuntimeController) {
        (self.engine, self.surface, self.controller)
    }

    // ── Backend restart ───────────────────────────────────────────────────────

    /// Queue a backend restart at the next frame boundary.
    ///
    /// The current backend is torn down (`vkDeviceWaitIdle`, device destroyed),
    /// a new one is created with `changes` applied to the current feature set,
    /// and the presentation surface is recreated automatically.  All existing
    /// GPU resource handles (images, buffers, pipelines, shaders, samplers)
    /// become invalid.  The [`RuntimeApp::on_backend_restarted`] callback fires
    /// on the same frame so the app can recreate its resources before the next
    /// `update` call.
    ///
    /// If a restart is already pending the new request replaces the previous one.
    pub fn request_backend_restart(&mut self, changes: BackendFeatureChange) {
        self.pending_backend_restart = Some(changes);
    }

    /// `true` if a backend restart has been requested and not yet applied.
    pub fn backend_restart_pending(&self) -> bool {
        self.pending_backend_restart.is_some()
    }

    /// All [`BackendFeature`] variants currently enabled on this runtime's backend.
    pub fn enabled_features(&self) -> Vec<BackendFeature> {
        self.engine.enabled_features()
    }

    /// Returns `true` when `feature` is active on the current backend.
    pub fn has_feature(&self, feature: BackendFeature) -> bool {
        self.engine.has_feature(feature)
    }

    /// Apply the queued backend restart synchronously.
    ///
    /// Called by the shell at the end of each `Ok` frame.  Returns the outcome
    /// (new caps, what features are now active) so the shell can notify the app.
    /// Clears `pending_backend_restart` regardless of success.
    pub(crate) fn apply_pending_backend_restart(&mut self) -> Result<BackendRestartOutcome> {
        let changes = self
            .pending_backend_restart
            .take()
            .expect("apply_pending_backend_restart called with no pending restart");

        // Build the new DeviceDesc from the current creation desc + requested changes.
        let mut new_desc = self.engine.creation_desc();
        for feature in &changes.enable {
            new_desc.disabled_features.retain(|f| f != feature);
            if !new_desc.optional_features.contains(feature)
                && !new_desc.required_features.contains(feature)
            {
                new_desc.optional_features.push(feature.clone());
            }
        }
        for feature in &changes.disable {
            new_desc.optional_features.retain(|f| f != feature);
            new_desc.required_features.retain(|f| f != feature);
            if !new_desc.disabled_features.contains(feature) {
                new_desc.disabled_features.push(feature.clone());
            }
        }

        // Capture the current surface size + native desc *before* tearing down the backend.
        // The new backend needs to recreate the surface from the same window handles.
        let current_size = self.surface.size();
        let native_desc_for_recreate = self.surface.native_desc.as_ref().map(|d| {
            let mut nd = d.clone();
            nd.size = current_size; // Use current (possibly-resized) size
            nd
        });

        // Swap the backend — waits for idle, creates new backend, drops old, clears all state.
        self.engine.device.rebuild_backend(&new_desc)?;

        // Clear engine-level caches (graph images, texture cache, pending uploads).
        self.engine.clear_caches_after_backend_restart();

        // Rebuild the sampler catalog so default samplers are valid in the new backend.
        self.engine.rebuild_sampler_catalog()?;

        // Recreate the presentation surface in the new backend.
        if let Some(nd) = native_desc_for_recreate {
            let new_handle = self.engine.device.create_surface(nd.clone())?;
            let new_info = self.engine.device.surface_info(new_handle)?;
            // Replace the surface — old Surface's Drop ignores the now-invalid handle.
            self.surface = Surface {
                device: self.engine.device.clone(),
                handle: new_handle,
                info: new_info,
                native_desc: Some(nd),
            };
        }

        // Reset timing histories — old frame times are meaningless after a backend swap.
        self.cpu_time_history = FrameTimeHistory::new(128);
        self.gpu_time_history = FrameTimeHistory::new(128);

        // Reset lazy-initialized renderer state.
        self.ui_renderer = None;
        self.motion_debug_pass = MotionVectorDebugPass::new(&self.engine)?;

        // Refresh the runtime controller with new backend/surface info.
        self.refresh_controller_state();

        Ok(BackendRestartOutcome {
            new_desc,
            new_caps: self.engine.caps(),
        })
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
    /// DPI scale factor from the OS window, set by the event loop shell.
    window_scale_factor: f32,
    /// Logical window size in window pixels, set by the event loop shell.
    window_logical_size: Option<[f32; 2]>,
    /// When enabled, `finish_and_present` waits for this frame's GPU submission
    /// before queueing presentation. This is useful for very expensive temporal
    /// paths where multiple frames in flight can make history appear to jump.
    wait_for_gpu_before_present: bool,
    /// Set to `true` after `finish_and_present` completes to prevent the `Drop`
    /// impl from double-presenting when the user calls it explicitly.
    finished: bool,
    /// Interpolation alpha in [0, 1] between the last fixed step and the next.
    fixed_alpha: f32,
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

    /// DPI scale factor for converting logical window/UI pixels to physical surface pixels.
    ///
    /// Set by the event loop shell from the OS window state before calling `update`.
    /// Defaults to `1.0` when running outside the first-party shell.
    pub fn window_scale_factor(&self) -> f32 {
        self.window_scale_factor
    }

    /// Current drawable window size in logical window pixels, when known.
    ///
    /// Set by the event loop shell from the OS window state before calling `update`.
    pub fn window_logical_size(&self) -> Option<[f32; 2]> {
        self.window_logical_size
    }

    pub(crate) fn set_window_scale_factor(&mut self, scale_factor: f32) {
        self.window_scale_factor = scale_factor.max(f32::EPSILON);
    }

    pub(crate) fn set_window_logical_size(&mut self, size: [f32; 2]) {
        self.window_logical_size = Some([size[0].max(1.0), size[1].max(1.0)]);
    }

    pub fn set_wait_for_gpu_before_present(&mut self, wait: bool) {
        self.wait_for_gpu_before_present = wait;
    }

    /// Interpolation alpha between the last fixed step and the next, in [0, 1].
    pub fn fixed_alpha(&self) -> f32 {
        self.fixed_alpha
    }

    pub(crate) fn set_fixed_alpha(&mut self, alpha: f32) {
        self.fixed_alpha = alpha.clamp(0.0, 1.0);
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

    /// Register a named debug image with the runtime-owned registry for this frame.
    pub fn register_debug_image(&self, name: impl Into<String>, image: &GraphImage) {
        self.runtime.debug_images.register(image, name);
    }

    /// Save a named graph image from this frame as a PNG.
    ///
    /// This is an explicit blocking screenshot/readback helper. It submits and
    /// waits for the current render graph with `FrameSyncReason::ReadbackCompletion`.
    pub fn save_named_graph_image_png(
        &self,
        name: &str,
        path: impl AsRef<std::path::Path>,
    ) -> Result<crate::ScreenshotExportReport> {
        self.shell_frame().save_named_graph_image_png(name, path)
    }

    /// Return the shared runtime settings/diagnostics controller.
    pub fn runtime_controller(&self) -> RuntimeController {
        self.runtime.controller.clone()
    }

    /// Return a snapshot of the current runtime diagnostics.
    pub fn runtime_diagnostics(&self) -> RuntimeDiagnostics {
        self.runtime.controller.diagnostics()
    }

    /// Format default diagnostics into overlay-friendly lines.
    ///
    /// Equivalent to [`ShellFrame::default_runtime_overlay_lines`] — useful when
    /// displaying a standard diagnostics overlay without constructing a shell frame.
    pub fn default_runtime_overlay_lines(&self) -> Vec<String> {
        self.shell_frame().default_runtime_overlay_lines()
    }

    /// Return compact render-graph inspection lines for the frame recorded so far.
    pub fn runtime_graph_inspection_lines(
        &self,
        max_passes: usize,
        max_images: usize,
    ) -> Vec<String> {
        self.shell_frame()
            .runtime_graph_inspection_lines(max_passes, max_images)
    }

    /// Run the default HDR post chain from scene color through tonemap.
    ///
    /// This is the `AppRuntimeFrame` entry point for the runtime-owned post stack:
    /// optional bloom, anti-aliasing, motion-vector debug visualization, tonemapping,
    /// debug-image registration, and runtime diagnostics are handled consistently
    /// without requiring app code to construct a [`ShellFrame`](crate::ShellFrame).
    pub fn run_default_post_process<T: bytemuck::Pod>(
        &self,
        desc: crate::application::RuntimePostProcessDesc<'_, T>,
    ) -> Result<crate::application::RuntimePostProcessOutput> {
        self.shell_frame().run_default_post_process(desc)
    }

    /// Create a [`ShellFrame`](crate::ShellFrame) wrapper for use with APIs that
    /// still require the older frame type.
    ///
    /// Prefer the native `AppRuntimeFrame` methods for new code. This bridge
    /// exists to ease migration from `EngineApp`/`GameApp` to `RuntimeApp`.
    pub fn shell_frame(&self) -> crate::application::ShellFrame<'_> {
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
    pub fn ui_context(&mut self) -> UiContext<'_> {
        let surface_info = self.runtime.surface.info();
        let viewport = clay_ui::Size {
            width: surface_info.size.width as f32,
            height: surface_info.size.height as f32,
        };
        let frame_number = self.runtime.frame_clock.time().frame;
        UiContext::new(self.runtime, viewport, frame_number)
    }

    pub fn route_input_to_ui(&mut self, hub: &mut crate::InputHub) {
        hub.route_to_clay(&mut self.runtime.clay_ui);
    }

    pub fn draw_ui(&mut self, output: &clay_ui::UiFrameOutput, target: &GraphImage) -> Result<()> {
        let surface = self.runtime.surface.info();
        let (w, h) = (surface.size.width, surface.size.height);
        let renderer = self.runtime.ui_renderer()?;
        for (_, tree) in &output.trees {
            renderer.draw_queue(&self.render_frame, target, &tree.queue, w, h)?;
        }
        if !output.text_scenes.is_empty() {
            crate::ui_renderer::draw_ui_text(
                &self.render_frame,
                &self.runtime.engine,
                output,
                target,
                w,
                h,
            )?;
        }
        Ok(())
    }

    /// Report scene-derived workload metrics — submitted triangle count from the
    /// GPU-driven bin list.  Call once per frame, typically right before
    /// `finish_and_present`.  No-op when GPU culling is not active on the scene.
    pub fn report_scene_workload(&self, scene: &Scene) {
        if let Some(submitted) = scene.submitted_triangle_count() {
            self.runtime.controller.update_diagnostics(|d| {
                d.workload.submitted_triangles = Some(submitted);
            });
        }
    }

    pub fn finish_and_present(&mut self) -> Result<()> {
        self.finished = true;
        let flush_report = match self
            .render_frame
            .flush_with_reason(crate::FrameSyncReason::FrameBoundaryPresent)
        {
            Ok(report) => report,
            Err(error) => {
                tracing::error!("frame flush/auto-present failed: {error:?}");
                return Err(error);
            }
        };
        let submit_gpu_wait_ms = self.runtime.engine.device.last_submit_gpu_wait_ms();
        let active_cpu_ms =
            self.runtime.frame_start.as_ref().map(|start| {
                (start.elapsed().as_secs_f32() * 1000.0 - submit_gpu_wait_ms).max(0.0)
            });
        let mut gpu_wait_ms = (submit_gpu_wait_ms > 0.0).then_some(submit_gpu_wait_ms);
        if self.wait_for_gpu_before_present {
            let wait_start = Instant::now();
            self.render_frame
                .wait_with_reason(crate::FrameSyncReason::FrameBoundaryPresent)?;
            let explicit_wait_ms = wait_start.elapsed().as_secs_f32() * 1000.0;
            gpu_wait_ms = Some(gpu_wait_ms.unwrap_or(0.0) + explicit_wait_ms);
        }
        self.runtime.controller.update_diagnostics(|d| {
            d.frame_sync = Some(format!(
                "reason={:?} submitted={} waited={} presented=true submission={:?}",
                flush_report.reason,
                flush_report.submitted,
                self.wait_for_gpu_before_present,
                flush_report.submission
            ));
        });
        if self.runtime.frame_start.take().is_some() {
            let cpu_ms = active_cpu_ms.unwrap_or(0.0);
            self.runtime.cpu_time_history.push(cpu_ms);
            let cpu_percentiles = self.runtime.cpu_time_history.percentiles();
            let pass_timings_raw = self.runtime.engine.device.pass_timings();
            let gpu_total: f32 = pass_timings_raw.iter().map(|t| t.gpu_ms).sum();
            let pass_timings: Vec<RuntimePassTiming> = pass_timings_raw
                .iter()
                .map(|t| RuntimePassTiming {
                    name: t.name.clone(),
                    gpu_time_ms: Some(t.gpu_ms),
                })
                .collect();
            let gpu_ms = if pass_timings.is_empty() {
                None
            } else {
                Some(gpu_total)
            };
            if let Some(total) = gpu_ms {
                self.runtime.gpu_time_history.push(total);
            }
            let gpu_percentiles = self.runtime.gpu_time_history.percentiles();
            let gpu_timeline = if pass_timings.is_empty() {
                None
            } else {
                Some(self.runtime.engine.device.gpu_timeline())
            };
            if let Some(session) = self.runtime.benchmark_session.as_mut() {
                let frame_index = session.next_frame_index;
                session.record(BenchmarkFrameSample {
                    frame_index,
                    cpu_ms,
                    gpu_ms,
                    gpu_wait_ms,
                    pass_timings: pass_timings
                        .iter()
                        .filter_map(|p| {
                            p.gpu_time_ms.map(|ms| BenchmarkPassSample {
                                name: p.name.clone(),
                                gpu_ms: ms,
                            })
                        })
                        .collect(),
                });
            }
            let present_to_display_ms: Option<f32> = None;
            let total_latency_ms = gpu_ms
                .zip(present_to_display_ms)
                .map(|(gpu_ms, present_ms)| cpu_ms + gpu_ms + present_ms);
            let mem_budget = self.runtime.engine.device.memory_budget();
            let memory_used_bytes = mem_budget.as_ref().map(|b| b.device_local_used_bytes);
            let memory_budget_bytes = mem_budget.as_ref().map(|b| b.device_local_capacity_bytes);
            let (raw_draws, raw_dispatches) =
                self.runtime.engine.device.frame_draw_dispatch_counts();
            let draw_count = (raw_draws > 0).then_some(raw_draws as u64);
            let dispatch_count = (raw_dispatches > 0).then_some(raw_dispatches as u64);
            let raw_upload_bytes = self.render_frame.frame_upload_bytes();
            let upload_bytes = (raw_upload_bytes > 0).then_some(raw_upload_bytes);
            let raw_transient = self.runtime.engine.device.transient_aliased_bytes();
            let transient_aliased_bytes = (raw_transient > 0).then_some(raw_transient);
            self.runtime.controller.update_diagnostics(|d| {
                d.timings.available = true;
                d.timings.cpu_frame_time_ms = Some(cpu_ms);
                d.timings.gpu_wait_time_ms = gpu_wait_ms;
                d.timings.gpu_frame_time_ms = gpu_ms;
                d.timings.present_to_display_ms = present_to_display_ms;
                d.timings.total_latency_ms = total_latency_ms;
                d.timings.pass_timings = pass_timings;
                d.timings.gpu_timeline = gpu_timeline;
                d.workload.memory_used_bytes = memory_used_bytes;
                d.workload.memory_budget_bytes = memory_budget_bytes;
                d.workload.draw_count = draw_count;
                d.workload.dispatch_count = dispatch_count;
                d.workload.upload_bytes = upload_bytes;
                d.workload.transient_aliased_bytes = transient_aliased_bytes;
                if let Some((mean, p95, p99)) = cpu_percentiles {
                    d.timings.cpu_mean_ms = Some(mean);
                    d.timings.cpu_p95_ms = Some(p95);
                    d.timings.cpu_p99_ms = Some(p99);
                }
                if let Some((mean, p95, p99)) = gpu_percentiles {
                    d.timings.gpu_mean_ms = Some(mean);
                    d.timings.gpu_p95_ms = Some(p95);
                    d.timings.gpu_p99_ms = Some(p99);
                }
                if let Some(report) = FrameTimingReport::from_summary(&d.timings) {
                    if report.is_jittery() {
                        tracing::info!(
                            "frame jitter: p99={:.1}ms mean={:.1}ms (p99 > 2× mean)",
                            report.p99_cpu_ms,
                            report.mean_cpu_ms
                        );
                    }
                    crate::engine_global::set_frame_timing(report);
                }
            });

            // Update the render strategy selector with the latest GPU frame time.
            // Use draw_count as a rough proxy for scene object count.
            let scene_objects = draw_count.unwrap_or(0);
            self.runtime.strategy_selector.update(gpu_ms, scene_objects);
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

/// Full application lifecycle trait for the runtime-owned frame loop.
///
/// Implement this instead of [`EngineApp`](crate::EngineApp) or
/// [`GameApp`](crate::GameApp) to get the first-party runtime shell as the
/// default frame loop. The runtime shell owns surface acquisition, frame
/// timing, CPU/GPU diagnostic recording, and P95/P99 history — the app only
/// provides content and responds to lifecycle callbacks.
///
/// Use [`run_with_runtime`](crate::run_with_runtime) to launch the event loop.
///
/// # Example
///
/// ```ignore
/// use sturdy_engine::{AppRuntime, AppRuntimeFrame, RuntimeApp, WindowConfig, run_with_runtime};
///
/// struct MyApp { /* scene, passes, etc. */ }
///
/// impl RuntimeApp for MyApp {
///     type Error = sturdy_engine::Error;
///
///     fn init(runtime: &mut AppRuntime) -> sturdy_engine::Result<Self> {
///         Ok(Self { /* ... */ })
///     }
///
///     fn update(&mut self, frame: &mut AppRuntimeFrame<'_>) -> sturdy_engine::Result<()> {
///         // Acquire, record passes, present is handled by the runtime.
///         Ok(())
///     }
/// }
///
/// fn main() {
///     run_with_runtime::<MyApp>(WindowConfig::new("My App", 1280, 720));
/// }
/// ```
pub trait RuntimeApp: Sized {
    type Error: std::error::Error;

    /// Initialize the application after `AppRuntime` is ready.
    ///
    /// Called once before the first frame. The runtime already owns the engine
    /// and surface; use it to create scenes, passes, and other renderer state.
    fn init(runtime: &mut AppRuntime) -> std::result::Result<Self, Self::Error>;

    /// Advance and render one frame.
    ///
    /// Called every frame with the acquired surface image and render frame
    /// bundled in `frame`. Call `frame.finish_and_present()` explicitly if you
    /// want to observe its error, or let the `Drop` impl present automatically.
    fn update(&mut self, frame: &mut AppRuntimeFrame<'_>) -> std::result::Result<(), Self::Error>;

    /// Handle a window resize.
    ///
    /// Called when the window is resized. The surface has already been
    /// resized by the runtime shell before this callback fires.
    fn resize(
        &mut self,
        _runtime: &mut AppRuntime,
        _width: u32,
        _height: u32,
    ) -> std::result::Result<(), Self::Error> {
        Ok(())
    }

    /// Return the app's [`InputHub`](crate::InputHub) for automatic input routing.
    ///
    /// When this returns `Some`, the shell routes keyboard, pointer, and scroll
    /// events into the hub before calling `update` — the individual `key_pressed`
    /// and pointer callbacks below are skipped. Prefer `InputHub` for new code.
    fn input_hub(&mut self) -> Option<&mut crate::InputHub> {
        None
    }

    /// Handle a structured key input event.
    ///
    /// Only called when [`input_hub`](RuntimeApp::input_hub) returns `None`.
    /// Unlike [`key_pressed`](RuntimeApp::key_pressed), this receives press and
    /// release transitions for physical keys and is suitable for held controls.
    fn key_input(&mut self, _input: &crate::KeyInput) -> std::result::Result<(), Self::Error> {
        Ok(())
    }

    /// Handle a character key press.
    ///
    /// Only called when [`input_hub`](RuntimeApp::input_hub) returns `None` and
    /// the pressed key has a printable character string (e.g. `"b"`, `"B"`, `"1"`).
    fn key_pressed(&mut self, _key: &str) -> std::result::Result<(), Self::Error> {
        Ok(())
    }

    /// Handle pointer (mouse/touch) movement.
    ///
    /// `pos` is in top-left/Y-down `WindowLogicalPx`. Only called when
    /// [`input_hub`](RuntimeApp::input_hub) returns `None`.
    fn pointer_moved(
        &mut self,
        _pos: clay_ui::WindowLogicalPx,
    ) -> std::result::Result<(), Self::Error> {
        Ok(())
    }

    /// Handle a pointer button press or release.
    ///
    /// `pos` is in top-left/Y-down `WindowLogicalPx`. `button` is 0 = primary,
    /// 1 = secondary, 2 = middle. Only called when
    /// [`input_hub`](RuntimeApp::input_hub) returns `None`.
    fn pointer_button(
        &mut self,
        _pos: clay_ui::WindowLogicalPx,
        _button: u8,
        _pressed: bool,
    ) -> std::result::Result<(), Self::Error> {
        Ok(())
    }

    /// React to runtime setting changes applied since the last frame.
    ///
    /// Called once per frame when at least one setting has changed. Inspect
    /// `changes` to rebuild passes, recreate targets, or update config structs.
    /// The surface has already been updated by the shell before this is called.
    fn runtime_settings_changed(
        &mut self,
        _controller: &RuntimeController,
        _changes: &[RuntimeSettingChange],
    ) -> std::result::Result<(), Self::Error> {
        Ok(())
    }

    /// Called after the backend has been restarted via
    /// [`AppRuntime::request_backend_restart`].
    ///
    /// All previously created GPU resources (images, buffers, pipelines,
    /// shaders, samplers) are invalid.  The new backend is fully initialised
    /// and the presentation surface has been recreated before this fires.
    /// Use this callback to recreate renderer state and re-upload assets.
    fn on_backend_restarted(
        &mut self,
        _runtime: &mut AppRuntime,
        _outcome: &BackendRestartOutcome,
    ) -> std::result::Result<(), Self::Error> {
        Ok(())
    }

    /// Return the desired fixed simulation step duration.
    ///
    /// When `Some`, the shell runs [`fixed_update`](RuntimeApp::fixed_update)
    /// as many times per render frame as needed to catch up, capped at 8 steps
    /// to prevent spiral-of-death. The remaining accumulator fraction is exposed
    /// as [`AppRuntimeFrame::fixed_alpha`] for render-time interpolation.
    fn fixed_step(&self) -> Option<std::time::Duration> {
        None
    }

    /// Advance the simulation by one fixed step.
    ///
    /// Only called when [`fixed_step`](RuntimeApp::fixed_step) returns `Some`.
    fn fixed_update(
        &mut self,
        _ctx: &RuntimeFixedUpdateContext,
    ) -> std::result::Result<(), Self::Error> {
        Ok(())
    }
}

/// Context passed to [`RuntimeApp::fixed_update`] each simulation step.
pub struct RuntimeFixedUpdateContext {
    pub step_index: u32,
    pub fixed_step: std::time::Duration,
    pub pacing_error: std::time::Duration,
}

impl RuntimeFixedUpdateContext {
    pub fn fixed_step_secs(&self) -> f32 {
        self.fixed_step.as_secs_f32()
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
        self.shared.lock().settings.clone()
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
        let mut shared = self.shared.lock();
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
        self.shared.lock().setting_entries.get(&id.into()).cloned()
    }

    /// Return every registered runtime setting.
    pub fn setting_entries(&self) -> Vec<RuntimeSettingEntry> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut entries = self
            .shared
            .lock()
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
        self.shared.lock().settings_revision
    }

    /// Return every settings change recorded after `revision`.
    pub fn setting_changes_since(&self, revision: u64) -> Vec<RuntimeSettingChange> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shared
            .lock()
            .change_log
            .iter()
            .filter(|change| change.revision > revision)
            .cloned()
            .collect()
    }

    /// Return the current apply-notification revision.
    pub fn apply_notifications_revision(&self) -> u64 {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shared.lock().apply_notifications_revision
    }

    /// Return apply notifications recorded after `revision`.
    ///
    /// Unlike `setting_changes_since`, this includes rejected requests and
    /// no-op accepted requests so applications can surface exact outcomes.
    pub fn apply_notifications_since(&self, revision: u64) -> Vec<RuntimeApplyNotification> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shared
            .lock()
            .apply_notifications
            .iter()
            .filter(|notification| notification.revision > revision)
            .cloned()
            .collect()
    }

    /// Return the most recent runtime apply report, if any transaction has run.
    pub fn last_apply_report(&self) -> Option<RuntimeApplyReport> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shared.lock().last_apply_report.clone()
    }

    /// Return the current runtime diagnostics snapshot.
    ///
    /// The returned snapshot includes any active shader compile errors and asset
    /// diagnostics reported via `report_shader_compile_error` / `report_asset_state`.
    pub fn diagnostics(&self) -> RuntimeDiagnostics {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let shared = self.shared.lock();
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

    /// Publish renderer workload counters for the current frame.
    pub fn report_workload_diagnostics(&self, workload: RuntimeWorkloadDiagnostics) {
        self.update_diagnostics(|diagnostics| {
            diagnostics.workload = workload;
        });
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
            .shader_compile_errors
            .insert(path.into(), message.into());
    }

    /// Clear a previously-reported shader compile error after a successful reload.
    pub fn clear_shader_compile_error(&self, path: impl Into<PathBuf>) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let _ = self
            .shared
            .lock()
            .shader_compile_errors
            .remove(&path.into());
    }

    /// Clear all shader compile errors.
    pub fn clear_all_shader_compile_errors(&self) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shared.lock().shader_compile_errors.clear();
    }

    /// Report or update the health state for a monitored asset path.
    ///
    /// `AssetState::Ok` entries are tracked internally but excluded from
    /// `diagnostics().asset_diagnostics` so only problems are surfaced.
    pub fn report_asset_state(&self, path: impl Into<PathBuf>, state: AssetState) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let _ = self.shared.lock().asset_states.insert(path.into(), state);
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
        let _ = self.shared.lock().asset_states.remove(&path.into());
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
    /// tracing::info!("{text}");
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
        self.shared.lock().overlay_lines.clone()
    }

    pub(crate) fn set_settings(&self, settings: RuntimeSettingsSnapshot) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut shared = self.shared.lock();
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
            .sync_engine_capabilities(hdr_caps, surface_caps.as_ref());
    }

    pub(crate) fn update_diagnostics(&self, f: impl FnOnce(&mut RuntimeDiagnostics)) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut shared = self.shared.lock();
        f(&mut shared.diagnostics);
    }

    pub(crate) fn set_overlay_lines(&self, lines: Vec<String>) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shared.lock().overlay_lines = lines;
    }

    pub(crate) fn clear_overlay_lines(&self) {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shared.lock().overlay_lines.clear();
    }

    pub(crate) fn record_runtime_apply_report(&self, report: RuntimeApplyReport) {
        if report.changes.is_empty() {
            return;
        }
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.shared.lock().record_apply_report(report);
    }
}

/// Snapshot of the current runtime settings.
#[derive(Clone, Debug, PartialEq)]
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
    /// Target GPU frame time for adaptive render strategy, in milliseconds.
    /// `None` disables timing-driven quality adaptation.
    pub target_frame_ms: Option<f32>,
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
    pub shader_hot_reload_policy: String,
    pub asset_hot_reload_policy: String,
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
            target_frame_ms: None,
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
            shader_hot_reload_policy: "Manual".to_string(),
            asset_hot_reload_policy: "Manual".to_string(),
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
    /// True when the auto-exposure histogram + adapt pipeline ran this frame.
    pub auto_exposure_active: bool,
    /// Latest readback from the auto-exposure exposure-state buffer, when
    /// available.  Updated only when [`Self::auto_exposure_active`] is true and
    /// the runtime polled the readback.
    pub auto_exposure: Option<AutoExposureDiagnostics>,
    pub native_window_appearance: Option<String>,
    pub windows: RuntimeWindowDiagnostics,
    pub runtime_setting_apply: Option<String>,
    pub frame_sync: Option<String>,
    pub user_diagnostics: Vec<RuntimeUserDiagnostic>,
    pub camera_locked_passes: Vec<String>,
    pub debug_images: Vec<String>,
    pub graph: RuntimeGraphDiagnostics,
    pub timings: RuntimeTimingSummary,
    pub workload: RuntimeWorkloadDiagnostics,
    /// Active shader compile errors reported via `RuntimeController::report_shader_compile_error`.
    pub shader_compile_errors: Vec<ShaderCompileError>,
    /// Asset paths that are missing or stale, surfaced via `RuntimeController::report_asset_state`.
    pub asset_diagnostics: Vec<AssetDiagnostic>,
}

/// Per-frame snapshot of auto-exposure state surfaced via [`RuntimeDiagnostics`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AutoExposureDiagnostics {
    /// Currently adapted exposure (EV100) being applied to the scene.
    pub adapted_ev: f32,
    /// Last unblended target exposure (EV100) derived from the histogram.
    pub target_ev: f32,
    /// Mean linear luminance derived from this frame's histogram.
    pub avg_luminance: f32,
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

/// A point-in-time snapshot of frame timing from the most recently completed frame.
///
/// Obtained via [`Engine::frame_timing()`] from any thread at any time.
#[derive(Clone, Debug)]
pub struct FrameTimingReport {
    /// CPU time spent building/submitting the frame, excluding explicit GPU waits.
    pub cpu_ms: f32,
    pub gpu_ms: Option<f32>,
    /// CPU time intentionally blocked waiting for GPU completion, tracked separately from `cpu_ms`.
    pub gpu_wait_ms: Option<f32>,
    /// Time from present submission to scan-out/display, when the backend can report it.
    pub present_to_display_ms: Option<f32>,
    /// End-to-end latency estimate from frame CPU start through display, when all measured
    /// components are available.
    pub total_latency_ms: Option<f32>,
    pub mean_cpu_ms: f32,
    pub p95_cpu_ms: f32,
    pub p99_cpu_ms: f32,
    pub fps: f32,
}

impl FrameTimingReport {
    pub(crate) fn from_summary(s: &RuntimeTimingSummary) -> Option<Self> {
        let cpu_ms = s.cpu_frame_time_ms?;
        let mean = s.cpu_mean_ms?;
        let p95 = s.cpu_p95_ms?;
        let p99 = s.cpu_p99_ms?;
        Some(Self {
            cpu_ms,
            gpu_ms: s.gpu_frame_time_ms,
            gpu_wait_ms: s.gpu_wait_time_ms,
            present_to_display_ms: s.present_to_display_ms,
            total_latency_ms: s.total_latency_ms,
            mean_cpu_ms: mean,
            p95_cpu_ms: p95,
            p99_cpu_ms: p99,
            fps: if mean > 0.0 { 1000.0 / mean } else { 0.0 },
        })
    }

    pub fn is_jittery(&self) -> bool {
        self.p99_cpu_ms > self.mean_cpu_ms * 2.0
    }
}

/// Rolling fixed-size frame-time sample buffer for P95/P99 computation.
pub(crate) struct FrameTimeHistory {
    samples: Vec<f32>,
    head: usize,
    count: usize,
    capacity: usize,
}

impl FrameTimeHistory {
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: vec![0.0; capacity],
            head: 0,
            count: 0,
            capacity,
        }
    }

    pub fn push(&mut self, ms: f32) {
        self.samples[self.head] = ms;
        self.head = (self.head + 1) % self.capacity;
        self.count = (self.count + 1).min(self.capacity);
    }

    pub fn percentiles(&self) -> Option<(f32, f32, f32)> {
        if self.count < 4 {
            return None;
        }
        let mut sorted: Vec<f32> = self.samples[..self.count].to_vec();
        sorted.sort_by(f32::total_cmp);
        let mean = sorted.iter().sum::<f32>() / sorted.len() as f32;
        let p95 = sorted[(sorted.len() as f32 * 0.95) as usize];
        let p99 = sorted[((sorted.len() - 1) as f32 * 0.99) as usize];
        Some((mean, p95, p99))
    }
}

/// Renderer workload counters surfaced to benchmark/reporting tools.
///
/// Values are optional when the backend or render path cannot report them yet.
/// This keeps benchmark reports schema-stable while making missing counters
/// explicit instead of silently omitting them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuntimeWorkloadDiagnostics {
    pub visible_triangles: Option<u64>,
    pub submitted_triangles: Option<u64>,
    pub draw_count: Option<u64>,
    pub dispatch_count: Option<u64>,
    pub memory_used_bytes: Option<u64>,
    pub memory_budget_bytes: Option<u64>,
    pub upload_bytes: Option<u64>,
    /// Bytes held in transient alias-heap memory shared by render-graph
    /// intermediate images (G-buffer, shadow maps, etc.).  Non-None when the
    /// render graph executes at least one aliased image this frame.
    pub transient_aliased_bytes: Option<u64>,
}

/// Frame timing summary surfaced through runtime diagnostics.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuntimeTimingSummary {
    pub available: bool,
    /// CPU time spent building/submitting the frame, excluding explicit GPU waits.
    pub cpu_frame_time_ms: Option<f32>,
    /// CPU wall time spent in explicit GPU waits requested by the app/runtime.
    pub gpu_wait_time_ms: Option<f32>,
    pub gpu_frame_time_ms: Option<f32>,
    pub present_to_display_ms: Option<f32>,
    pub total_latency_ms: Option<f32>,
    pub pass_timings: Vec<RuntimePassTiming>,
    pub cpu_mean_ms: Option<f32>,
    pub cpu_p95_ms: Option<f32>,
    pub cpu_p99_ms: Option<f32>,
    pub gpu_mean_ms: Option<f32>,
    pub gpu_p95_ms: Option<f32>,
    pub gpu_p99_ms: Option<f32>,
    pub gpu_timeline: Option<crate::GpuTimeline>,
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
            RuntimeSettingKey::TargetFrameMs,
            RuntimeSettingValue::Float(settings.target_frame_ms.unwrap_or(0.0) as f64),
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
        self.sync_engine_value(
            RuntimeSettingKey::ShaderHotReloadPolicy,
            RuntimeSettingValue::Text(settings.shader_hot_reload_policy.clone()),
        );
        self.sync_engine_value(
            RuntimeSettingKey::AssetHotReloadPolicy,
            RuntimeSettingValue::Text(settings.asset_hot_reload_policy.clone()),
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
        sync_runtime_settings_snapshot_value(&mut self.settings, &id, &applied_value);
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
        (
            RuntimeSettingId::Engine(RuntimeSettingKey::TargetFrameMs),
            RuntimeSettingValue::Float(requested),
        ) => {
            const MIN_TARGET_FRAME_MS: f64 = 0.0;
            const MAX_TARGET_FRAME_MS: f64 = 1000.0;
            let clamped = requested.clamp(MIN_TARGET_FRAME_MS, MAX_TARGET_FRAME_MS);
            if clamped == requested {
                (RuntimeSettingValue::Float(requested), None)
            } else {
                (
                    RuntimeSettingValue::Float(clamped),
                    Some(format!(
                        "requested {requested}, allowed range is {MIN_TARGET_FRAME_MS}..={MAX_TARGET_FRAME_MS}; 0 disables adaptive strategy"
                    )),
                )
            }
        }
        (_, value) => (value, None),
    }
}

fn sync_runtime_settings_snapshot_value(
    settings: &mut RuntimeSettingsSnapshot,
    id: &RuntimeSettingId,
    value: &RuntimeSettingValue,
) {
    match (id, value) {
        (
            RuntimeSettingId::Engine(RuntimeSettingKey::ShaderHotReloadPolicy),
            RuntimeSettingValue::Text(policy),
        ) => settings.shader_hot_reload_policy = policy.clone(),
        (
            RuntimeSettingId::Engine(RuntimeSettingKey::AssetHotReloadPolicy),
            RuntimeSettingValue::Text(policy),
        ) => settings.asset_hot_reload_policy = policy.clone(),
        (
            RuntimeSettingId::Engine(RuntimeSettingKey::TargetFrameMs),
            RuntimeSettingValue::Float(ms),
        ) => settings.target_frame_ms = (*ms > 0.0).then_some(*ms as f32),
        (
            RuntimeSettingId::Engine(RuntimeSettingKey::LogLevel),
            RuntimeSettingValue::Text(level),
        ) => {
            crate::set_log_level(level);
        }
        _ => {}
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
        self.names.lock().clear();
    }

    pub fn register(&self, image: &GraphImage, name: impl Into<String>) {
        let name = name.into();
        image.register_as(name.clone());
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut names = self.names.lock();
        if !names.iter().any(|existing| existing == &name) {
            names.push(name);
        }
    }

    pub fn names(&self) -> Vec<String> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        self.names.lock().clone()
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
    TargetFrameMs,
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
    /// Controls the stdout log level at runtime.
    /// Values: "error", "warn", "info", "debug", "trace".
    LogLevel,
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
            Self::TargetFrameMs,
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
            Self::LogLevel,
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
            Self::AntiAliasingMode => RuntimeApplyPath::GraphRebuild,
            Self::AntiAliasingDial
            | Self::BloomEnabled
            | Self::BloomOnly
            | Self::ToneMappingOperator
            | Self::ToneMappingDial
            | Self::MotionDebugView
            | Self::OverlayVisibility
            | Self::ShaderHotReloadPolicy
            | Self::AssetHotReloadPolicy
            | Self::LatencyMode
            | Self::FramePacingMode
            | Self::MaxFramesInFlight
            | Self::ThreadedInputMode
            | Self::RenderThreadingMode
            | Self::TargetFrameMs
            | Self::LogLevel => RuntimeApplyPath::Immediate,
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
            Self::TargetFrameMs => "target_frame_ms",
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
            Self::LogLevel => "log_level",
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
            Self::TargetFrameMs => "target frame time",
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
            Self::LogLevel => "log level",
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
    /// Record that an existing setting should emit an apply notification
    /// without changing its value.
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
        let mut shared = self.controller.shared.lock();
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
            RuntimeSettingKey::TargetFrameMs,
            "Target Frame Time (ms)",
            RuntimeSettingKey::TargetFrameMs.apply_path(),
            settings.target_frame_ms.unwrap_or(0.0) as f64,
        )
        .with_description("GPU frame-time budget for adaptive render strategy. Set to 0 to disable automatic quality scaling."),
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
            "Hermite",
        )
        .with_options(text_options(&[
            "Aces",
            "Reinhard",
            "Hermite",
            "Linear",
            "PbrNeutral",
            "AgX",
            "PsychoV11",
            "PsychoV17",
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
            settings.shader_hot_reload_policy.clone(),
        )
        .with_options(text_options(&["Disabled", "Manual", "Automatic"])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::AssetHotReloadPolicy,
            "Asset Hot Reload Policy",
            RuntimeSettingKey::AssetHotReloadPolicy.apply_path(),
            settings.asset_hot_reload_policy.clone(),
        )
        .with_options(text_options(&["Disabled", "Manual", "Automatic"])),
        RuntimeSettingDescriptor::new(
            RuntimeSettingKey::LogLevel,
            "Log Level",
            RuntimeSettingKey::LogLevel.apply_path(),
            "warn",
        )
        .with_description("Stdout log verbosity. Changes take effect immediately.")
        .with_options(text_options(&["error", "warn", "info", "debug", "trace"])),
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
    viewport: clay_ui::Size,
    frame_number: u64,
}

impl<'a> UiContext<'a> {
    pub fn new(runtime: &'a mut AppRuntime, viewport: clay_ui::Size, frame_number: u64) -> Self {
        Self {
            runtime,
            viewport,
            frame_number,
        }
    }

    pub fn runtime(&mut self) -> &mut AppRuntime {
        self.runtime
    }
    pub fn clay(&mut self) -> &mut clay_ui::UiContext {
        &mut self.runtime.clay_ui
    }
    pub fn viewport(&self) -> clay_ui::Size {
        self.viewport
    }
    pub fn frame_number(&self) -> u64 {
        self.frame_number
    }

    pub fn build_frame(&mut self) -> clay_ui::UiFrameOutput {
        let limits = self.runtime.engine.caps().limits;
        self.runtime
            .clay_ui
            .build_frame_with_limits(self.viewport, self.frame_number, &limits, 1.0)
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

// ── Backend restart types ─────────────────────────────────────────────────────

/// Requested changes to the backend's active feature/extension set.
///
/// Pass to [`AppRuntime::request_backend_restart`].  Prefer [`BackendFeature`]
/// variants for compile-time safety; raw strings are also accepted as an escape
/// hatch for unlisted or experimental features.
///
/// ```ignore
/// runtime.request_backend_restart(
///     BackendFeatureChange::default()
///         .enable(BackendFeature::RayTracing)
///         .disable(BackendFeature::MeshShader),
/// );
/// ```
#[derive(Clone, Debug, Default)]
pub struct BackendFeatureChange {
    /// Feature names to additionally enable (added to `optional_features`).
    pub enable: Vec<String>,
    /// Feature names to disable (removed from `optional_features`/`required_features`).
    pub disable: Vec<String>,
}

impl BackendFeatureChange {
    /// Enable a feature.  Accepts [`BackendFeature`] variants or raw `&str`/`String`.
    pub fn enable(mut self, feature: impl Into<String>) -> Self {
        self.enable.push(feature.into());
        self
    }
    /// Disable a feature.  Accepts [`BackendFeature`] variants or raw `&str`/`String`.
    pub fn disable(mut self, feature: impl Into<String>) -> Self {
        self.disable.push(feature.into());
        self
    }
}

/// Outcome of a completed backend restart.
///
/// Passed to [`RuntimeApp::on_backend_restarted`] so the app can inspect
/// what the new backend actually supports.
#[derive(Clone, Debug)]
pub struct BackendRestartOutcome {
    /// The `DeviceDesc` that was used to create the new backend.
    pub new_desc: crate::DeviceDesc,
    /// Capability snapshot for the new backend.
    pub new_caps: crate::Caps,
}

// ── Benchmark harness ────────────────────────────────────────────────────────

/// Per-frame snapshot recorded during an active [`BenchmarkSession`].
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BenchmarkFrameSample {
    pub frame_index: u64,
    pub cpu_ms: f32,
    pub gpu_ms: Option<f32>,
    pub gpu_wait_ms: Option<f32>,
    pub pass_timings: Vec<BenchmarkPassSample>,
}

/// Per-pass GPU timing sample for one frame.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BenchmarkPassSample {
    pub name: String,
    pub gpu_ms: f32,
}

/// Aggregated statistics over a set of f32 samples.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FrameStats {
    pub min: f32,
    pub max: f32,
    pub mean: f32,
    pub p50: f32,
    pub p95: f32,
    pub p99: f32,
}

impl FrameStats {
    fn zero() -> Self {
        Self {
            min: 0.0,
            max: 0.0,
            mean: 0.0,
            p50: 0.0,
            p95: 0.0,
            p99: 0.0,
        }
    }

    fn from_samples(samples: &[f32]) -> Self {
        if samples.is_empty() {
            return Self::zero();
        }
        let mut sorted = samples.to_vec();
        sorted.sort_by(f32::total_cmp);
        let n = sorted.len();
        let min = sorted[0];
        let max = sorted[n - 1];
        let mean = sorted.iter().sum::<f32>() / n as f32;
        let p50 = sorted[(n as f32 * 0.5) as usize];
        let p95 = sorted[((n - 1) as f32 * 0.95) as usize];
        let p99 = sorted[((n - 1) as f32 * 0.99) as usize];
        Self {
            min,
            max,
            mean,
            p50,
            p95,
            p99,
        }
    }
}

/// Aggregated benchmark results produced by [`AppRuntime::stop_benchmark`].
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BenchmarkReport {
    pub frame_count: usize,
    pub cpu_ms: FrameStats,
    pub gpu_ms: Option<FrameStats>,
    pub per_pass: std::collections::HashMap<String, FrameStats>,
}

impl BenchmarkReport {
    /// Serialize the report to a pretty-printed JSON string.
    pub fn to_json(&self) -> std::result::Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

struct BenchmarkSession {
    frames: Vec<BenchmarkFrameSample>,
    pub(crate) next_frame_index: u64,
}

impl BenchmarkSession {
    fn new() -> Self {
        Self {
            frames: Vec::new(),
            next_frame_index: 0,
        }
    }

    fn record(&mut self, sample: BenchmarkFrameSample) {
        self.frames.push(sample);
        self.next_frame_index += 1;
    }

    fn finish(self) -> BenchmarkReport {
        let frame_count = self.frames.len();
        if frame_count == 0 {
            return BenchmarkReport {
                frame_count: 0,
                cpu_ms: FrameStats::zero(),
                gpu_ms: None,
                per_pass: Default::default(),
            };
        }
        let cpu_samples: Vec<f32> = self.frames.iter().map(|f| f.cpu_ms).collect();
        let gpu_samples: Vec<f32> = self.frames.iter().filter_map(|f| f.gpu_ms).collect();
        let mut pass_map: std::collections::HashMap<String, Vec<f32>> = Default::default();
        for frame in &self.frames {
            for pass in &frame.pass_timings {
                pass_map
                    .entry(pass.name.clone())
                    .or_default()
                    .push(pass.gpu_ms);
            }
        }
        BenchmarkReport {
            frame_count,
            cpu_ms: FrameStats::from_samples(&cpu_samples),
            gpu_ms: (!gpu_samples.is_empty()).then(|| FrameStats::from_samples(&gpu_samples)),
            per_pass: pass_map
                .into_iter()
                .map(|(name, s)| (name, FrameStats::from_samples(&s)))
                .collect(),
        }
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
