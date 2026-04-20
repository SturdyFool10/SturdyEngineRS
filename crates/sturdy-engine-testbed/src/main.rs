use std::time::Instant;

use sturdy_engine::{
    BloomConfig, BloomPass, Engine, EngineApp, HdrPipelineDesc, HdrPreference, Result as EngineResult,
    ShaderProgram, ShellFrame, Surface, SurfaceImage, WindowConfig, push_constants,
};

#[push_constants]
struct FrameConstants {
    time: f32,
    aspect: f32,
    resolution: [f32; 2],
}

struct Testbed {
    scene_program: ShaderProgram,
    tonemap_program: ShaderProgram,
    bloom_pass: BloomPass,
    bloom_config: BloomConfig,
    bloom_only: bool,
    started_at: Instant,
}

impl EngineApp for Testbed {
    type Error = sturdy_engine::Error;

    fn init(engine: &Engine, surface: &Surface) -> EngineResult<Self> {
        let surface_info = surface.info();
        let hdr_caps = surface.hdr_caps()?;
        let hdr_desc =
            HdrPipelineDesc::select(&hdr_caps, &engine.caps(), HdrPreference::PreferHdr)?;

        println!(
            "rendering on {:?} using {:?}; surface {:?} at {}x{}",
            engine.adapter_name(),
            engine.backend_kind(),
            surface_info.format,
            surface_info.size.width,
            surface_info.size.height,
        );
        println!(
            "HDR mode: {:?}, tone mapping: {:?}",
            hdr_desc.mode, hdr_desc.tone_mapping,
        );

        Ok(Self {
            scene_program: engine.load_shader(shader_path("shader_graph_fragment.slang"))?,
            tonemap_program: engine.load_shader(shader_path("tonemap.slang"))?,
            bloom_pass: BloomPass::new(engine)?,
            bloom_config: BloomConfig::default(),
            bloom_only: false,
            started_at: Instant::now(),
        })
    }

    fn render(&mut self, frame: &mut ShellFrame<'_>, surface_image: &SurfaceImage) -> EngineResult<()> {
        let frame = frame.inner();
        let elapsed = self.started_at.elapsed().as_secs_f32();
        let ext = surface_image.desc().extent;

        // Register swapchain first — required so hdr_color_image can read the extent.
        let swapchain = frame.swapchain_image(surface_image)?;
        // Register the FP16 scene buffer sized to the swapchain.
        let scene_color = frame.hdr_color_image("scene_color")?;

        // Passes are declared out of dependency order to exercise the deferred
        // bind-group system: tonemap is declared before bloom has registered
        // "hdr_composite". The scheduler resolves reads at flush time and
        // re-orders passes into the correct RAW execution sequence automatically.

        // Pass 3 declared first: tonemap reads "hdr_composite", writes swapchain.
        // "hdr_composite" does not exist yet — registered by bloom below.
        swapchain.execute_shader(&self.tonemap_program)?;

        // Pass 1: scene writes "scene_color".
        scene_color.execute_shader_with_constants_auto(
            &self.scene_program,
            &FrameConstants {
                time: elapsed,
                aspect: ext.width as f32 / ext.height.max(1) as f32,
                resolution: [ext.width as f32, ext.height as f32],
            },
        )?;

        // Pass 2: bloom reads "scene_color", writes "hdr_composite".
        let _hdr_composite = self.bloom_pass.execute(&scene_color, frame, &self.bloom_config, self.bloom_only)?;
        frame.present_image(&swapchain)?;

        // In debug builds, validate the recorded graph and print any diagnostics.
        #[cfg(debug_assertions)]
        for d in frame.validate() {
            eprintln!("[graph {:?}] {}", d.level, d.message);
        }

        Ok(())
    }

    fn key_pressed(&mut self, key: &str) {
        if key.eq_ignore_ascii_case("b") {
            self.bloom_only = !self.bloom_only;
            eprintln!("bloom-only: {}", self.bloom_only);
        }
    }

    fn resize(&mut self, _width: u32, _height: u32) -> EngineResult<()> {
        // Surface resize is handled by the shell; graph images resize automatically
        // via cache stale-eviction on the next frame.
        Ok(())
    }
}

fn shader_path(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(name)
}

fn main() {
    sturdy_engine::run::<Testbed>(
        WindowConfig::new("SturdyEngine HDR bloom testbed", 1280, 720).with_resizable(true),
    );
}
