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
    push_constants, Engine, Format, GraphImage, ImageDesc, ImageUsage, RenderFrame, Result,
    ShaderProgram, StageMask,
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

    /// Allocate the bloom mip-chain intermediate images for the given source size.
    pub fn create_bloom_mip_chain(
        frame: &RenderFrame,
        width: u32,
        height: u32,
        mip_count: u32,
        format: &Format,
    ) -> Result<Vec<GraphImage>> {
        let mut mips = Vec::with_capacity(mip_count as usize);

        for level in 0..mip_count {
            let mip_width = width.max(1) >> level;
            let mip_height = height.max(1) >> level;

            if mip_width == 0 || mip_height == 0 {
                break;
            }

            let desc = ImageDesc {
                dimension: crate::ImageDimension::D2,
                extent: crate::Extent3d {
                    width: mip_width,
                    height: mip_height,
                    depth: 1,
                },
                mip_levels: 1,
                layers: 1,
                samples: 1,
                format: if level == 0 { *format } else { Format::Rgba16Float },
                usage: ImageUsage::SAMPLED | ImageUsage::RENDER_TARGET,
                transient: false,
                clear_value: None,
                debug_name: None,
            };

            let image = frame.image(&format!("bloom_mip_{level}"), desc)?;
            mips.push(image);
        }

        Ok(mips)
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
            Self::create_bloom_mip_chain(frame, src_width, src_height, mip_count, &src_desc.format)?;

        // Pass 1: bright-pass filter → bloom_mip_0
        self.execute_bright_pass(&bloom_mips[0], config)?;

        // Pass 2: CoD:AW 13-tap downsampling chain
        for level in 0..bloom_mips.len().saturating_sub(1) {
            self.execute_downsample(&bloom_mips[level], &bloom_mips[level + 1])?;
        }

        // Pass 3: pyramid collapse — tent-upsample and accumulate back to full res
        let bloom_result = self.execute_upsample_chain(&bloom_mips, frame)?;

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

        let mut last_up: Option<GraphImage> = None;

        for level in (0..n - 1).rev() {
            let down_desc = bloom_mips[level].desc();
            let accum_desc = bloom_mips[level + 1].desc();

            let up_desc = ImageDesc {
                dimension: crate::ImageDimension::D2,
                extent: down_desc.extent,
                mip_levels: 1,
                layers: 1,
                samples: 1,
                format: Format::Rgba16Float,
                usage: ImageUsage::SAMPLED | ImageUsage::RENDER_TARGET,
                transient: false,
                clear_value: None,
                debug_name: None,
            };
            let up_image = frame.image(&format!("bloom_up_{level}"), up_desc)?;

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
            last_up = Some(up_image);
        }

        Ok(last_up.expect("bloom mip chain must have at least 2 levels"))
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

        let desc = scene_color.desc();
        let hdr_desc = ImageDesc {
            dimension: crate::ImageDimension::D2,
            extent: desc.extent,
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Rgba16Float,
            usage: ImageUsage::SAMPLED | ImageUsage::RENDER_TARGET,
            transient: false,
            clear_value: None,
            debug_name: None,
        };
        let hdr_out = frame.image("hdr_composite", hdr_desc)?;

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
