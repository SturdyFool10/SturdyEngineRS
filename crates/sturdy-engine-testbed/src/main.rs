use std::time::Instant;

use sturdy_engine::{
    AntiAliasingConfig, AntiAliasingDial, AntiAliasingPass, BloomConfig, BloomPass,
    CpuProceduralTexture2d, DebugOverlay, DebugOverlayRenderer, DebugViewPicker, Engine, EngineApp,
    Extent3d, Format, GpuProceduralTexture, HdrPipelineDesc, HdrPreference, ImageDesc,
    ImageDimension, ImageUsage, MotionVectorLayer, MotionVectorSpace, ProceduralTextureRecipe,
    ProceduralTextureUpdatePolicy, Result as EngineResult, RuntimeController,
    RuntimeMotionVectorDesc, RuntimePostProcessDesc, RuntimeSettingDescriptor, RuntimeSettingId,
    RuntimeSettingKey, RuntimeSettingOption, ShaderProgram, ShaderWatcher, ShellFrame, Surface,
    SurfaceColorSpace, SurfaceImage, ToneMappingOp, WindowConfig, push_constants,
};

#[push_constants]
struct FrameConstants {
    time: f32,
    aspect: f32,
    resolution: [f32; 2],
}

#[push_constants]
struct LutParams {
    phase: f32,
}

#[push_constants]
struct TonemapParams {
    tonemap_op: u32,
    hdr_output: u32,
    exposure: f32,
    white_point: f32,
    display_gain: f32,
    output_gamma: f32,
    aces_a: f32,
    aces_b: f32,
    aces_c: f32,
    aces_d: f32,
    aces_e: f32,
    reinhard_white: f32,
    hermite_contrast: f32,
    linear_white: f32,
}

#[derive(Clone, Copy, Debug)]
struct TonemapSettings {
    exposure: f32,
    white_point: f32,
    display_gain: f32,
    output_gamma: f32,
    aces_a: f32,
    aces_b: f32,
    aces_c: f32,
    aces_d: f32,
    aces_e: f32,
    reinhard_white: f32,
    hermite_contrast: f32,
    linear_white: f32,
}

impl Default for TonemapSettings {
    fn default() -> Self {
        Self {
            exposure: 1.0,
            white_point: 4.0,
            display_gain: 1.0,
            output_gamma: 2.2,
            aces_a: 2.51,
            aces_b: 0.03,
            aces_c: 2.43,
            aces_d: 0.59,
            aces_e: 0.14,
            reinhard_white: 4.0,
            hermite_contrast: 1.55,
            linear_white: 1.25,
        }
    }
}

impl TonemapSettings {
    fn params(
        self,
        tone_mapping: ToneMappingOp,
        hdr_output: bool,
        selected_dial: TonemapDial,
    ) -> TonemapParams {
        let mut settings = self;
        settings.sync_operator_white_point(tone_mapping, selected_dial);
        TonemapParams {
            tonemap_op: tone_mapping_id(tone_mapping),
            hdr_output: hdr_output as u32,
            exposure: settings.exposure,
            white_point: settings.white_point,
            display_gain: settings.display_gain,
            output_gamma: settings.output_gamma,
            aces_a: settings.aces_a,
            aces_b: settings.aces_b,
            aces_c: settings.aces_c,
            aces_d: settings.aces_d,
            aces_e: settings.aces_e,
            reinhard_white: settings.reinhard_white,
            hermite_contrast: settings.hermite_contrast,
            linear_white: settings.linear_white,
        }
    }

    fn get(self, dial: TonemapDial) -> f32 {
        match dial {
            TonemapDial::Exposure => self.exposure,
            TonemapDial::WhitePoint => self.white_point,
            TonemapDial::DisplayGain => self.display_gain,
            TonemapDial::OutputGamma => self.output_gamma,
            TonemapDial::AcesA => self.aces_a,
            TonemapDial::AcesB => self.aces_b,
            TonemapDial::AcesC => self.aces_c,
            TonemapDial::AcesD => self.aces_d,
            TonemapDial::AcesE => self.aces_e,
            TonemapDial::ReinhardWhite => self.reinhard_white,
            TonemapDial::HermiteContrast => self.hermite_contrast,
            TonemapDial::LinearWhite => self.linear_white,
        }
    }

    fn reset_for(&mut self, tone_mapping: ToneMappingOp) {
        let defaults = Self::default();
        self.exposure = defaults.exposure;
        self.white_point = defaults.white_point;
        self.display_gain = defaults.display_gain;
        self.output_gamma = defaults.output_gamma;
        match tone_mapping {
            ToneMappingOp::Aces => {
                self.aces_a = defaults.aces_a;
                self.aces_b = defaults.aces_b;
                self.aces_c = defaults.aces_c;
                self.aces_d = defaults.aces_d;
                self.aces_e = defaults.aces_e;
            }
            ToneMappingOp::Reinhard => self.reinhard_white = defaults.reinhard_white,
            ToneMappingOp::Hermite => self.hermite_contrast = defaults.hermite_contrast,
            ToneMappingOp::Linear => self.linear_white = defaults.linear_white,
        }
    }

    fn set(&mut self, dial: TonemapDial, value: f32) {
        match dial {
            TonemapDial::Exposure => self.exposure = value,
            TonemapDial::WhitePoint => self.white_point = value,
            TonemapDial::DisplayGain => self.display_gain = value,
            TonemapDial::OutputGamma => self.output_gamma = value,
            TonemapDial::AcesA => self.aces_a = value,
            TonemapDial::AcesB => self.aces_b = value,
            TonemapDial::AcesC => self.aces_c = value,
            TonemapDial::AcesD => self.aces_d = value,
            TonemapDial::AcesE => self.aces_e = value,
            TonemapDial::ReinhardWhite => self.reinhard_white = value,
            TonemapDial::HermiteContrast => self.hermite_contrast = value,
            TonemapDial::LinearWhite => self.linear_white = value,
        }
    }

    fn sync_operator_white_point(&mut self, tone_mapping: ToneMappingOp, changed: TonemapDial) {
        if changed != TonemapDial::WhitePoint {
            return;
        }
        match tone_mapping {
            ToneMappingOp::Reinhard => self.reinhard_white = self.white_point,
            ToneMappingOp::Linear => self.linear_white = self.white_point,
            ToneMappingOp::Aces | ToneMappingOp::Hermite => {}
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TonemapDial {
    Exposure,
    WhitePoint,
    DisplayGain,
    OutputGamma,
    AcesA,
    AcesB,
    AcesC,
    AcesD,
    AcesE,
    ReinhardWhite,
    HermiteContrast,
    LinearWhite,
}

impl TonemapDial {
    fn next(self) -> Self {
        match self {
            Self::Exposure => Self::WhitePoint,
            Self::WhitePoint => Self::DisplayGain,
            Self::DisplayGain => Self::OutputGamma,
            Self::OutputGamma => Self::AcesA,
            Self::AcesA => Self::AcesB,
            Self::AcesB => Self::AcesC,
            Self::AcesC => Self::AcesD,
            Self::AcesD => Self::AcesE,
            Self::AcesE => Self::ReinhardWhite,
            Self::ReinhardWhite => Self::HermiteContrast,
            Self::HermiteContrast => Self::LinearWhite,
            Self::LinearWhite => Self::Exposure,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Exposure => "exposure",
            Self::WhitePoint => "white point",
            Self::DisplayGain => "display gain",
            Self::OutputGamma => "SDR gamma",
            Self::AcesA => "ACES a",
            Self::AcesB => "ACES b",
            Self::AcesC => "ACES c",
            Self::AcesD => "ACES d",
            Self::AcesE => "ACES e",
            Self::ReinhardWhite => "Reinhard white",
            Self::HermiteContrast => "Hermite contrast",
            Self::LinearWhite => "Linear white",
        }
    }

    fn step(self) -> f32 {
        match self {
            Self::AcesB | Self::AcesE => 0.01,
            Self::OutputGamma | Self::HermiteContrast => 0.05,
            Self::AcesA | Self::AcesC | Self::AcesD => 0.1,
            Self::Exposure | Self::DisplayGain => 0.1,
            Self::WhitePoint | Self::ReinhardWhite | Self::LinearWhite => 0.25,
        }
    }

    fn min(self) -> f32 {
        match self {
            Self::AcesB | Self::AcesE => 0.0,
            Self::OutputGamma | Self::HermiteContrast => 0.2,
            Self::Exposure
            | Self::WhitePoint
            | Self::DisplayGain
            | Self::ReinhardWhite
            | Self::LinearWhite => 0.05,
            Self::AcesA | Self::AcesC | Self::AcesD => 0.01,
        }
    }

    fn max(self) -> f32 {
        match self {
            Self::OutputGamma => 4.0,
            Self::HermiteContrast => 3.0,
            Self::AcesB | Self::AcesE => 1.0,
            Self::AcesA | Self::AcesC | Self::AcesD => 8.0,
            Self::Exposure
            | Self::WhitePoint
            | Self::DisplayGain
            | Self::ReinhardWhite
            | Self::LinearWhite => 16.0,
        }
    }
}

struct Testbed {
    engine: Engine,
    scene_program: ShaderProgram,
    motion_program: ShaderProgram,
    tonemap_program: ShaderProgram,
    bloom_pass: BloomPass,
    aa_pass: AntiAliasingPass,
    bloom_config: BloomConfig,
    bloom_enabled: bool,
    bloom_only: bool,
    show_motion_vectors: bool,
    hdr_output: bool,
    tone_mapping: ToneMappingOp,
    tonemap_settings: TonemapSettings,
    selected_tonemap_dial: TonemapDial,
    aa: AntiAliasingConfig,
    color_lut: GpuProceduralTexture,
    procedural_mask: CpuProceduralTexture2d,
    debug_overlay: DebugOverlayRenderer,
    debug_view_picker: DebugViewPicker,
    runtime_controller: Option<RuntimeController>,
    texture_resolution: TextureResolutionTier,
    started_at: Instant,
    shader_watcher: ShaderWatcher,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TextureResolutionTier {
    Low,
    Medium,
    High,
}

impl TextureResolutionTier {
    fn label(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    fn size(self) -> u32 {
        match self {
            Self::Low => 256,
            Self::Medium => 512,
            Self::High => 1024,
        }
    }

    fn from_setting(value: &str) -> Option<Self> {
        match value {
            "low" | "Low" => Some(Self::Low),
            "medium" | "Medium" => Some(Self::Medium),
            "high" | "High" => Some(Self::High),
            _ => None,
        }
    }
}

impl EngineApp for Testbed {
    type Error = sturdy_engine::Error;

    fn init(engine: &Engine, surface: &Surface) -> EngineResult<Self> {
        let surface_info = surface.info();
        let hdr_caps = surface.hdr_caps()?;
        let hdr_desc =
            HdrPipelineDesc::select(&hdr_caps, &engine.caps(), HdrPreference::PreferHdr)?;
        let hdr_output = surface_is_hdr(surface_info.color_space);

        println!(
            "rendering on {:?} using {:?}; surface {:?}/{:?} at {}x{}",
            engine.adapter_name(),
            engine.backend_kind(),
            surface_info.format,
            surface_info.color_space,
            surface_info.size.width,
            surface_info.size.height,
        );
        println!(
            "HDR mode: {:?}, tone mapping: {:?}",
            hdr_desc.mode, hdr_desc.tone_mapping,
        );

        // GPU-driven color LUT: the generator shader receives a phase parameter
        // each frame and writes the gradient directly on the GPU — no CPU upload.
        let lut_program = engine.load_shader(shader_path("color_lut_gen.slang"))?;
        let color_lut = GpuProceduralTexture::new(
            engine,
            "color_lut",
            256,
            1,
            Format::Rgba8Unorm,
            lut_program,
        )?;
        let procedural_mask = CpuProceduralTexture2d::from_recipe_rgba8(
            engine,
            "procedural_mask",
            512,
            512,
            ProceduralTextureUpdatePolicy::Once,
            ProceduralTextureRecipe::RadialMask {
                inner_radius: 0.18,
                outer_radius: 1.0,
                color: [255, 255, 255, 255],
            },
        )?;

        let scene_program = engine.load_shader(shader_path("shader_graph_fragment.slang"))?;
        let motion_program = engine.load_shader(shader_path("motion_vectors.slang"))?;
        let tonemap_program = engine.load_shader(shader_path("tonemap.slang"))?;

        let mut shader_watcher = ShaderWatcher::new();
        for program in [&scene_program, &motion_program, &tonemap_program] {
            if let Some(path) = program.source_path() {
                shader_watcher.watch(path);
            }
        }

        Ok(Self {
            engine: engine.clone(),
            scene_program,
            motion_program,
            tonemap_program,
            bloom_pass: BloomPass::new(engine)?,
            aa_pass: AntiAliasingPass::new(engine)?,
            bloom_config: BloomConfig::default(),
            bloom_enabled: true,
            bloom_only: false,
            show_motion_vectors: false,
            hdr_output,
            tone_mapping: if hdr_output && hdr_desc.tone_mapping == ToneMappingOp::Linear {
                ToneMappingOp::Aces
            } else {
                hdr_desc.tone_mapping
            },
            tonemap_settings: TonemapSettings::default(),
            selected_tonemap_dial: TonemapDial::Exposure,
            aa: AntiAliasingConfig::default(),
            color_lut,
            procedural_mask,
            debug_overlay: DebugOverlayRenderer::new(engine)?,
            debug_view_picker: DebugViewPicker::new(engine)?,
            runtime_controller: None,
            texture_resolution: TextureResolutionTier::Medium,
            started_at: Instant::now(),
            shader_watcher,
        })
    }

    fn render(
        &mut self,
        frame: &mut ShellFrame<'_>,
        surface_image: &SurfaceImage,
    ) -> EngineResult<()> {
        let shell_frame = &*frame;
        let runtime_controller = shell_frame.runtime_controller();
        if self.runtime_controller.is_none() {
            self.register_runtime_settings(&runtime_controller)?;
            self.seed_runtime_settings(&runtime_controller)?;
            self.runtime_controller = Some(runtime_controller.clone());
        }

        // Poll for shader file changes and hot-reload any that have changed.
        let changed_paths = self.shader_watcher.poll_changed();
        for path in &changed_paths {
            let result = if path == self.scene_program.source_path().unwrap_or(path.as_path()) {
                self.scene_program.reload()
            } else if path == self.motion_program.source_path().unwrap_or(path.as_path()) {
                self.motion_program.reload()
            } else if path == self.tonemap_program.source_path().unwrap_or(path.as_path()) {
                self.tonemap_program.reload()
            } else {
                Ok(false)
            };
            match result {
                Ok(true) => {
                    runtime_controller.clear_shader_compile_error(path);
                    eprintln!("hot reload: reloaded {}", path.display());
                }
                Err(e) => {
                    runtime_controller.report_shader_compile_error(path, format!("{}", e));
                    eprintln!("hot reload: compile error in {}: {}", path.display(), e);
                }
                _ => {}
            }
        }

        let elapsed = self.started_at.elapsed().as_secs_f32();
        let ext = surface_image.desc().extent;

        // Register swapchain first — required so hdr_color_image can read the extent.
        let swapchain = shell_frame.inner().swapchain_image(surface_image)?;
        let scene_target =
            shell_frame.default_hdr_scene_target("scene_color", self.actual_msaa_samples())?;
        let scene_color =
            shell_frame.resolve_default_hdr_scene_target(&scene_target, "scene_color")?;
        let frame = frame.inner();

        // GPU procedural LUT: the generator shader runs on the GPU every frame.
        // The scene shader reads "color_lut" and will be scheduled after this pass.
        self.color_lut.generate_with_constants(
            frame,
            &LutParams {
                phase: elapsed * 0.4,
            },
        )?;
        self.procedural_mask.prepare(frame)?;

        // Pass 1: scene writes "scene_color".
        scene_target.execute_shader_with_constants_auto(
            &self.scene_program,
            &FrameConstants {
                time: elapsed,
                aspect: ext.width as f32 / ext.height.max(1) as f32,
                resolution: [ext.width as f32, ext.height as f32],
            },
        )?;
        let motion_vectors = self.motion_vector_image(frame, ext.width, ext.height)?;
        motion_vectors.execute_shader_with_constants_auto(
            &self.motion_program,
            &FrameConstants {
                time: elapsed,
                aspect: ext.width as f32 / ext.height.max(1) as f32,
                resolution: [ext.width as f32, ext.height as f32],
            },
        )?;

        let tonemap_constants = self.tonemap_settings.params(
            self.tone_mapping,
            self.hdr_output,
            self.selected_tonemap_dial,
        );
        let _post = shell_frame.run_default_post_process(RuntimePostProcessDesc {
            scene_color: &scene_color,
            motion_vectors: Some(RuntimeMotionVectorDesc {
                image: &motion_vectors,
                space: MotionVectorSpace::CameraLocal,
                layer: MotionVectorLayer::World,
            }),
            bloom_pass: self.bloom_enabled.then_some(&self.bloom_pass),
            bloom_config: self.bloom_enabled.then_some(&self.bloom_config),
            bloom_only: self.bloom_only,
            aa_pass: &self.aa_pass,
            aa_mode: self.aa.mode,
            swapchain: &swapchain,
            tonemap_program: &self.tonemap_program,
            tonemap_constants: &tonemap_constants,
        })?;
        shell_frame.publish_runtime_diagnostics(
            self.aa.mode.label(),
            self.actual_msaa_samples(),
            self.bloom_enabled,
            self.bloom_only,
        );
        let _ = self
            .debug_view_picker
            .present_selected(shell_frame, &swapchain)?;
        if runtime_controller
            .bool_setting(RuntimeSettingKey::OverlayVisibility)
            .unwrap_or(true)
        {
            let mut overlay_lines = shell_frame.default_runtime_overlay_lines();
            overlay_lines.push(format!(
                "controls: T={} P={}={:.2}",
                tone_mapping_label(self.tone_mapping),
                self.selected_tonemap_dial.label(),
                self.tonemap_settings.get(self.selected_tonemap_dial),
            ));
            overlay_lines.push(format!(
                "controls: D={} V={} H={} T={} FX={} tex={}",
                self.aa.selected_dial.label(),
                if self.show_motion_vectors {
                    "shown"
                } else {
                    "hidden"
                },
                if self.hdr_output { "on" } else { "off" },
                if runtime_controller
                    .bool_setting(RuntimeSettingKey::SurfaceTransparency)
                    .unwrap_or(false)
                {
                    "on"
                } else {
                    "off"
                },
                runtime_controller
                    .text_setting(RuntimeSettingKey::WindowBackgroundEffect)
                    .unwrap_or_else(|| "None".to_string()),
                self.texture_resolution.label(),
            ));
            overlay_lines.push(format!(
                "debug view: {}",
                self.debug_view_picker
                    .selected_name(&runtime_controller)
                    .unwrap_or_else(|| "Off".to_string())
            ));
            overlay_lines.push(
                "keys: [/]=tonemap .=aa R/U reset B/b bloom H hdr V motion O overlay X transparency G effect N/M debug 1/2/3 tex"
                    .to_string(),
            );
            // Show any active shader compile errors (cleared automatically on successful hot reload).
            for err in runtime_controller.diagnostics().shader_compile_errors {
                overlay_lines.push(format!(
                    "[shader error] {}: {}",
                    err.path
                        .file_name()
                        .unwrap_or(err.path.as_os_str())
                        .to_string_lossy(),
                    err.message.lines().next().unwrap_or("compile failed"),
                ));
            }
            shell_frame.set_runtime_overlay_lines(overlay_lines);
        }

        shell_frame.run_camera_locked_pass("hud_overlay", &swapchain, |frame, target| {
            self.draw_hud(shell_frame, frame, target, ext.width, ext.height)
        })?;
        frame.present_image(&swapchain)?;

        // In debug builds, validate the recorded graph and print any diagnostics.
        #[cfg(debug_assertions)]
        for d in frame.validate() {
            eprintln!("[graph {:?}] {}", d.level, d.message);
        }

        Ok(())
    }

    fn key_pressed(&mut self, key: &str, _surface: &mut Surface) -> EngineResult<()> {
        let mut runtime_controller = self.runtime_controller.clone();
        if key == "b" {
            if let Some(controller) = runtime_controller.as_mut() {
                let next = !controller
                    .bool_setting(RuntimeSettingKey::BloomOnly)
                    .unwrap_or(self.bloom_only);
                controller
                    .transact()
                    .set_engine_value(RuntimeSettingKey::BloomOnly, next)
                    .apply()?;
                eprintln!("bloom-only: {next}");
            }
        } else if key == "B" {
            if let Some(controller) = runtime_controller.as_mut() {
                let next = !controller
                    .bool_setting(RuntimeSettingKey::BloomEnabled)
                    .unwrap_or(self.bloom_enabled);
                controller
                    .transact()
                    .set_engine_value(RuntimeSettingKey::BloomEnabled, next)
                    .apply()?;
                eprintln!("bloom: {}", if next { "on" } else { "off" });
            }
        } else if key == "V" || key == "v" {
            if let Some(controller) = runtime_controller.as_mut() {
                let next = !controller
                    .bool_setting(RuntimeSettingKey::MotionDebugView)
                    .unwrap_or(self.show_motion_vectors);
                controller
                    .transact()
                    .set_engine_value(RuntimeSettingKey::MotionDebugView, next)
                    .apply()?;
                eprintln!("motion vectors: {}", if next { "shown" } else { "hidden" });
            }
        } else if key == "T" || key == "t" {
            if let Some(controller) = runtime_controller.as_mut() {
                let next = next_tone_mapping(self.tone_mapping);
                controller
                    .transact()
                    .set_engine_value(
                        RuntimeSettingKey::ToneMappingOperator,
                        tone_mapping_setting_name(next),
                    )
                    .apply()?;
                eprintln!("tone mapping: {}", tone_mapping_label(next));
            }
        } else if key == "H" || key == "h" {
            if let Some(controller) = runtime_controller.as_mut() {
                let next = !controller
                    .bool_setting(RuntimeSettingKey::HdrMode)
                    .unwrap_or(self.hdr_output);
                controller
                    .transact()
                    .set_engine_value(RuntimeSettingKey::HdrMode, next)
                    .apply()?;
                eprintln!("HDR output requested: {}", if next { "on" } else { "off" });
            }
        } else if key == "P" || key == "p" {
            self.selected_tonemap_dial = self.selected_tonemap_dial.next();
            eprintln!(
                "tonemap dial: {} = {:.3}",
                self.selected_tonemap_dial.label(),
                self.tonemap_settings.get(self.selected_tonemap_dial),
            );
        } else if key == "A" || key == "a" {
            let mut next = self.aa.clone();
            next.next_mode();
            if let Some(controller) = runtime_controller.as_mut() {
                controller
                    .transact()
                    .set_engine_value(
                        RuntimeSettingKey::AntiAliasingMode,
                        aa_mode_setting_name(next.mode),
                    )
                    .apply()?;
            }
            eprintln!("aa mode: {}", next.mode.label());
        } else if key == "D" || key == "d" {
            self.aa.cycle_dial();
            eprintln!("aa dial: {}", self.aa.selected_dial.label());
        } else if key == "O" || key == "o" {
            if let Some(controller) = runtime_controller.as_mut() {
                let visible = !controller
                    .bool_setting(RuntimeSettingKey::OverlayVisibility)
                    .unwrap_or(true);
                controller
                    .transact()
                    .set_engine_value(RuntimeSettingKey::OverlayVisibility, visible)
                    .apply()?;
                eprintln!("overlay: {}", if visible { "shown" } else { "hidden" });
            }
        } else if key == "X" || key == "x" {
            if let Some(controller) = runtime_controller.as_mut() {
                let enabled = !controller
                    .bool_setting(RuntimeSettingKey::SurfaceTransparency)
                    .unwrap_or(false);
                controller
                    .transact()
                    .set_engine_value(RuntimeSettingKey::SurfaceTransparency, enabled)
                    .apply()?;
                eprintln!(
                    "surface transparency: {}",
                    if enabled { "on" } else { "off" }
                );
            }
        } else if key == "G" || key == "g" {
            self.cycle_window_background_effect()?;
        } else if key == "N" || key == "n" {
            if let Some(controller) = runtime_controller.as_mut() {
                let selection = self
                    .debug_view_picker
                    .cycle_next(controller, &self.current_debug_image_names())?;
                eprintln!(
                    "debug view: {}",
                    selection.unwrap_or_else(|| "Off".to_string())
                );
            }
        } else if key == "M" || key == "m" {
            if let Some(controller) = runtime_controller.as_mut() {
                let selection = self
                    .debug_view_picker
                    .cycle_previous(controller, &self.current_debug_image_names())?;
                eprintln!(
                    "debug view: {}",
                    selection.unwrap_or_else(|| "Off".to_string())
                );
            }
        } else if key == "1" {
            self.set_texture_resolution_setting(TextureResolutionTier::Low)?;
        } else if key == "2" {
            self.set_texture_resolution_setting(TextureResolutionTier::Medium)?;
        } else if key == "3" {
            self.set_texture_resolution_setting(TextureResolutionTier::High)?;
        } else if key == "]" || key == "=" || key == "+" {
            let value = self.preview_tonemap_dial(1.0);
            if let Some(controller) = runtime_controller.as_mut() {
                controller
                    .transact()
                    .set_engine_value(RuntimeSettingKey::ToneMappingDial, value as f64)
                    .apply()?;
            }
            eprintln!(
                "{} {}: {:.3}",
                tone_mapping_label(self.tone_mapping),
                self.selected_tonemap_dial.label(),
                value
            );
        } else if key == "[" || key == "-" || key == "_" {
            let value = self.preview_tonemap_dial(-1.0);
            if let Some(controller) = runtime_controller.as_mut() {
                controller
                    .transact()
                    .set_engine_value(RuntimeSettingKey::ToneMappingDial, value as f64)
                    .apply()?;
            }
            eprintln!(
                "{} {}: {:.3}",
                tone_mapping_label(self.tone_mapping),
                self.selected_tonemap_dial.label(),
                value
            );
        } else if key == "." || key == ">" {
            let value = self.preview_aa_dial(1.0);
            if let Some(controller) = runtime_controller.as_mut() {
                controller
                    .transact()
                    .set_engine_value(RuntimeSettingKey::AntiAliasingDial, value as f64)
                    .apply()?;
            }
            eprintln!("aa {}: {:.3}", self.aa.selected_dial.label(), value);
        } else if key == "," || key == "<" {
            let value = self.preview_aa_dial(-1.0);
            if let Some(controller) = runtime_controller.as_mut() {
                controller
                    .transact()
                    .set_engine_value(RuntimeSettingKey::AntiAliasingDial, value as f64)
                    .apply()?;
            }
            eprintln!("aa {}: {:.3}", self.aa.selected_dial.label(), value);
        } else if key == "R" || key == "r" {
            self.tonemap_settings.reset_for(self.tone_mapping);
            if let Some(controller) = runtime_controller.as_mut() {
                controller
                    .transact()
                    .set_engine_value(
                        RuntimeSettingKey::ToneMappingDial,
                        self.tonemap_settings.get(self.selected_tonemap_dial) as f64,
                    )
                    .apply()?;
            }
            eprintln!(
                "reset {} tonemap dials",
                tone_mapping_label(self.tone_mapping)
            );
        } else if key == "U" || key == "u" {
            self.aa = AntiAliasingConfig::default();
            if let Some(controller) = runtime_controller.as_mut() {
                controller
                    .transact()
                    .set_engine_value(
                        RuntimeSettingKey::AntiAliasingMode,
                        aa_mode_setting_name(self.aa.mode),
                    )
                    .set_engine_value(
                        RuntimeSettingKey::AntiAliasingDial,
                        self.current_aa_dial_value() as f64,
                    )
                    .apply()?;
            }
            eprintln!("reset aa dials");
        }
        Ok(())
    }

    fn runtime_settings_changed(
        &mut self,
        controller: &RuntimeController,
        changes: &[sturdy_engine::RuntimeSettingChange],
        surface: &mut Surface,
    ) -> EngineResult<()> {
        self.runtime_controller = Some(controller.clone());
        self.apply_runtime_settings(controller, changes, surface)
    }

    fn resize(&mut self, _width: u32, _height: u32) -> EngineResult<()> {
        Ok(())
    }
}

impl Testbed {
    fn register_runtime_settings(&mut self, controller: &RuntimeController) -> EngineResult<()> {
        controller.register_app_setting(
            RuntimeSettingDescriptor::new(
                RuntimeSettingId::app("testbed.texture_resolution"),
                "Texture Resolution",
                sturdy_engine::RuntimeApplyPath::Immediate,
                self.texture_resolution.label(),
            )
            .with_description("Swap the procedural mask texture resolution immediately.")
            .with_options(vec![
                RuntimeSettingOption {
                    value: "low".into(),
                    label: "Low".to_string(),
                },
                RuntimeSettingOption {
                    value: "medium".into(),
                    label: "Medium".to_string(),
                },
                RuntimeSettingOption {
                    value: "high".into(),
                    label: "High".to_string(),
                },
            ]),
        )?;
        self.debug_view_picker.register(controller)?;
        Ok(())
    }

    fn seed_runtime_settings(&mut self, controller: &RuntimeController) -> EngineResult<()> {
        let mut controller = controller.clone();
        controller
            .transact()
            .set_engine_value(RuntimeSettingKey::BloomEnabled, self.bloom_enabled)
            .set_engine_value(RuntimeSettingKey::BloomOnly, self.bloom_only)
            .set_engine_value(RuntimeSettingKey::MotionDebugView, self.show_motion_vectors)
            .set_engine_value(RuntimeSettingKey::HdrMode, self.hdr_output)
            .set_engine_value(
                RuntimeSettingKey::ToneMappingOperator,
                tone_mapping_setting_name(self.tone_mapping),
            )
            .set_engine_value(
                RuntimeSettingKey::ToneMappingDial,
                self.tonemap_settings.get(self.selected_tonemap_dial) as f64,
            )
            .set_engine_value(
                RuntimeSettingKey::AntiAliasingMode,
                aa_mode_setting_name(self.aa.mode),
            )
            .set_engine_value(
                RuntimeSettingKey::AntiAliasingDial,
                self.current_aa_dial_value() as f64,
            )
            .set_engine_value(RuntimeSettingKey::OverlayVisibility, true)
            .set_app_value(
                "testbed.texture_resolution",
                self.texture_resolution.label(),
            )
            .apply()?;
        Ok(())
    }

    fn apply_runtime_settings(
        &mut self,
        controller: &RuntimeController,
        changes: &[sturdy_engine::RuntimeSettingChange],
        surface: &Surface,
    ) -> EngineResult<()> {
        self.bloom_enabled = controller
            .bool_setting(RuntimeSettingKey::BloomEnabled)
            .unwrap_or(self.bloom_enabled);
        self.bloom_only = controller
            .bool_setting(RuntimeSettingKey::BloomOnly)
            .unwrap_or(self.bloom_only);
        self.show_motion_vectors = controller
            .bool_setting(RuntimeSettingKey::MotionDebugView)
            .unwrap_or(self.show_motion_vectors);
        self.hdr_output = surface_is_hdr(surface.info().color_space);
        if let Some(tone_mapping) = controller
            .text_setting(RuntimeSettingKey::ToneMappingOperator)
            .and_then(|value| parse_tone_mapping_setting(&value))
        {
            self.tone_mapping = tone_mapping;
        }
        if let Some(aa_mode) = controller
            .text_setting(RuntimeSettingKey::AntiAliasingMode)
            .and_then(|value| parse_aa_mode_setting(&value, self.actual_msaa_samples()))
        {
            self.aa.mode = aa_mode;
        }

        for change in changes {
            if change.setting == RuntimeSettingId::from(RuntimeSettingKey::ToneMappingDial)
                && let sturdy_engine::RuntimeSettingValue::Float(value) = change.value
            {
                self.apply_tonemap_dial_value(value as f32);
            }
            if change.setting == RuntimeSettingId::from(RuntimeSettingKey::AntiAliasingDial)
                && let sturdy_engine::RuntimeSettingValue::Float(value) = change.value
            {
                self.apply_aa_dial_value(value as f32);
            }
            if change.setting == RuntimeSettingId::app("testbed.texture_resolution")
                && let sturdy_engine::RuntimeSettingValue::Text(value) = &change.value
                && let Some(tier) = TextureResolutionTier::from_setting(value)
            {
                self.recreate_procedural_mask(tier)?;
            }
        }
        Ok(())
    }

    fn recreate_procedural_mask(&mut self, tier: TextureResolutionTier) -> EngineResult<()> {
        if self.texture_resolution == tier {
            return Ok(());
        }
        self.procedural_mask = CpuProceduralTexture2d::from_recipe_rgba8(
            &self.engine,
            "procedural_mask",
            tier.size(),
            tier.size(),
            ProceduralTextureUpdatePolicy::Once,
            ProceduralTextureRecipe::RadialMask {
                inner_radius: 0.18,
                outer_radius: 1.0,
                color: [255, 255, 255, 255],
            },
        )?;
        self.texture_resolution = tier;
        eprintln!("texture resolution: {}", tier.label());
        Ok(())
    }

    fn current_debug_image_names(&self) -> Vec<String> {
        self.runtime_controller
            .as_ref()
            .map(|controller| controller.diagnostics().debug_images)
            .unwrap_or_default()
    }

    fn set_texture_resolution_setting(&mut self, tier: TextureResolutionTier) -> EngineResult<()> {
        if let Some(controller) = self.runtime_controller.as_mut() {
            controller
                .transact()
                .set_app_value("testbed.texture_resolution", tier.label())
                .apply()?;
        } else {
            self.recreate_procedural_mask(tier)?;
        }
        Ok(())
    }

    fn cycle_window_background_effect(&mut self) -> EngineResult<()> {
        let Some(controller) = self.runtime_controller.as_mut() else {
            return Ok(());
        };
        let entry = match controller.setting_entry(RuntimeSettingKey::WindowBackgroundEffect) {
            Some(entry) => entry,
            None => return Ok(()),
        };
        let options = entry.descriptor.options;
        if options.is_empty() {
            return Ok(());
        }
        let current = controller
            .text_setting(RuntimeSettingKey::WindowBackgroundEffect)
            .unwrap_or_else(|| "None".to_string());
        let current_index = options
            .iter()
            .position(|option| option.value.serialized() == current)
            .unwrap_or(0);
        let next = &options[(current_index + 1) % options.len()];
        controller
            .transact()
            .set_engine_value(
                RuntimeSettingKey::WindowBackgroundEffect,
                next.value.serialized(),
            )
            .apply()?;
        eprintln!("window background effect: {}", next.label);
        Ok(())
    }

    fn preview_tonemap_dial(&self, direction: f32) -> f32 {
        let dial = self.selected_tonemap_dial;
        (self.tonemap_settings.get(dial) + dial.step() * direction).clamp(dial.min(), dial.max())
    }

    fn apply_tonemap_dial_value(&mut self, value: f32) {
        self.tonemap_settings.set(
            self.selected_tonemap_dial,
            value.clamp(
                self.selected_tonemap_dial.min(),
                self.selected_tonemap_dial.max(),
            ),
        );
        self.tonemap_settings
            .sync_operator_white_point(self.tone_mapping, self.selected_tonemap_dial);
    }

    fn preview_aa_dial(&self, direction: f32) -> f32 {
        let mut preview = self.aa.clone();
        preview.adjust(direction, self.engine.caps().max_color_sample_count);
        aa_dial_value(preview.mode, preview.selected_dial)
    }

    fn apply_aa_dial_value(&mut self, value: f32) {
        if self.aa.selected_dial == AntiAliasingDial::Mode {
            return;
        }
        apply_aa_value(
            &mut self.aa,
            value,
            self.engine.caps().max_color_sample_count,
        );
    }

    fn current_aa_dial_value(&self) -> f32 {
        aa_dial_value(self.aa.mode, self.aa.selected_dial)
    }

    fn draw_hud(
        &mut self,
        shell_frame: &ShellFrame<'_>,
        frame: &sturdy_engine::RenderFrame,
        target: &sturdy_engine::GraphImage,
        width: u32,
        height: u32,
    ) -> EngineResult<()> {
        let hud_text = std::iter::once("SturdyEngine testbed".to_string())
            .chain(shell_frame.runtime_overlay_lines())
            .chain(std::iter::once(
                "Resize window to test graph image recreation\nClose window to exit".to_string(),
            ))
            .collect::<Vec<_>>()
            .join("\n");
        let mut overlay = DebugOverlay::new();
        overlay.add_screen_text(hud_text, 18.0, 18.0);
        self.debug_overlay
            .draw(frame, target, width, height, &overlay)
    }

    fn actual_msaa_samples(&self) -> u8 {
        self.aa
            .mode
            .msaa_samples()
            .clamp(1, self.engine.caps().max_color_sample_count.max(1))
            .min(16)
    }

    fn motion_vector_image(
        &self,
        frame: &sturdy_engine::RenderFrame,
        width: u32,
        height: u32,
    ) -> EngineResult<sturdy_engine::GraphImage> {
        frame.image(
            "motion_vectors",
            ImageDesc {
                dimension: ImageDimension::D2,
                extent: Extent3d {
                    width: width.max(1),
                    height: height.max(1),
                    depth: 1,
                },
                mip_levels: 1,
                layers: 1,
                samples: 1,
                format: Format::Rgba16Float,
                usage: ImageUsage::SAMPLED | ImageUsage::RENDER_TARGET,
                transient: false,
                clear_value: None,
                debug_name: Some("testbed motion vector"),
            },
        )
    }
}

fn next_tone_mapping(op: ToneMappingOp) -> ToneMappingOp {
    match op {
        ToneMappingOp::Aces => ToneMappingOp::Reinhard,
        ToneMappingOp::Reinhard => ToneMappingOp::Hermite,
        ToneMappingOp::Hermite => ToneMappingOp::Linear,
        ToneMappingOp::Linear => ToneMappingOp::Aces,
    }
}

fn tone_mapping_id(op: ToneMappingOp) -> u32 {
    match op {
        ToneMappingOp::Aces => 0,
        ToneMappingOp::Reinhard => 1,
        ToneMappingOp::Hermite => 2,
        ToneMappingOp::Linear => 3,
    }
}

fn tone_mapping_label(op: ToneMappingOp) -> &'static str {
    match op {
        ToneMappingOp::Aces => "ACES",
        ToneMappingOp::Reinhard => "Reinhard",
        ToneMappingOp::Hermite => "Hermite",
        ToneMappingOp::Linear => "Linear",
    }
}

fn tone_mapping_setting_name(op: ToneMappingOp) -> &'static str {
    match op {
        ToneMappingOp::Aces => "Aces",
        ToneMappingOp::Reinhard => "Reinhard",
        ToneMappingOp::Hermite => "Hermite",
        ToneMappingOp::Linear => "Linear",
    }
}

fn parse_tone_mapping_setting(value: &str) -> Option<ToneMappingOp> {
    match value {
        "Aces" | "ACES" => Some(ToneMappingOp::Aces),
        "Reinhard" => Some(ToneMappingOp::Reinhard),
        "Hermite" => Some(ToneMappingOp::Hermite),
        "Linear" => Some(ToneMappingOp::Linear),
        _ => None,
    }
}

fn aa_mode_setting_name(mode: sturdy_engine::AntiAliasingMode) -> &'static str {
    match mode {
        sturdy_engine::AntiAliasingMode::Off => "Off",
        sturdy_engine::AntiAliasingMode::Msaa(_) => "MSAA",
        sturdy_engine::AntiAliasingMode::Fxaa(_) => "FXAA",
        sturdy_engine::AntiAliasingMode::Taa(_) => "TAA",
        sturdy_engine::AntiAliasingMode::FxaaTaa { .. } => "FXAA+TAA",
    }
}

fn parse_aa_mode_setting(
    value: &str,
    current_msaa_samples: u8,
) -> Option<sturdy_engine::AntiAliasingMode> {
    match value {
        "Off" | "off" => Some(sturdy_engine::AntiAliasingMode::Off),
        "MSAA" => Some(sturdy_engine::AntiAliasingMode::Msaa(
            sturdy_engine::MsaaSettings {
                samples: current_msaa_samples.max(1),
            },
        )),
        "FXAA" => Some(sturdy_engine::AntiAliasingMode::Fxaa(Default::default())),
        "TAA" => Some(sturdy_engine::AntiAliasingMode::Taa(Default::default())),
        "FXAA+TAA" => Some(sturdy_engine::AntiAliasingMode::FxaaTaa {
            fxaa: Default::default(),
            taa: Default::default(),
        }),
        _ => None,
    }
}

fn aa_dial_value(mode: sturdy_engine::AntiAliasingMode, dial: AntiAliasingDial) -> f32 {
    match (mode, dial) {
        (sturdy_engine::AntiAliasingMode::Msaa(settings), AntiAliasingDial::MsaaSamples) => {
            settings.samples as f32
        }
        (
            sturdy_engine::AntiAliasingMode::Fxaa(settings),
            AntiAliasingDial::FxaaSubpixelQuality,
        )
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { fxaa: settings, .. },
            AntiAliasingDial::FxaaSubpixelQuality,
        ) => settings.subpixel_quality,
        (sturdy_engine::AntiAliasingMode::Fxaa(settings), AntiAliasingDial::FxaaEdgeThreshold)
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { fxaa: settings, .. },
            AntiAliasingDial::FxaaEdgeThreshold,
        ) => settings.edge_threshold,
        (
            sturdy_engine::AntiAliasingMode::Fxaa(settings),
            AntiAliasingDial::FxaaEdgeThresholdMin,
        )
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { fxaa: settings, .. },
            AntiAliasingDial::FxaaEdgeThresholdMin,
        ) => settings.edge_threshold_min,
        (sturdy_engine::AntiAliasingMode::Taa(settings), AntiAliasingDial::TaaHistoryWeight)
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { taa: settings, .. },
            AntiAliasingDial::TaaHistoryWeight,
        ) => settings.history_weight,
        (sturdy_engine::AntiAliasingMode::Taa(settings), AntiAliasingDial::TaaJitterScale)
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { taa: settings, .. },
            AntiAliasingDial::TaaJitterScale,
        ) => settings.jitter_scale,
        (sturdy_engine::AntiAliasingMode::Taa(settings), AntiAliasingDial::TaaClampFactor)
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { taa: settings, .. },
            AntiAliasingDial::TaaClampFactor,
        ) => settings.clamp_factor,
        _ => 1.0,
    }
}

fn apply_aa_value(config: &mut AntiAliasingConfig, value: f32, max_msaa_samples: u8) {
    match (&mut config.mode, config.selected_dial) {
        (sturdy_engine::AntiAliasingMode::Msaa(settings), AntiAliasingDial::MsaaSamples) => {
            let rounded = value.round().clamp(1.0, max_msaa_samples.max(1) as f32);
            let candidates = [1.0_f32, 2.0, 4.0, 8.0, 16.0];
            settings.samples = candidates
                .into_iter()
                .min_by(|left, right| {
                    (left - rounded)
                        .abs()
                        .partial_cmp(&(right - rounded).abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap_or(1.0)
                .min(max_msaa_samples.max(1) as f32) as u8;
        }
        (
            sturdy_engine::AntiAliasingMode::Fxaa(settings),
            AntiAliasingDial::FxaaSubpixelQuality,
        )
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { fxaa: settings, .. },
            AntiAliasingDial::FxaaSubpixelQuality,
        ) => settings.subpixel_quality = value.clamp(0.0, 1.0),
        (sturdy_engine::AntiAliasingMode::Fxaa(settings), AntiAliasingDial::FxaaEdgeThreshold)
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { fxaa: settings, .. },
            AntiAliasingDial::FxaaEdgeThreshold,
        ) => settings.edge_threshold = value.clamp(0.0, 1.0),
        (
            sturdy_engine::AntiAliasingMode::Fxaa(settings),
            AntiAliasingDial::FxaaEdgeThresholdMin,
        )
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { fxaa: settings, .. },
            AntiAliasingDial::FxaaEdgeThresholdMin,
        ) => settings.edge_threshold_min = value.clamp(0.0, 1.0),
        (sturdy_engine::AntiAliasingMode::Taa(settings), AntiAliasingDial::TaaHistoryWeight)
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { taa: settings, .. },
            AntiAliasingDial::TaaHistoryWeight,
        ) => settings.history_weight = value.clamp(0.0, 1.0),
        (sturdy_engine::AntiAliasingMode::Taa(settings), AntiAliasingDial::TaaJitterScale)
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { taa: settings, .. },
            AntiAliasingDial::TaaJitterScale,
        ) => settings.jitter_scale = value.max(0.0),
        (sturdy_engine::AntiAliasingMode::Taa(settings), AntiAliasingDial::TaaClampFactor)
        | (
            sturdy_engine::AntiAliasingMode::FxaaTaa { taa: settings, .. },
            AntiAliasingDial::TaaClampFactor,
        ) => settings.clamp_factor = value.max(0.0),
        _ => {}
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

fn shader_path(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(name)
}

fn main() {
    sturdy_engine::run::<Testbed>(
        WindowConfig::new("SturdyEngine HDR bloom testbed", 1280, 720)
            .with_resizable(true)
            .with_hdr(true),
    );
}
