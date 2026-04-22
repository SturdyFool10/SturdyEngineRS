use std::time::Instant;

use sturdy_engine::{
    BloomConfig, BloomPass, CpuProceduralTexture2d, Engine, EngineApp, Extent3d, Format,
    GpuProceduralTexture, HdrPipelineDesc, HdrPreference, Image, ImageDesc, ImageDimension,
    ImageUsage, Mesh, MeshProgram, ProceduralTextureRecipe, ProceduralTextureUpdatePolicy,
    QuadBatch, Result as EngineResult, SamplerPreset, ShaderProgram, ShellFrame, Surface,
    SurfaceColorSpace, SurfaceHdrCaps, SurfaceHdrPreference, SurfaceImage, SurfaceRecreateDesc,
    TextDrawDesc, TextEngine, TextPlacement, TextTypography, TextUiRenderer, ToneMappingOp,
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

    fn adjust(&mut self, tone_mapping: ToneMappingOp, dial: TonemapDial, direction: f32) {
        let step = dial.step();
        let value = self.get(dial) + step * direction;
        self.set(dial, value.clamp(dial.min(), dial.max()));
        self.sync_operator_white_point(tone_mapping, dial);
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
    tonemap_program: ShaderProgram,
    hud_program: MeshProgram,
    bloom_pass: BloomPass,
    bloom_config: BloomConfig,
    bloom_enabled: bool,
    bloom_only: bool,
    hdr_caps: SurfaceHdrCaps,
    hdr_output: bool,
    tone_mapping: ToneMappingOp,
    tonemap_settings: TonemapSettings,
    selected_tonemap_dial: TonemapDial,
    color_lut: GpuProceduralTexture,
    procedural_mask: CpuProceduralTexture2d,
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

        Ok(Self {
            engine: engine.clone(),
            scene_program: engine.load_shader(shader_path("shader_graph_fragment.slang"))?,
            tonemap_program: engine.load_shader(shader_path("tonemap.slang"))?,
            hud_program: MeshProgram::load_2d_alpha(engine, shader_path("hud_text.slang"))?,
            bloom_pass: BloomPass::new(engine)?,
            bloom_config: BloomConfig::default(),
            bloom_enabled: true,
            bloom_only: false,
            hdr_caps,
            hdr_output,
            tone_mapping: if hdr_output && hdr_desc.tone_mapping == ToneMappingOp::Linear {
                ToneMappingOp::Aces
            } else {
                hdr_desc.tone_mapping
            },
            tonemap_settings: TonemapSettings::default(),
            selected_tonemap_dial: TonemapDial::Exposure,
            color_lut,
            procedural_mask,
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
        self.procedural_mask.prepare(frame)?;

        // Pass 3 declared first: tonemap reads "hdr_composite", writes swapchain.
        // "hdr_composite" does not exist yet — registered by bloom below.
        swapchain.execute_shader_with_constants_auto(
            &self.tonemap_program,
            &self.tonemap_settings.params(
                self.tone_mapping,
                self.hdr_output,
                self.selected_tonemap_dial,
            ),
        )?;

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

    fn key_pressed(&mut self, key: &str, surface: &mut Surface) -> EngineResult<()> {
        if key == "b" {
            self.bloom_only = !self.bloom_only;
            eprintln!("bloom-only: {}", self.bloom_only);
        } else if key == "B" {
            self.bloom_enabled = !self.bloom_enabled;
            eprintln!("bloom: {}", if self.bloom_enabled { "on" } else { "off" });
        } else if key == "T" || key == "t" {
            self.tone_mapping = next_tone_mapping(self.tone_mapping);
            eprintln!("tone mapping: {}", tone_mapping_label(self.tone_mapping));
        } else if key == "H" || key == "h" {
            self.toggle_hdr_output(surface)?;
        } else if key == "P" || key == "p" {
            self.selected_tonemap_dial = self.selected_tonemap_dial.next();
            eprintln!(
                "tonemap dial: {} = {:.3}",
                self.selected_tonemap_dial.label(),
                self.tonemap_settings.get(self.selected_tonemap_dial),
            );
        } else if key == "]" || key == "=" || key == "+" {
            self.adjust_tonemap_dial(1.0);
        } else if key == "[" || key == "-" || key == "_" {
            self.adjust_tonemap_dial(-1.0);
        } else if key == "R" || key == "r" {
            self.tonemap_settings.reset_for(self.tone_mapping);
            eprintln!(
                "reset {} tonemap dials",
                tone_mapping_label(self.tone_mapping)
            );
        }
        Ok(())
    }

    fn resize(&mut self, _width: u32, _height: u32) -> EngineResult<()> {
        Ok(())
    }
}

impl Testbed {
    fn adjust_tonemap_dial(&mut self, direction: f32) {
        self.tonemap_settings
            .adjust(self.tone_mapping, self.selected_tonemap_dial, direction);
        eprintln!(
            "{} {}: {:.3}",
            tone_mapping_label(self.tone_mapping),
            self.selected_tonemap_dial.label(),
            self.tonemap_settings.get(self.selected_tonemap_dial),
        );
    }

    fn toggle_hdr_output(&mut self, surface: &mut Surface) -> EngineResult<()> {
        let target = if self.hdr_output {
            SurfaceHdrPreference::Sdr
        } else if self.hdr_caps.sc_rgb {
            SurfaceHdrPreference::ScRgb
        } else if self.hdr_caps.hdr10 {
            SurfaceHdrPreference::Hdr10
        } else {
            eprintln!("HDR output unavailable on this surface");
            return Ok(());
        };

        surface.recreate(SurfaceRecreateDesc {
            size: Some(surface.size()),
            hdr: Some(target),
            ..SurfaceRecreateDesc::default()
        })?;

        self.hdr_output = surface_is_hdr(surface.info().color_space);
        if !self.hdr_output && self.tone_mapping == ToneMappingOp::Linear {
            self.tone_mapping = ToneMappingOp::Aces;
        }

        eprintln!(
            "HDR output: {} ({:?}, {:?}); tone mapping: {}",
            if self.hdr_output { "on" } else { "off" },
            surface.info().format,
            surface.info().color_space,
            tone_mapping_label(self.tone_mapping),
        );
        Ok(())
    }

    fn draw_hud(
        &mut self,
        frame: &sturdy_engine::RenderFrame,
        target: &sturdy_engine::GraphImage,
        width: u32,
        height: u32,
    ) -> EngineResult<()> {
        let hud_text = format!(
            "SturdyEngine testbed\nT  tone mapping: {}\nP  dial: {} = {:.2}\n[/] adjust dial, R reset curve\nH  HDR output: {}\nB  bloom: {}\nb  bloom-only: {}\nResize window to test graph image recreation\nClose window to exit",
            tone_mapping_label(self.tone_mapping),
            self.selected_tonemap_dial.label(),
            self.tonemap_settings.get(self.selected_tonemap_dial),
            if self.hdr_output { "on" } else { "off" },
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
