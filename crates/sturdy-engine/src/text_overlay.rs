use crate::{
    Engine, Extent3d, Format, GraphImage, Image, ImageDesc, ImageDimension, ImageUsage, Mesh,
    MeshProgram, MeshProgramDesc, MeshVertexKind, QuadBatch, RenderFrame, Result, ShaderDesc,
    ShaderSource, ShaderStage, TextAtlasContentMode, TextDrawDesc, TextEngine, TextPlacement,
    TextTypography, TextUiRenderer, TiledTextAtlasPage,
};

const TEXT_OVERLAY_ALPHA_FRAGMENT: &str = r#"
Texture2D<float4> text_atlas;
SamplerState text_atlas_sampler;

struct FSInput {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
    float4 color : COLOR0;
};

float4 main(FSInput input) : SV_TARGET {
    float4 sample = text_atlas.SampleLevel(text_atlas_sampler, input.uv, 0.0);
    return sample * input.color;
}
"#;

const TEXT_OVERLAY_SDF_FRAGMENT: &str = r#"
Texture2D<float4> text_atlas;
SamplerState text_atlas_sampler;

struct FSInput {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
    float4 color : COLOR0;
};

float4 main(FSInput input) : SV_TARGET {
    float4 sample = text_atlas.SampleLevel(text_atlas_sampler, input.uv, 0.0);
    float distance = sample.r;
    float width = max(fwidth(distance), 1.0 / 255.0);
    float alpha = smoothstep(0.5 - width, 0.5 + width, distance);
    return float4(input.color.rgb, input.color.a * alpha);
}
"#;

const TEXT_OVERLAY_MSDF_FRAGMENT: &str = r#"
Texture2D<float4> text_atlas;
SamplerState text_atlas_sampler;

struct FSInput {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
    float4 color : COLOR0;
};

float median3(float a, float b, float c) {
    return max(min(a, b), min(max(a, b), c));
}

float4 main(FSInput input) : SV_TARGET {
    float4 sample = text_atlas.SampleLevel(text_atlas_sampler, input.uv, 0.0);
    float distance = median3(sample.r, sample.g, sample.b);
    float width = max(fwidth(distance), 1.0 / 255.0);
    float alpha = smoothstep(0.5 - width, 0.5 + width, distance);
    return float4(input.color.rgb, input.color.a * alpha);
}
"#;

/// First-party text/debug overlay built on top of `textui`.
pub struct TextOverlay {
    engine: Engine,
    text_engine: TextEngine<TextUiRenderer>,
    alpha_program: MeshProgram,
    sdf_program: MeshProgram,
    msdf_program: MeshProgram,
    atlas_images: Vec<Image>,
    /// Tracks the last-uploaded content hash per atlas image slot to avoid
    /// re-uploading unchanged pages every frame.
    atlas_image_hashes: Vec<u64>,
    meshes: Vec<Mesh>,
    mesh_pages: Vec<u32>,
}

impl TextOverlay {
    pub fn new(engine: &Engine) -> Result<Self> {
        Ok(Self {
            engine: engine.clone(),
            text_engine: TextEngine::new(TextUiRenderer::with_engine(engine)),
            alpha_program: text_program(engine, TEXT_OVERLAY_ALPHA_FRAGMENT)?,
            sdf_program: text_program(engine, TEXT_OVERLAY_SDF_FRAGMENT)?,
            msdf_program: text_program(engine, TEXT_OVERLAY_MSDF_FRAGMENT)?,
            atlas_images: Vec::new(),
            atlas_image_hashes: Vec::new(),
            meshes: Vec::new(),
            mesh_pages: Vec::new(),
        })
    }

    pub fn draw(
        &mut self,
        frame: &RenderFrame,
        target: &GraphImage,
        width: u32,
        height: u32,
        descs: &[TextDrawDesc],
    ) -> Result<()> {
        let tiled_text_frame = self.text_engine.prepare_tiled_frame_with_engine_limits(
            &self.engine,
            descs,
            width,
            height,
        );
        if tiled_text_frame.draws.is_empty() || tiled_text_frame.atlas_pages.is_empty() {
            return Ok(());
        }

        self.ensure_atlas_images(&tiled_text_frame.atlas_pages)?;
        self.meshes.clear();
        self.mesh_pages.clear();

        let fw = width.max(1) as f32;
        let fh = height.max(1) as f32;
        for page in &tiled_text_frame.atlas_pages {
            let mut batch = QuadBatch::new();
            for draw in &tiled_text_frame.draws {
                let clip = descs
                    .get(draw.source_index)
                    .and_then(|d| d.clip_rect);
                for quad in &draw.quads {
                    if quad.atlas_page != page.page_index {
                        continue;
                    }
                    // Screen-space corners.
                    let mut sx0 = quad.positions[0][0];
                    let mut sy0 = quad.positions[0][1];
                    let mut sx1 = quad.positions[1][0];
                    let mut sy1 = quad.positions[2][1];
                    // UV corners (top-left and bottom-right).
                    let mut u0 = quad.uvs[0][0];
                    let mut v0 = quad.uvs[0][1];
                    let mut u1 = quad.uvs[2][0];
                    let mut v1 = quad.uvs[2][1];

                    if let Some([cx, cy, cw, ch]) = clip {
                        let cl = cx;
                        let ct = cy;
                        let cr = cx + cw;
                        let cb = cy + ch;
                        // Skip fully-outside quads.
                        if sx1 <= cl || sx0 >= cr || sy1 <= ct || sy0 >= cb {
                            continue;
                        }
                        let qw = (sx1 - sx0).max(f32::EPSILON);
                        let qh = (sy1 - sy0).max(f32::EPSILON);
                        // Clip left.
                        if sx0 < cl {
                            let t = (cl - sx0) / qw;
                            u0 += t * (u1 - u0);
                            sx0 = cl;
                        }
                        // Clip right.
                        if sx1 > cr {
                            let t = (sx1 - cr) / qw;
                            u1 -= t * (u1 - u0);
                            sx1 = cr;
                        }
                        // Clip top.
                        if sy0 < ct {
                            let t = (ct - sy0) / qh;
                            v0 += t * (v1 - v0);
                            sy0 = ct;
                        }
                        // Clip bottom.
                        if sy1 > cb {
                            let t = (sy1 - cb) / qh;
                            v1 -= t * (v1 - v0);
                            sy1 = cb;
                        }
                    }

                    let ndc_x = sx0 / fw * 2.0 - 1.0;
                    let ndc_y = 1.0 - sy0 / fh * 2.0;
                    let ndc_w = (sx1 - sx0) / fw * 2.0;
                    let ndc_h = -((sy1 - sy0) / fh * 2.0);
                    batch.push(
                        [ndc_x, ndc_y],
                        [ndc_w, ndc_h],
                        [u0, v1, u1, v0],
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
            let cached_hash = self
                .atlas_image_hashes
                .get(page.page_index as usize)
                .copied()
                .unwrap_or(!0);
            if cached_hash != page.content_hash {
                frame.update_texture_2d_pixels("text_atlas", image, &page.pixels)?;
                while self.atlas_image_hashes.len() <= page.page_index as usize {
                    self.atlas_image_hashes.push(!0);
                }
                self.atlas_image_hashes[page.page_index as usize] = page.content_hash;
            } else {
                // Content unchanged: register the image name without re-uploading.
                frame.import_image("text_atlas", image)?;
            }
            frame.set_sampler("text_atlas_sampler", crate::SamplerPreset::Linear);
            if let Some(mesh) = self.meshes.get(mesh_index) {
                target.draw_mesh(mesh, self.program_for_content_mode(page.content_mode))?;
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
                self.atlas_image_hashes.push(!0);
            }
            self.atlas_images[index] = self.create_atlas_image(page.size_px[0], page.size_px[1])?;
            self.atlas_image_hashes[index] = !0;
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

    fn program_for_content_mode(&self, mode: TextAtlasContentMode) -> &MeshProgram {
        match mode {
            TextAtlasContentMode::AlphaMask => &self.alpha_program,
            TextAtlasContentMode::Sdf => &self.sdf_program,
            TextAtlasContentMode::Msdf => &self.msdf_program,
        }
    }
}

fn text_program(engine: &Engine, fragment: &str) -> Result<MeshProgram> {
    MeshProgram::new(
        engine,
        MeshProgramDesc {
            fragment: ShaderDesc {
                source: ShaderSource::Inline(fragment.to_string()),
                entry_point: "main".to_string(),
                stage: ShaderStage::Fragment,
            },
            vertex: None,
            vertex_kind: MeshVertexKind::V2d,
            alpha_blend: true,
        },
    )
}
