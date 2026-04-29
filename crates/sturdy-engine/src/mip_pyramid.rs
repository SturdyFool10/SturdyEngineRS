use crate::{Format, GraphImage, ImageDesc, ImageDimension, ImageUsage, RenderFrame, Result};
use sturdy_engine_core::Extent3d;

/// A sequence of same-format graph images sized as a mip pyramid.
///
/// Each level is an independent graph image named `"{base_name}_mip_{level}"`.
/// Level 0 is full resolution; each subsequent level is halved in width and
/// height. Every level can be written by a render pass and read as a texture,
/// making this suitable for downsample/upsample chains like bloom.
pub struct MipPyramid {
    levels: Vec<GraphImage>,
}

impl MipPyramid {
    /// Build a mip pyramid with `mip_count` levels starting at `width × height`.
    ///
    /// Levels are created as separate `SAMPLED | RENDER_TARGET` graph images.
    /// The format of level 0 matches `format`; all deeper levels use
    /// `Rgba16Float` to keep the chain in linear HDR space.
    pub fn new(
        frame: &RenderFrame,
        base_name: &str,
        width: u32,
        height: u32,
        mip_count: u32,
        format: Format,
    ) -> Result<Self> {
        let mut levels = Vec::with_capacity(mip_count as usize);

        for level in 0..mip_count {
            let mip_width = (width >> level).max(1);
            let mip_height = (height >> level).max(1);

            let desc = ImageDesc {
                dimension: ImageDimension::D2,
                extent: Extent3d {
                    width: mip_width,
                    height: mip_height,
                    depth: 1,
                },
                mip_levels: 1,
                layers: 1,
                samples: 1,
                format: if level == 0 {
                    format
                } else {
                    Format::Rgba16Float
                },
                usage: ImageUsage::SAMPLED | ImageUsage::RENDER_TARGET,
                transient: false,
                clear_value: None,
                debug_name: None,
            };

            let image = frame.image(&format!("{base_name}_mip_{level}"), desc)?;
            levels.push(image);
        }

        Ok(Self { levels })
    }

    /// Level 0 — full-resolution base image.
    pub fn base(&self) -> &GraphImage {
        &self.levels[0]
    }

    /// Mip level `n` (0 = full resolution).
    ///
    /// Panics if `n >= len()`.
    pub fn mip(&self, n: usize) -> &GraphImage {
        &self.levels[n]
    }

    /// Number of mip levels in this pyramid.
    pub fn len(&self) -> usize {
        self.levels.len()
    }

    /// True only when `len() == 0` (can't happen via `new`, exposed for completeness).
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    /// All levels as a slice, index 0 = coarsest full-res level.
    pub fn levels(&self) -> &[GraphImage] {
        &self.levels
    }

    /// The coarsest (smallest) level — the last in the chain.
    pub fn coarsest(&self) -> &GraphImage {
        //panic allowed, reason = "non-empty by construction: at least one mip level is always present"
        self.levels.last().expect("mip pyramid is never empty")
    }
}
