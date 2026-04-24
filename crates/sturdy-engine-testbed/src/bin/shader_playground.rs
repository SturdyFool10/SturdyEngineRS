use std::{path::PathBuf, time::Instant};

use sturdy_engine::{
    AntiAliasingMode, AntiAliasingPass, CpuProceduralTexture2d, DebugOverlay,
    DebugOverlayRenderer, DebugViewPicker, Engine, EngineApp, HdrPipelineDesc, HdrPreference,
    ProceduralTextureRecipe, ProceduralTextureUpdatePolicy, Result, RuntimeController,
    RuntimePostProcessDesc, ShaderProgram, ShellFrame, Surface, SurfaceImage, WindowConfig,
    push_constants,
};

#[push_constants]
struct PlaygroundConstants {
    time: f32,
    frame: u32,
    paused: u32,
    resolution: [f32; 2],
    aspect: f32,
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

struct ShaderPlayground {
    scene_program: ShaderProgram,
    tonemap_program: ShaderProgram,
    aa_pass: AntiAliasingPass,
    procedural_mask: CpuProceduralTexture2d,
    overlay: DebugOverlayRenderer,
    debug_view_picker: DebugViewPicker,
    runtime_controller: Option<RuntimeController>,
    playhead_time: f32,
    paused: bool,
    pending_step_frames: u32,
    scrub_delta: f32,
    frame_index: u32,
    last_frame_started: Instant,
}

impl EngineApp for ShaderPlayground {
    type Error = sturdy_engine::Error;

    fn init(engine: &Engine, surface: &Surface) -> Result<Self> {
        let hdr_caps = surface.hdr_caps()?;
        let _hdr_desc =
            HdrPipelineDesc::select(&hdr_caps, &engine.caps(), HdrPreference::PreferHdr)?;

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

        Ok(Self {
            scene_program: engine.load_shader(shader_path("shader_playground_fragment.slang"))?,
            tonemap_program: engine.load_shader(shader_path("tonemap.slang"))?,
            aa_pass: AntiAliasingPass::new(engine)?,
            procedural_mask,
            overlay: DebugOverlayRenderer::new(engine)?,
            debug_view_picker: DebugViewPicker::new(engine)?,
            runtime_controller: None,
            playhead_time: 0.0,
            paused: false,
            pending_step_frames: 0,
            scrub_delta: 0.0,
            frame_index: 0,
            last_frame_started: Instant::now(),
        })
    }

    fn render(&mut self, frame: &mut ShellFrame<'_>, surface_image: &SurfaceImage) -> Result<()> {
        let controller = frame.runtime_controller();
        if self.runtime_controller.is_none() {
            self.debug_view_picker.register(&controller)?;
            self.runtime_controller = Some(controller.clone());
        }

        let now = Instant::now();
        let delta = (now - self.last_frame_started).as_secs_f32();
        self.last_frame_started = now;
        if !self.paused {
            self.playhead_time += delta;
        }
        if self.pending_step_frames > 0 {
            self.playhead_time += self.pending_step_frames as f32 / 60.0;
            self.pending_step_frames = 0;
        }
        if self.scrub_delta != 0.0 {
            self.playhead_time = (self.playhead_time + self.scrub_delta).max(0.0);
            self.scrub_delta = 0.0;
        }

        let ext = surface_image.desc().extent;
        let swapchain = frame.inner().swapchain_image(surface_image)?;
        let scene_target = frame.default_hdr_scene_target("playground_scene", 1)?;
        let scene_color = frame.resolve_default_hdr_scene_target(&scene_target, "playground_scene")?;
        let render_frame = frame.inner();

        self.procedural_mask.prepare(render_frame)?;

        scene_target.execute_shader_with_constants_auto(
            &self.scene_program,
            &PlaygroundConstants {
                time: self.playhead_time,
                frame: self.frame_index,
                paused: self.paused as u32,
                resolution: [ext.width as f32, ext.height as f32],
                aspect: ext.width as f32 / ext.height.max(1) as f32,
            },
        )?;

        let _ = frame.run_default_post_process(RuntimePostProcessDesc {
            scene_color: &scene_color,
            motion_vectors: None,
            bloom_pass: None,
            bloom_config: None,
            bloom_only: false,
            aa_pass: &self.aa_pass,
            aa_mode: AntiAliasingMode::Off,
            swapchain: &swapchain,
            tonemap_program: &self.tonemap_program,
            tonemap_constants: &TonemapParams {
                tonemap_op: 0,
                hdr_output: 0,
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
            },
        })?;
        frame.publish_runtime_diagnostics("Off", 1, false, false);
        let _ = self.debug_view_picker.present_selected(frame, &swapchain)?;

        let mut overlay = DebugOverlay::new();
        overlay.rounded_rectangle_outline_screen(
            ext.width,
            ext.height,
            [16.0, 16.0],
            [620.0, 148.0],
            10.0,
            3.0,
            [1.0, 1.0, 1.0, 1.0],
        );
        overlay.add_screen_text("Shader Playground", 24.0, 22.0);
        overlay.add_screen_text(
            format!(
                "time={:.3} frame={} paused={} debug={}",
                self.playhead_time,
                self.frame_index,
                self.paused,
                self.debug_view_picker
                    .selected_name(&controller)
                    .unwrap_or_else(|| "Off".to_string())
            ),
            24.0,
            50.0,
        );
        overlay.add_screen_text(
            "keys: P pause S step [ / ] scrub M/N debug view",
            24.0,
            78.0,
        );
        overlay.add_screen_text(
            "uniforms: time, frame, paused, resolution, aspect | textures: procedural_mask | LUTs: disabled",
            24.0,
            106.0,
        );
        frame.run_camera_locked_pass("playground_overlay", &swapchain, |render_frame, target| {
            self.overlay
                .draw(render_frame, target, ext.width, ext.height, &overlay)
        })?;

        render_frame.present_image(&swapchain)?;
        self.frame_index = self.frame_index.saturating_add(1);
        Ok(())
    }

    fn resize(&mut self, _width: u32, _height: u32) -> Result<()> {
        Ok(())
    }

    fn key_pressed(&mut self, key: &str, _surface: &mut Surface) -> Result<()> {
        match key {
            "P" | "p" => {
                self.paused = !self.paused;
            }
            "S" | "s" => {
                self.pending_step_frames = self.pending_step_frames.saturating_add(1);
                self.paused = true;
            }
            "[" => {
                self.scrub_delta -= 0.25;
                self.paused = true;
            }
            "]" => {
                self.scrub_delta += 0.25;
                self.paused = true;
            }
            "N" | "n" => {
                if let Some(controller) = self.runtime_controller.as_mut() {
                    let names = controller.diagnostics().debug_images;
                    let _ = self.debug_view_picker.cycle_next(controller, &names)?;
                }
            }
            "M" | "m" => {
                if let Some(controller) = self.runtime_controller.as_mut() {
                    let names = controller.diagnostics().debug_images;
                    let _ = self.debug_view_picker.cycle_previous(controller, &names)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn shader_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(name)
}

fn main() {
    sturdy_engine::run::<ShaderPlayground>(
        WindowConfig::new("SturdyEngine Shader Playground", 1280, 720).with_resizable(true),
    );
}
