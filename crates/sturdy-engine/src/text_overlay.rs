use std::path::PathBuf;

use crate::{
    BlendMode, Engine, Extent3d, Format, GraphImage, Image, ImageDesc, ImageDimension, ImageUsage,
    Mesh, MeshProgram, MeshProgramDesc, MeshVertexKind, QuadBatch, RenderFrame, Result, ShaderDesc,
    ShaderProgram, ShaderSource, ShaderStage, TextAtlasContentMode, TextDrawDesc, TextEngine,
    TextPlacement, TextTypography, TextUiRenderer, TiledTextAtlasPage,
};

fn shader_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(name)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TextOverlayDrawMode {
    Color,
    Negative,
    Mask,
}

/// First-party text/debug overlay built on top of `textui`.
pub struct TextOverlay {
    engine: Engine,
    text_engine: TextEngine<TextUiRenderer>,
    alpha_program: MeshProgram,
    sdf_program: MeshProgram,
    msdf_program: MeshProgram,
    negative_alpha_program: MeshProgram,
    negative_sdf_program: MeshProgram,
    negative_msdf_program: MeshProgram,
    mask_alpha_program: MeshProgram,
    mask_sdf_program: MeshProgram,
    mask_msdf_program: MeshProgram,
    mask_clear_program: ShaderProgram,
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
            alpha_program: text_program(engine, "text_overlay_alpha_fragment.slang")?,
            sdf_program: text_program(engine, "text_overlay_sdf_fragment.slang")?,
            msdf_program: text_program(engine, "text_overlay_msdf_fragment.slang")?,
            negative_alpha_program: text_program_with_blend(
                engine,
                "text_overlay_negative_alpha_fragment.slang",
                BlendMode::Negative,
            )?,
            negative_sdf_program: text_program_with_blend(
                engine,
                "text_overlay_negative_sdf_fragment.slang",
                BlendMode::Negative,
            )?,
            negative_msdf_program: text_program_with_blend(
                engine,
                "text_overlay_negative_msdf_fragment.slang",
                BlendMode::Negative,
            )?,
            mask_alpha_program: text_program(engine, "text_overlay_mask_alpha_fragment.slang")?,
            mask_sdf_program: text_program(engine, "text_overlay_mask_sdf_fragment.slang")?,
            mask_msdf_program: text_program(engine, "text_overlay_mask_msdf_fragment.slang")?,
            mask_clear_program: ShaderProgram::load_fragment(
                engine,
                shader_path("text_mask_clear_fragment.slang"),
            )?,
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
        self.draw_inner(
            frame,
            target,
            width,
            height,
            descs,
            TextOverlayDrawMode::Color,
        )
    }

    pub fn draw_negative(
        &mut self,
        frame: &RenderFrame,
        target: &GraphImage,
        width: u32,
        height: u32,
        descs: &[TextDrawDesc],
    ) -> Result<()> {
        self.draw_inner(
            frame,
            target,
            width,
            height,
            descs,
            TextOverlayDrawMode::Negative,
        )
    }

    /// Draw text as a white coverage mask into an existing target.
    ///
    /// The glyph coverage is preserved in the output alpha channel, including
    /// alpha-mask and SDF/MSDF antialiasing. The target should be cleared to
    /// transparent before the first mask draw unless you intentionally want to
    /// accumulate multiple masks.
    pub fn draw_mask(
        &mut self,
        frame: &RenderFrame,
        target: &GraphImage,
        width: u32,
        height: u32,
        descs: &[TextDrawDesc],
    ) -> Result<()> {
        self.draw_inner(
            frame,
            target,
            width,
            height,
            descs,
            TextOverlayDrawMode::Mask,
        )
    }

    /// Create an off-screen text mask image, clear it to transparent, and draw
    /// `descs` into it as antialiased white coverage.
    ///
    /// The returned image has `SAMPLED | RENDER_TARGET` usage so it can be
    /// registered under a shader binding name (for example `text_mask`) and
    /// sampled by a later shader pass.
    pub fn draw_mask_image(
        &mut self,
        frame: &RenderFrame,
        name: impl Into<String>,
        width: u32,
        height: u32,
        descs: &[TextDrawDesc],
    ) -> Result<GraphImage> {
        let width = width.max(1);
        let height = height.max(1);
        let target = frame.image(name, text_mask_image_desc(width, height))?;
        target.execute_shader(&self.mask_clear_program)?;
        self.draw_mask(frame, &target, width, height, descs)?;
        Ok(target)
    }

    fn draw_inner(
        &mut self,
        frame: &RenderFrame,
        target: &GraphImage,
        width: u32,
        height: u32,
        descs: &[TextDrawDesc],
        draw_mode: TextOverlayDrawMode,
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
                let clip = descs.get(draw.source_index).and_then(|d| d.clip_rect);
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
                    let ndc_y = sy0 / fh * 2.0 - 1.0;
                    let ndc_w = (sx1 - sx0) / fw * 2.0;
                    let ndc_h = (sy1 - sy0) / fh * 2.0;
                    batch.push([ndc_x, ndc_y], [ndc_w, ndc_h], [u0, v0, u1, v1], quad.color);
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
                target.draw_mesh(
                    mesh,
                    self.program_for_content_mode(page.content_mode, draw_mode),
                )?;
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
            compression: Default::default(),
            min_lod_bits: None,
            msaa_resolve_to_single_sampled: false,
            drm_format_modifier: None,
        })?;
        let _ = image.set_debug_name("text-overlay-atlas");
        Ok(image)
    }

    fn program_for_content_mode(
        &self,
        mode: TextAtlasContentMode,
        draw_mode: TextOverlayDrawMode,
    ) -> &MeshProgram {
        match (mode, draw_mode) {
            (TextAtlasContentMode::AlphaMask, TextOverlayDrawMode::Color) => &self.alpha_program,
            (TextAtlasContentMode::Sdf, TextOverlayDrawMode::Color) => &self.sdf_program,
            (TextAtlasContentMode::Msdf, TextOverlayDrawMode::Color) => &self.msdf_program,
            (TextAtlasContentMode::AlphaMask, TextOverlayDrawMode::Negative) => {
                &self.negative_alpha_program
            }
            (TextAtlasContentMode::Sdf, TextOverlayDrawMode::Negative) => {
                &self.negative_sdf_program
            }
            (TextAtlasContentMode::Msdf, TextOverlayDrawMode::Negative) => {
                &self.negative_msdf_program
            }
            (TextAtlasContentMode::AlphaMask, TextOverlayDrawMode::Mask) => {
                &self.mask_alpha_program
            }
            (TextAtlasContentMode::Sdf, TextOverlayDrawMode::Mask) => &self.mask_sdf_program,
            (TextAtlasContentMode::Msdf, TextOverlayDrawMode::Mask) => &self.mask_msdf_program,
        }
    }
}

fn text_mask_image_desc(width: u32, height: u32) -> ImageDesc {
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
        format: Format::Rgba8Unorm,
        usage: ImageUsage::SAMPLED | ImageUsage::RENDER_TARGET,
        transient: false,
        clear_value: None,
        debug_name: Some("text-mask"),
        compression: Default::default(),
        min_lod_bits: None,
        msaa_resolve_to_single_sampled: false,
        drm_format_modifier: None,
    }
}

fn text_program(engine: &Engine, fragment_file: &str) -> Result<MeshProgram> {
    text_program_with_blend(engine, fragment_file, BlendMode::Alpha)
}

fn text_program_with_blend(
    engine: &Engine,
    fragment_file: &str,
    blend_mode: BlendMode,
) -> Result<MeshProgram> {
    MeshProgram::new_with_blend_mode(
        engine,
        MeshProgramDesc {
            fragment: ShaderDesc {
                source: ShaderSource::File(shader_path(fragment_file)),
                entry_point: "main".to_string(),
                stage: ShaderStage::Fragment,
                requires_ray_query: false,
                requires_cooperative_matrix: false,
                uses_ser: false,
            },
            vertex: None,
            vertex_kind: MeshVertexKind::V2d,
            alpha_blend: true,
            uses_depth: false,
        },
        blend_mode,
    )
}
