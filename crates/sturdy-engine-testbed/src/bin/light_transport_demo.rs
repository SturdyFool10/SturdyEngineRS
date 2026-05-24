use std::{path::PathBuf, time::Instant};

use sturdy_engine::{
    AntiAliasingMode, AntiAliasingPass, AppRuntime, AppRuntimeFrame, DebugOverlay,
    DebugOverlayRenderer, HdrPipelineDesc, HdrPreference, Result, RuntimeApp,
    RuntimePostProcessDesc, ShaderProgram, WindowConfig, push_constants, run_with_runtime,
};

#[push_constants]
struct LightTransportConstants {
    time: f32,
    frame: u32,
    mode: u32,
    _pad0: u32,
    resolution: [f32; 2],
    aspect: f32,
    exposure: f32,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LightDemoViewMode {
    Beauty,
    CausticOnly,
    Normals,
}

impl LightDemoViewMode {
    fn as_u32(self) -> u32 {
        match self {
            Self::Beauty => 0,
            Self::CausticOnly => 1,
            Self::Normals => 2,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Beauty => "beauty",
            Self::CausticOnly => "caustic estimate",
            Self::Normals => "normals",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Beauty => Self::CausticOnly,
            Self::CausticOnly => Self::Normals,
            Self::Normals => Self::Beauty,
        }
    }
}

struct LightTransportDemo {
    scene_program: ShaderProgram,
    tonemap_program: ShaderProgram,
    aa_pass: AntiAliasingPass,
    overlay: DebugOverlayRenderer,
    view_mode: LightDemoViewMode,
    playhead_time: f32,
    paused: bool,
    pending_step_frames: u32,
    exposure: f32,
    frame_index: u32,
    last_frame_started: Instant,
}

impl RuntimeApp for LightTransportDemo {
    type Error = sturdy_engine::Error;

    fn init(runtime: &mut AppRuntime) -> Result<Self> {
        let engine = runtime.engine();
        let hdr_caps = runtime.surface().hdr_caps()?;
        let _hdr_desc =
            HdrPipelineDesc::select(&hdr_caps, &engine.caps(), HdrPreference::PreferHdr)?;

        Ok(Self {
            scene_program: engine.load_shader(shader_path("light_transport_fragment.slang"))?,
            tonemap_program: engine.load_shader(shader_path("tonemap.slang"))?,
            aa_pass: AntiAliasingPass::new(engine)?,
            overlay: DebugOverlayRenderer::new(engine)?,
            view_mode: LightDemoViewMode::Beauty,
            playhead_time: 0.0,
            paused: false,
            pending_step_frames: 0,
            exposure: 0.92,
            frame_index: 0,
            last_frame_started: Instant::now(),
        })
    }

    fn update(&mut self, appframe: &mut AppRuntimeFrame<'_>) -> Result<()> {
        let shell_frame = appframe.shell_frame();
        let surface_image = appframe.surface_image();

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

        let ext = surface_image.desc().extent;
        let swapchain = shell_frame.inner().swapchain_image(surface_image)?;
        let scene_target = shell_frame.default_hdr_scene_target("light_transport_scene", 1)?;
        let scene_color =
            shell_frame.resolve_default_hdr_scene_target(&scene_target, "light_transport_scene")?;
        let render_frame = shell_frame.inner();

        scene_target.execute_shader_with_constants_auto(
            &self.scene_program,
            &LightTransportConstants {
                time: self.playhead_time,
                frame: self.frame_index,
                mode: self.view_mode.as_u32(),
                _pad0: 0,
                resolution: [ext.width as f32, ext.height as f32],
                aspect: ext.width as f32 / ext.height.max(1) as f32,
                exposure: self.exposure,
            },
        )?;

        let _ = shell_frame.run_default_post_process(RuntimePostProcessDesc {
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
        shell_frame.publish_runtime_diagnostics("Off", 1, false, false);

        let mut overlay = DebugOverlay::new();
        overlay.rounded_rectangle_outline_screen(
            ext.width,
            ext.height,
            [16.0, 16.0],
            [690.0, 148.0],
            10.0,
            3.0,
            [1.0, 1.0, 1.0, 1.0],
        );
        overlay.add_screen_text("Clean-room Light Transport Demo", 24.0, 22.0);
        overlay.add_screen_text(
            "inspired by e10b/light ideas: analytic glass, spectral split, photon-style caustic estimate",
            24.0,
            50.0,
        );
        overlay.add_screen_text(
            format!(
                "time={:.2} frame={} paused={} mode={} exposure={:.2}",
                self.playhead_time,
                self.frame_index,
                self.paused,
                self.view_mode.label(),
                self.exposure,
            ),
            24.0,
            78.0,
        );
        overlay.add_screen_text(
            "keys: P pause | S step | V view mode | [/] exposure",
            24.0,
            106.0,
        );
        shell_frame.run_camera_locked_pass(
            "light_transport_overlay",
            &swapchain,
            |render_frame, target| {
                self.overlay
                    .draw(render_frame, target, ext.width, ext.height, &overlay)
            },
        )?;

        render_frame.present_image(&swapchain)?;
        self.frame_index = self.frame_index.saturating_add(1);
        Ok(())
    }

    fn key_pressed(&mut self, key: &str) -> Result<()> {
        match key {
            "P" | "p" => {
                self.paused = !self.paused;
            }
            "S" | "s" => {
                self.pending_step_frames = self.pending_step_frames.saturating_add(1);
                self.paused = true;
            }
            "V" | "v" => {
                self.view_mode = self.view_mode.next();
            }
            "[" => {
                self.exposure = (self.exposure * 0.9).max(0.1);
            }
            "]" => {
                self.exposure = (self.exposure * 1.1).min(4.0);
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
    run_with_runtime::<LightTransportDemo>(
        WindowConfig::new("SturdyEngine Light Transport Demo", 1280, 720).with_resizable(true),
    );
}
