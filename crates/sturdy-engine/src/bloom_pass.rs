//! Configurable bloom post-processing pass.
//!
//! Implements the CoD:Advanced Warfare bloom approach:
//! 1. Bright-pass filter to extract luminance
//! 2. CoD:AW 13-tap downsampling mip-chain
//! 3. Hermite spline tone-mapping + bloom composite
//!
//! The pass works with the engine's reflected shader graph pattern —
//! bind groups are auto-resolved from shader reflection, so only
//! push constants need to be passed through.
//!
//! # Frame image naming contract
//!
//! Before calling [`BloomPass::execute`] the caller **must** have registered a
//! frame image named `"scene_color"` (the linear-HDR scene buffer).  The
//! bright-extract and composite shaders bind that name by reflection.

use crate::{
    Engine, Error, Format, GraphImage, ImageDesc, ImageUsage, RenderFrame, Result, ShaderProgram,
    StageMask,
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

    /// Whether to use the adaptive 13-tap/box filter (true) or always
    /// use the CoD:AW 13-tap filter for every mip level (false).
    pub adaptive_filter: bool,

    /// Whether to apply Gaussian accumulation between mip levels.
    pub gaussian_accumulate: bool,
}

impl Default for BloomConfig {
    fn default() -> Self {
        Self {
            threshold: 0.8,
            knee: 0.05,
            intensity: 0.5,
            mip_count: 0,
            adaptive_filter: true,
            gaussian_accumulate: false,
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

    /// Hermite spline tone-mapping + bloom composite shader.
    /// `None` only if the shader failed to compile at build time.
    pub tonemap_bloom_program: Option<ShaderProgram>,
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
        let tonemap_bloom = ShaderProgram::from_inline_fragment(
            engine,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/shaders/bloom_tonemap_composite.slang"
            )),
        )
        .ok();

        Ok(Self {
            bright_extract_program: bright_extract,
            downsample_program: downsample,
            tonemap_bloom_program: tonemap_bloom,
        })
    }

    /// Create from pre-loaded programs.
    pub fn from_programs(
        bright_extract: ShaderProgram,
        downsample: ShaderProgram,
        tonemap_bloom: Option<ShaderProgram>,
    ) -> Self {
        Self {
            bright_extract_program: bright_extract,
            downsample_program: downsample,
            tonemap_bloom_program: tonemap_bloom,
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

    /// Run the full bloom + tonemap pipeline.
    ///
    /// 1. Bright-pass extraction → `bloom_mip_0`
    /// 2. CoD:AW 13-tap downsampling chain
    /// 3. Hermite spline tonemap + bloom composite → `output`
    ///
    /// **Pre-condition**: the caller must have registered a frame image named
    /// `"scene_color"` before calling this method.
    pub fn execute(
        &self,
        scene_color: &GraphImage,
        output: &GraphImage,
        frame: &RenderFrame,
        config: &BloomConfig,
    ) -> Result<()> {
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
            let src = &bloom_mips[level];
            let dst = &bloom_mips[level + 1];
            self.execute_downsample(src, dst, config)?;
        }

        // Pass 3: tonemap + bloom composite
        let tonemap_prog = self.tonemap_bloom_program.as_ref().ok_or_else(|| {
            Error::Unknown("hermite_tonemap_bloom shader not available".into())
        })?;
        let bloom_base = bloom_mips.last().expect("bloom mip chain is non-empty");
        self.execute_tonemap_composite(output, bloom_base, config, tonemap_prog)
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

    fn execute_downsample(
        &self,
        input: &GraphImage,
        output: &GraphImage,
        _config: &BloomConfig,
    ) -> Result<()> {
        // The downsample shader reads "source_tex"; alias the input under that name.
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

    fn execute_tonemap_composite(
        &self,
        output: &GraphImage,
        bloom: &GraphImage,
        config: &BloomConfig,
        tonemap_prog: &ShaderProgram,
    ) -> Result<()> {
        // The tonemap shader reads "scene_color" (already in frame) and "bloom_base".
        bloom.register_as("bloom_base");

        let desc = output.desc();
        let constants = ToneBloomConstants {
            time: 0.0,
            inverse_resolution: [
                1.0 / desc.extent.width as f32,
                1.0 / desc.extent.height as f32,
            ],
            bloom_intensity: config.intensity,
            hdr_color: [1.0; 3],
            _pad2: [0.0, 0.0],
            _pad3: 0.0,
        };
        output.execute_shader_with_push_constants(
            tonemap_prog,
            StageMask::FRAGMENT,
            bytemuck::bytes_of(&constants),
        )
    }
}

// ------------------------------------------------------------------
// Push constant layouts
// ------------------------------------------------------------------

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct BrightPassConstants {
    pub threshold: f32,
    pub knee: f32,
    pub _pad: [f32; 2],
}
unsafe impl bytemuck::Pod for BrightPassConstants {}
unsafe impl bytemuck::Zeroable for BrightPassConstants {}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct DownsampleConstants {
    pub texel_size: [f32; 2],
}
unsafe impl bytemuck::Pod for DownsampleConstants {}
unsafe impl bytemuck::Zeroable for DownsampleConstants {}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct ToneBloomConstants {
    pub time: f32,
    pub inverse_resolution: [f32; 2],
    pub bloom_intensity: f32,
    pub hdr_color: [f32; 3],
    pub _pad2: [f32; 2],
    pub _pad3: f32,
}
unsafe impl bytemuck::Pod for ToneBloomConstants {}
unsafe impl bytemuck::Zeroable for ToneBloomConstants {}

/// Push constants for the standard composite pass (fallback).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct CompositeConstants {
    pub bloom_intensity: f32,
    pub bloom_padding: [f32; 3],
    pub scene_padding: [f32; 2],
    pub _pad: f32,
}
unsafe impl bytemuck::Pod for CompositeConstants {}
unsafe impl bytemuck::Zeroable for CompositeConstants {}
