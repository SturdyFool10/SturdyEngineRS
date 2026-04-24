use crate::{
    Engine, Extent3d, Format, GraphImage, Image, ImageDesc, ImageDimension, ImageUsage, Mesh,
    MeshProgram, MeshProgramDesc, MeshVertexKind, QuadBatch, RenderFrame, Result, ShaderDesc,
    ShaderSource, ShaderStage, TextDrawDesc, TextEngine, TextPlacement, TextTypography,
    TextUiRenderer, TiledTextAtlasPage,
};

const TEXT_OVERLAY_FRAGMENT: &str = r#"
Texture2D<float4> text_atlas;
SamplerState text_atlas_sampler;

struct FSInput {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
    float4 color : COLOR0;
};

float4 main(FSInput input) : SV_TARGET {
    float4 sample = text_atlas.SampleLevel(text_atlas_sampler, input.uv, 0.0);
    return float4(input.color.rgb, input.color.a * sample.a);
}
"#;

/// First-party text/debug overlay built on top of `textui`.
pub struct TextOverlay {
    engine: Engine,
    text_engine: TextEngine<TextUiRenderer>,
    program: MeshProgram,
    atlas_images: Vec<Image>,
    meshes: Vec<Mesh>,
    mesh_pages: Vec<u32>,
}

impl TextOverlay {
    pub fn new(engine: &Engine) -> Result<Self> {
        Ok(Self {
            engine: engine.clone(),
            text_engine: TextEngine::new(TextUiRenderer::with_engine(engine)),
            program: MeshProgram::new(
                engine,
                MeshProgramDesc {
                    fragment: ShaderDesc {
                        source: ShaderSource::Inline(TEXT_OVERLAY_FRAGMENT.to_string()),
                        entry_point: "main".to_string(),
                        stage: ShaderStage::Fragment,
                    },
                    vertex: None,
                    vertex_kind: MeshVertexKind::V2d,
                    alpha_blend: true,
                },
            )?,
            atlas_images: Vec::new(),
            meshes: Vec::new(),
            mesh_pages: Vec::new(),
        })
    }

    pub fn draw(&mut self, frame: &RenderFrame, target: &GraphImage, width: u32, height: u32, descs: &[TextDrawDesc]) -> Result<()> {
        let tiled_text_frame = self
            .text_engine
            .prepare_tiled_frame_with_engine_limits(&self.engine, descs, width, height);
        if tiled_text_frame.draws.is_empty() || tiled_text_frame.atlas_pages.is_empty() {
            return Ok(());
        }

        self.ensure_atlas_images(&tiled_text_frame.atlas_pages)?;
        self.meshes.clear();
        self.mesh_pages.clear();

        for page in &tiled_text_frame.atlas_pages {
            let mut batch = QuadBatch::new();
            for draw in &tiled_text_frame.draws {
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
                self.meshes.push(batch.build(&self.engine)?);
                self.mesh_pages.push(page.page_index);
            }
        }

        for (mesh_index, page_index) in self.mesh_pages.iter().copied().enumerate() {
            let Some(page) = tiled_text_frame
                .atlas_pages
                .iter()
                .find(|page| page.page_index == page_index)
            else {
                continue;
            };
            let Some(image) = self.atlas_images.get(page.page_index as usize) else {
                continue;
            };
            let pixels = page.pixels.clone();
            let page_width = page.size_px[0];
            frame.update_texture_2d("text_atlas", image, move |x, y| {
                let index = ((y * page_width + x) * 4) as usize;
                [
                    pixels[index],
                    pixels[index + 1],
                    pixels[index + 2],
                    pixels[index + 3],
                ]
            })?;
            frame.set_sampler("text_atlas_sampler", crate::SamplerPreset::Linear);
            if let Some(mesh) = self.meshes.get(mesh_index) {
                target.draw_mesh(mesh, &self.program)?;
            }
        }

        Ok(())
    }

    pub fn draw_screen_text(
        &mut self,
        frame: &RenderFrame,
        target: &GraphImage,
        width: u32,
        height: u32,
        text: impl Into<String>,
        x: f32,
        y: f32,
    ) -> Result<()> {
        let desc = TextDrawDesc::new(text.into())
            .placement(TextPlacement::Screen2d { x, y })
            .typography(
                TextTypography::default()
                    .font_size(18.0)
                    .line_height(24.0)
                    .weight(600),
            )
            .color([0.92, 0.98, 1.0, 1.0])
            .max_width(460.0);
        self.draw(frame, target, width, height, &[desc])
    }

    fn ensure_atlas_images(&mut self, pages: &[TiledTextAtlasPage]) -> Result<()> {
        for page in pages {
            let index = page.page_index as usize;
            let needs_image = self
                .atlas_images
                .get(index)
                .map(|image| {
                    let desc = image.desc();
                    desc.extent.width != page.size_px[0] || desc.extent.height != page.size_px[1]
                })
                .unwrap_or(true);
            if !needs_image {
                continue;
            }
            while self.atlas_images.len() <= index {
                self.atlas_images.push(self.create_atlas_image(1, 1)?);
            }
            self.atlas_images[index] = self.create_atlas_image(page.size_px[0], page.size_px[1])?;
        }
        Ok(())
    }

    fn create_atlas_image(&self, width: u32, height: u32) -> Result<Image> {
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
            debug_name: Some("text overlay atlas"),
        })?;
        let _ = image.set_debug_name("text-overlay-atlas");
        Ok(image)
    }
}
