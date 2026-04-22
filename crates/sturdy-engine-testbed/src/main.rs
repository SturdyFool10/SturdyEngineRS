use std::time::Instant;

use sturdy_engine::{
    BloomConfig, BloomPass, Engine, EngineApp, Extent3d, Format, GpuProceduralTexture,
    HdrPipelineDesc, HdrPreference, Image, ImageDesc, ImageDimension, ImageUsage, Mesh,
    MeshProgram, QuadBatch, Result as EngineResult, SamplerPreset, ShaderProgram, ShellFrame,
    Surface, SurfaceImage, TextDrawDesc, TextEngine, TextPlacement, TextTypography, TextUiRenderer,
    WindowConfig, push_constants,
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

struct Testbed {
    engine: Engine,
    scene_program: ShaderProgram,
    tonemap_program: ShaderProgram,
    hud_program: MeshProgram,
    bloom_pass: BloomPass,
    bloom_config: BloomConfig,
    bloom_enabled: bool,
    bloom_only: bool,
    color_lut: GpuProceduralTexture,
    text_engine: TextEngine<TextUiRenderer>,
    hud_atlas_images: Vec<Image>,
    hud_meshes: Vec<Mesh>,
    hud_mesh_pages: Vec<u32>,
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

        Ok(Self {
            engine: engine.clone(),
            scene_program: engine.load_shader(shader_path("shader_graph_fragment.slang"))?,
            tonemap_program: engine.load_shader(shader_path("tonemap.slang"))?,
            hud_program: MeshProgram::load_2d_alpha(engine, shader_path("hud_text.slang"))?,
            bloom_pass: BloomPass::new(engine)?,
            bloom_config: BloomConfig::default(),
            bloom_enabled: true,
            bloom_only: false,
            color_lut,
            text_engine: TextEngine::new(TextUiRenderer::with_engine(engine)),
            hud_atlas_images: Vec::new(),
            hud_meshes: Vec::new(),
            hud_mesh_pages: Vec::new(),
            started_at: Instant::now(),
        })
    }

    fn render(
        &mut self,
        frame: &mut ShellFrame<'_>,
        surface_image: &SurfaceImage,
    ) -> EngineResult<()> {
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

        // GPU procedural LUT: the generator shader runs on the GPU every frame.
        // The scene shader reads "color_lut" and will be scheduled after this pass.
        self.color_lut.generate_with_constants(
            frame,
            &LutParams {
                phase: elapsed * 0.4,
            },
        )?;

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
        // When bloom is disabled, alias scene_color directly as hdr_composite.
        if self.bloom_enabled {
            let _hdr_composite = self.bloom_pass.execute(
                &scene_color,
                frame,
                &self.bloom_config,
                self.bloom_only,
            )?;
        } else {
            scene_color.register_as("hdr_composite");
        }

        self.draw_hud(frame, &swapchain, ext.width, ext.height)?;
        frame.present_image(&swapchain)?;

        // In debug builds, validate the recorded graph and print any diagnostics.
        #[cfg(debug_assertions)]
        for d in frame.validate() {
            eprintln!("[graph {:?}] {}", d.level, d.message);
        }

        Ok(())
    }

    fn key_pressed(&mut self, key: &str) {
        if key == "b" {
            self.bloom_only = !self.bloom_only;
            eprintln!("bloom-only: {}", self.bloom_only);
        } else if key == "B" {
            self.bloom_enabled = !self.bloom_enabled;
            eprintln!("bloom: {}", if self.bloom_enabled { "on" } else { "off" });
        }
    }

    fn resize(&mut self, _width: u32, _height: u32) -> EngineResult<()> {
        Ok(())
    }
}

impl Testbed {
    fn draw_hud(
        &mut self,
        frame: &sturdy_engine::RenderFrame,
        target: &sturdy_engine::GraphImage,
        width: u32,
        height: u32,
    ) -> EngineResult<()> {
        let hud_text = format!(
            "SturdyEngine testbed\nB  bloom: {}\nb  bloom-only: {}\nResize window to test graph image recreation\nClose window to exit",
            if self.bloom_enabled { "on" } else { "off" },
            if self.bloom_only { "on" } else { "off" },
        );
        let desc = TextDrawDesc::new(hud_text)
            .placement(TextPlacement::Screen2d { x: 18.0, y: 18.0 })
            .typography(
                TextTypography::default()
                    .font_size(18.0)
                    .line_height(24.0)
                    .weight(600),
            )
            .color([0.92, 0.98, 1.0, 1.0])
            .max_width(460.0);
        let text_frame = self.text_engine.prepare_frame(&[desc], width, height);
        if text_frame.draws.is_empty() || text_frame.atlas_pages.is_empty() {
            return Ok(());
        }

        self.ensure_hud_atlas_images(&text_frame.atlas_pages)?;
        self.hud_meshes.clear();
        self.hud_mesh_pages.clear();

        for page in &text_frame.atlas_pages {
            let mut batch = QuadBatch::new();
            for draw in &text_frame.draws {
                for quad in &draw.quads {
                    if quad.atlas_page != page.page_index {
                        continue;
                    }
                    let x0 = quad.positions[0][0];
                    let y0 = quad.positions[0][1];
                    let x1 = quad.positions[1][0];
                    let y2 = quad.positions[2][1];
                    let ndc_x = x0 / width.max(1) as f32 * 2.0 - 1.0;
                    let ndc_y = 1.0 - y0 / height.max(1) as f32 * 2.0;
                    let ndc_w = (x1 - x0) / width.max(1) as f32 * 2.0;
                    let ndc_h = -(y2 - y0) / height.max(1) as f32 * 2.0;
                    batch.push(
                        [ndc_x, ndc_y],
                        [ndc_w, ndc_h],
                        [
                            quad.uvs[0][0],
                            quad.uvs[2][1],
                            quad.uvs[2][0],
                            quad.uvs[0][1],
                        ],
                        quad.color,
                    );
                }
            }
            if !batch.is_empty() {
                self.hud_meshes.push(batch.build(&self.engine)?);
                self.hud_mesh_pages.push(page.page_index);
            }
        }

        for (mesh_index, page_index) in self.hud_mesh_pages.iter().copied().enumerate() {
            let Some(page) = text_frame
                .atlas_pages
                .iter()
                .find(|page| page.page_index == page_index)
            else {
                continue;
            };
            let Some(image) = self.hud_atlas_images.get(page.page_index as usize) else {
                continue;
            };
            let pixels = page.pixels.clone();
            let page_width = page.width;
            frame.update_texture_2d("text_atlas", image, move |x, y| {
                let index = ((y * page_width + x) * 4) as usize;
                [
                    pixels[index],
                    pixels[index + 1],
                    pixels[index + 2],
                    pixels[index + 3],
                ]
            })?;
            frame.set_sampler("text_atlas_sampler", SamplerPreset::Linear);
            if let Some(mesh) = self.hud_meshes.get(mesh_index) {
                target.draw_mesh(mesh, &self.hud_program)?;
            }
        }

        Ok(())
    }

    fn ensure_hud_atlas_images(
        &mut self,
        pages: &[sturdy_engine::TextAtlasPage],
    ) -> EngineResult<()> {
        for page in pages {
            let index = page.page_index as usize;
            let needs_image = self
                .hud_atlas_images
                .get(index)
                .map(|image| {
                    let desc = image.desc();
                    desc.extent.width != page.width || desc.extent.height != page.height
                })
                .unwrap_or(true);
            if !needs_image {
                continue;
            }
            while self.hud_atlas_images.len() <= index {
                self.hud_atlas_images
                    .push(self.create_hud_atlas_image(1, 1)?);
            }
            self.hud_atlas_images[index] = self.create_hud_atlas_image(page.width, page.height)?;
        }
        Ok(())
    }

    fn create_hud_atlas_image(&self, width: u32, height: u32) -> EngineResult<Image> {
        let image = self.engine.create_image(ImageDesc {
            dimension: ImageDimension::D2,
            extent: Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Rgba8Unorm,
            usage: ImageUsage::SAMPLED | ImageUsage::COPY_DST,
            transient: false,
            clear_value: None,
            debug_name: Some("hud text atlas"),
        })?;
        let _ = image.set_debug_name("hud-text-atlas");
        Ok(image)
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
