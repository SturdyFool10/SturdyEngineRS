//! Configurable bloom post-processing pass.
//!
//! Pipeline:
//! 1. Bright-pass filter to extract luminance above threshold
//! 2. CoD:AW 13-tap downsampling mip-chain
//! 3. Tent-filtered pyramid collapse (upsample + accumulate)
//! 4. HDR composite: scene_color + bloom * intensity → FP16 output
//!
//! The pass deliberately does **not** tonemap. It outputs a linear-HDR
//! `GraphImage` named `"hdr_composite"` so that other effects (fog, lens
//! flares, colour grading) can be inserted in the compositing chain before
//! the caller's tonemap pass converts to display-referred output.
//!
//! # Frame image naming contract
//!
//! The caller **must** register a frame image named `"scene_color"` before
//! calling [`BloomPass::execute`]. The returned image is registered in the
//! frame as `"hdr_composite"`.

use crate::{
    Engine, Format, GraphImage, ImageDesc, MipPyramid, RenderFrame, Result, ShaderProgram,
    StageMask, push_constants,
};

/// Configuration for the bloom post-processing pass.
#[derive(Clone, Debug)]
pub struct BloomConfig {
    /// Luminance threshold for the bright-pass filter.
    pub threshold: f32,

    /// Soft-knee width for the bright-pass filter's transition region.
    pub knee: f32,

    /// Overall intensity multiplier for the bloom contribution.
    pub intensity: f32,

    /// Number of mip levels for the bloom downsampling chain.
    /// 0 means auto (based on source size).
    pub mip_count: u32,
}

impl Default for BloomConfig {
    fn default() -> Self {
        Self {
            threshold: 0.6,
            knee: 0.1,
            intensity: 0.4,
            mip_count: 0,
        }
    }
}

/// A bloom post-processing pass.
///
/// Create via [`BloomPass::new`] or [`BloomPass::from_programs`].
/// Drive the pass each frame with [`BloomPass::execute`].
pub struct BloomPass {
    pub bright_extract_program: ShaderProgram,
    pub downsample_program: ShaderProgram,
    pub upsample_program: ShaderProgram,
    pub composite_program: ShaderProgram,
}

impl BloomPass {
    /// Create a `BloomPass` with the engine's built-in bloom shaders.
    pub fn new(engine: &Engine) -> Result<Self> {
        let bright_extract = ShaderProgram::from_inline_fragment(
            engine,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/shaders/bloom_bright_extract.slang"
            )),
        )?;
        let downsample = ShaderProgram::from_inline_fragment(
            engine,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/shaders/bloom_downsample.slang"
            )),
        )?;
        let upsample = ShaderProgram::from_inline_fragment(
            engine,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/shaders/bloom_upsample.slang"
            )),
        )?;
        let composite = ShaderProgram::from_inline_fragment(
            engine,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/shaders/bloom_composite.slang"
            )),
        )?;

        Ok(Self {
            bright_extract_program: bright_extract,
            downsample_program: downsample,
            upsample_program: upsample,
            composite_program: composite,
        })
    }

    /// Create from pre-loaded programs.
    pub fn from_programs(
        bright_extract: ShaderProgram,
        downsample: ShaderProgram,
        upsample: ShaderProgram,
        composite: ShaderProgram,
    ) -> Self {
        Self {
            bright_extract_program: bright_extract,
            downsample_program: downsample,
            upsample_program: upsample,
            composite_program: composite,
        }
    }

    fn bloom_mip_pyramid(
        frame: &RenderFrame,
        width: u32,
        height: u32,
        mip_count: u32,
        format: Format,
    ) -> Result<MipPyramid> {
        MipPyramid::new(frame, "bloom", width, height, mip_count, format)
    }

    /// Run the bloom pipeline and return the HDR composite image.
    ///
    /// 1. Bright-pass extraction → `bloom_mip_0`
    /// 2. CoD:AW 13-tap downsampling chain → `bloom_mip_N`
    /// 3. Tent-filtered upsample/accumulate chain → `bloom_up_0`
    /// 4. HDR composite: `scene_color + bloom_up_0 * intensity` → `"hdr_composite"`
    ///
    /// The returned [`GraphImage`] is registered in the frame as `"hdr_composite"`.
    /// Pass it to a tonemap shader to produce display output.
    ///
    /// **Pre-condition**: the caller must have registered a frame image named
    /// `"scene_color"` before calling this method.
    pub fn execute(
        &self,
        scene_color: &GraphImage,
        frame: &RenderFrame,
        config: &BloomConfig,
        bloom_only: bool,
    ) -> Result<GraphImage> {
        let src_desc = scene_color.desc();
        let src_width = src_desc.extent.width;
        let src_height = src_desc.extent.height;

        let mip_count = if config.mip_count > 0 {
            config.mip_count
        } else {
            src_width.max(src_height).ilog2().max(1)
        };

        let bloom_mips =
            Self::bloom_mip_pyramid(frame, src_width, src_height, mip_count, src_desc.format)?;

        // Pass 1: bright-pass filter → bloom_mip_0
        self.execute_bright_pass(bloom_mips.base(), config)?;

        // Pass 2: CoD:AW 13-tap downsampling chain
        for level in 0..bloom_mips.len().saturating_sub(1) {
            self.execute_downsample(bloom_mips.mip(level), bloom_mips.mip(level + 1))?;
        }

        // Pass 3: pyramid collapse — tent-upsample and accumulate back to full res
        let bloom_result = self.execute_upsample_chain(bloom_mips.levels(), frame)?;

        // Pass 4: HDR composite — scene + bloom in linear HDR space, no tonemap
        self.execute_composite(scene_color, &bloom_result, frame, config, bloom_only)
    }

    // ------------------------------------------------------------------
    // Pass implementations
    // ------------------------------------------------------------------

    fn execute_bright_pass(&self, output: &GraphImage, config: &BloomConfig) -> Result<()> {
        let constants = BrightPassConstants {
            threshold: config.threshold,
            knee: config.knee,
            _pad: [0.0, 0.0],
        };
        output.execute_shader_with_push_constants(
            &self.bright_extract_program,
            StageMask::FRAGMENT,
            bytemuck::bytes_of(&constants),
        )
    }

    fn execute_downsample(&self, input: &GraphImage, output: &GraphImage) -> Result<()> {
        input.register_as("source_tex");

        let src_desc = input.desc();
        let constants = DownsampleConstants {
            texel_size: [
                1.0 / src_desc.extent.width as f32,
                1.0 / src_desc.extent.height as f32,
            ],
        };
        output.execute_shader_with_push_constants(
            &self.downsample_program,
            StageMask::FRAGMENT,
            bytemuck::bytes_of(&constants),
        )
    }

    fn execute_upsample_chain(
        &self,
        bloom_mips: &[GraphImage],
        frame: &RenderFrame,
    ) -> Result<GraphImage> {
        let n = bloom_mips.len();
        bloom_mips[n - 1].register_as("bloom_accum");

        let base_desc = bloom_mips[0].desc();
        let up_chain = MipPyramid::new(
            frame,
            "bloom_up",
            base_desc.extent.width,
            base_desc.extent.height,
            (n - 1) as u32,
            Format::Rgba16Float,
        )?;

        for level in (0..n - 1).rev() {
            let accum_desc = bloom_mips[level + 1].desc();
            let up_image = up_chain.mip(level);

            bloom_mips[level].register_as("bloom_down");

            let constants = UpsampleConstants {
                accum_texel_size: [
                    1.0 / accum_desc.extent.width as f32,
                    1.0 / accum_desc.extent.height as f32,
                ],
            };
            up_image.execute_shader_with_push_constants(
                &self.upsample_program,
                StageMask::FRAGMENT,
                bytemuck::bytes_of(&constants),
            )?;

            up_image.register_as("bloom_accum");
        }

        Ok(up_chain.base().clone())
    }

    fn execute_composite(
        &self,
        scene_color: &GraphImage,
        bloom: &GraphImage,
        frame: &RenderFrame,
        config: &BloomConfig,
        bloom_only: bool,
    ) -> Result<GraphImage> {
        bloom.register_as("bloom_base");
        // scene_color is already in the frame registry as "scene_color".

        let ext = scene_color.desc().extent;
        let hdr_out = frame.image("hdr_composite", ImageDesc::hdr_color(ext.width, ext.height))?;

        let constants = BloomCompositeConstants {
            bloom_intensity: config.intensity,
            bloom_only: bloom_only as u32,
            _pad: [0.0; 2],
        };
        hdr_out.execute_shader_with_push_constants(
            &self.composite_program,
            StageMask::FRAGMENT,
            bytemuck::bytes_of(&constants),
        )?;

        Ok(hdr_out)
    }
}

// ------------------------------------------------------------------
// Push constant layouts
// ------------------------------------------------------------------

#[push_constants]
#[derive(Debug, Default)]
pub struct BrightPassConstants {
    pub threshold: f32,
    pub knee: f32,
    pub _pad: [f32; 2],
}

#[push_constants]
#[derive(Debug, Default)]
pub struct DownsampleConstants {
    pub texel_size: [f32; 2],
}

#[push_constants]
#[derive(Debug, Default)]
pub struct UpsampleConstants {
    pub accum_texel_size: [f32; 2],
}

#[push_constants]
#[derive(Debug, Default)]
pub struct BloomCompositeConstants {
    pub bloom_intensity: f32,
    /// When non-zero, outputs bloom-only (no scene_color) for debug visualization.
    pub bloom_only: u32,
    pub _pad: [f32; 2],
}
