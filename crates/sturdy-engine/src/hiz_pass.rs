//! Hierarchical-Z depth pyramid pass.
//!
//! Builds a max-depth pyramid from a full-resolution depth image. The levels are
//! independent graph images named `"{base_name}_mip_{level}"`, which keeps the
//! contract simple for features that need sampled depth hierarchy access.

use sturdy_engine_core::Extent3d;

use crate::{
    ComputeProgram, Engine, Format, GraphImage, GraphImageHistory, ImageDesc, ImageDimension,
    ImageUsage, RenderFrame, Result,
};

const HIZ_SHADER_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/shaders/hiz_downsample.slang");

/// Configuration for [`HizPass`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct HizConfig {
    /// Maximum number of levels to build.
    ///
    /// The actual count is clamped to the full pyramid length for the depth
    /// image. Values below 1 are treated as 1.
    pub max_mip_levels: u32,
}

impl Default for HizConfig {
    fn default() -> Self {
        Self {
            max_mip_levels: u32::MAX,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct HizPush {
    source_extent: [u32; 2],
    copy_source: u32,
    _pad: u32,
}

/// Per-level ping-pong history for a Hi-Z pyramid.
///
/// GPU occlusion culling normally consumes the previous frame's hierarchy while
/// the current frame writes a fresh hierarchy. This helper owns that history
/// state without coupling callers to the underlying graph image names.
#[derive(Clone, Debug, Default)]
pub struct HizHistory {
    levels: Vec<GraphImageHistory>,
}

impl HizHistory {
    /// Create an empty Hi-Z history.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of per-level histories currently retained.
    pub fn len(&self) -> usize {
        self.levels.len()
    }

    /// True when no levels have been allocated yet.
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    /// Drop all retained history. The next history build will report no
    /// previous pyramid.
    pub fn clear(&mut self) {
        self.levels.clear();
    }
}

/// Output depth hierarchy from [`HizPass::execute`].
pub struct HizPyramid {
    base_name: String,
    width: u32,
    height: u32,
    levels: Vec<GraphImage>,
}

impl HizPyramid {
    /// Logical base name used to create level images.
    pub fn base_name(&self) -> &str {
        &self.base_name
    }

    /// Full-resolution width of level 0.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Full-resolution height of level 0.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Number of levels as a `u32`, matching shader/config descriptors.
    pub fn mip_levels(&self) -> u32 {
        self.levels.len() as u32
    }

    /// Level 0 of the Hi-Z pyramid.
    pub fn base(&self) -> &GraphImage {
        &self.levels[0]
    }

    /// Mip level `n` (0 = full resolution).
    ///
    /// Panics if `n >= len()`.
    pub fn mip(&self, n: usize) -> &GraphImage {
        &self.levels[n]
    }

    /// Dimensions of mip level `n`.
    ///
    /// Panics if `n >= len()`.
    pub fn mip_extent(&self, n: usize) -> Extent3d {
        self.levels[n].desc().extent
    }

    /// Number of levels in the pyramid.
    pub fn len(&self) -> usize {
        self.levels.len()
    }

    /// True only when `len() == 0` (can't happen via `execute`, exposed for completeness).
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    /// All levels, index 0 = full resolution.
    pub fn levels(&self) -> &[GraphImage] {
        &self.levels
    }

    /// The coarsest, smallest level.
    pub fn coarsest(&self) -> &GraphImage {
        //panic allowed, reason = "non-empty by construction: at least one mip level is always present"
        self.levels.last().expect("Hi-Z pyramid is never empty")
    }

    /// Register all levels under `"{prefix}_mip_{level}"` for reflected shader
    /// binding.
    pub fn register_levels(&self, prefix: &str) {
        for (level, image) in self.levels.iter().enumerate() {
            image.register_as(format!("{prefix}_mip_{level}"));
        }
    }

    /// Register the coarsest level under a shader binding name.
    ///
    /// This is useful for conservative coarse tests before a feature is wired to
    /// all pyramid levels.
    pub fn register_coarsest_as(&self, name: impl Into<String>) {
        self.coarsest().register_as(name);
    }
}

/// Result of building a history-backed Hi-Z pyramid.
pub struct HizHistoryFrame {
    /// Current-frame hierarchy written by the build passes.
    pub current: HizPyramid,
    /// Previous-frame hierarchy, if every level had valid history.
    pub previous: Option<HizPyramid>,
    /// Ping-pong frame index for this history update.
    pub frame_index: u64,
}

/// Reusable compute pass that builds a max-depth Hi-Z pyramid.
pub struct HizPass {
    config: HizConfig,
    program: ComputeProgram,
}

impl HizPass {
    /// Create a Hi-Z pass with the default full-pyramid configuration.
    pub fn new(engine: &Engine) -> Result<Self> {
        Self::with_config(engine, HizConfig::default())
    }

    /// Create a Hi-Z pass with explicit configuration.
    pub fn with_config(engine: &Engine, config: HizConfig) -> Result<Self> {
        Ok(Self {
            config,
            program: ComputeProgram::load(engine, HIZ_SHADER_PATH)?,
        })
    }

    /// Current configuration.
    pub fn config(&self) -> HizConfig {
        self.config
    }

    /// Mutable configuration.
    pub fn config_mut(&mut self) -> &mut HizConfig {
        &mut self.config
    }

    /// Build a Hi-Z pyramid from `depth`.
    ///
    /// Output images use `Rgba32Float` with the max depth stored in the red
    /// channel. This matches the engine's current single-channel storage-image
    /// workaround and keeps the graph contract backend-neutral.
    pub fn execute(&self, frame: &RenderFrame, depth: &GraphImage) -> Result<HizPyramid> {
        self.execute_named(frame, "hiz", depth)
    }

    /// Build a Hi-Z pyramid with a custom graph-image base name.
    pub fn execute_named(
        &self,
        frame: &RenderFrame,
        base_name: impl Into<String>,
        depth: &GraphImage,
    ) -> Result<HizPyramid> {
        let base_name = base_name.into();
        let depth_desc = depth.desc();
        let width = depth_desc.extent.width.max(1);
        let height = depth_desc.extent.height.max(1);
        let mip_count = full_mip_count(width, height).min(self.config.max_mip_levels.max(1));
        let levels = self.allocate_levels(frame, &base_name, width, height, mip_count)?;
        self.record_build(frame, &base_name, depth, &levels)?;

        Ok(HizPyramid {
            base_name,
            width,
            height,
            levels,
        })
    }

    /// Build the current Hi-Z pyramid into ping-pong history images and return
    /// the previous pyramid when available.
    pub fn execute_history(
        &self,
        frame: &RenderFrame,
        depth: &GraphImage,
        history: &mut HizHistory,
    ) -> Result<HizHistoryFrame> {
        self.execute_history_named(frame, "hiz_history", depth, history)
    }

    /// Build a history-backed Hi-Z pyramid with a custom graph-image base name.
    pub fn execute_history_named(
        &self,
        frame: &RenderFrame,
        base_name: impl Into<String>,
        depth: &GraphImage,
        history: &mut HizHistory,
    ) -> Result<HizHistoryFrame> {
        let base_name = base_name.into();
        let depth_desc = depth.desc();
        let width = depth_desc.extent.width.max(1);
        let height = depth_desc.extent.height.max(1);
        let mip_count = full_mip_count(width, height).min(self.config.max_mip_levels.max(1));

        history
            .levels
            .resize_with(mip_count as usize, GraphImageHistory::new);
        history.levels.truncate(mip_count as usize);

        let mut current_levels = Vec::with_capacity(mip_count as usize);
        let mut previous_levels = Vec::with_capacity(mip_count as usize);
        let mut has_complete_history = true;
        let mut frame_index = 0;

        for level in 0..mip_count {
            let level_name = format!("{base_name}_mip_{level}");
            let pair = frame.history_images(
                &mut history.levels[level as usize],
                &level_name,
                hiz_level_desc(width, height, level),
            )?;
            has_complete_history &= pair.has_history;
            frame_index = pair.frame_index;
            previous_levels.push(pair.read);
            current_levels.push(pair.write);
        }

        self.record_build(frame, &base_name, depth, &current_levels)?;

        let current = HizPyramid {
            base_name: base_name.clone(),
            width,
            height,
            levels: current_levels,
        };
        let previous = has_complete_history.then(|| HizPyramid {
            base_name: format!("{base_name}_previous"),
            width,
            height,
            levels: previous_levels,
        });

        Ok(HizHistoryFrame {
            current,
            previous,
            frame_index,
        })
    }

    fn allocate_levels(
        &self,
        frame: &RenderFrame,
        base_name: &str,
        width: u32,
        height: u32,
        mip_count: u32,
    ) -> Result<Vec<GraphImage>> {
        let mut levels = Vec::with_capacity(mip_count as usize);
        for level in 0..mip_count {
            levels.push(frame.image(
                format!("{base_name}_mip_{level}"),
                hiz_level_desc(width, height, level),
            )?);
        }
        Ok(levels)
    }

    fn record_build(
        &self,
        frame: &RenderFrame,
        pass_base_name: &str,
        depth: &GraphImage,
        levels: &[GraphImage],
    ) -> Result<()> {
        for level in 0..levels.len() {
            let source = if level == 0 {
                depth
            } else {
                &levels[level - 1]
            };
            let source_desc = source.desc();
            let target = &levels[level];
            let target_desc = target.desc();
            let push = HizPush {
                source_extent: [
                    source_desc.extent.width.max(1),
                    source_desc.extent.height.max(1),
                ],
                copy_source: u32::from(level == 0),
                _pad: 0,
            };
            frame
                .shader_pass(format!("{pass_base_name}_mip_{level}"))
                .target_as("hiz_out", target)
                .image("hiz_source", source)
                .constants_auto(&push)
                .compute(
                    &self.program,
                    [
                        target_desc.extent.width.div_ceil(8),
                        target_desc.extent.height.div_ceil(8),
                        1,
                    ],
                )?;
        }
        Ok(())
    }
}

fn hiz_level_desc(width: u32, height: u32, level: u32) -> ImageDesc {
    ImageDesc {
        dimension: ImageDimension::D2,
        extent: Extent3d {
            width: (width >> level).max(1),
            height: (height >> level).max(1),
            depth: 1,
        },
        mip_levels: 1,
        layers: 1,
        samples: 1,
        format: Format::Rgba32Float,
        usage: ImageUsage::SAMPLED | ImageUsage::STORAGE,
        transient: false,
        clear_value: None,
        debug_name: None,
        compression: Default::default(),
        min_lod_bits: None,
        msaa_resolve_to_single_sampled: false,
    }
}

fn full_mip_count(width: u32, height: u32) -> u32 {
    u32::BITS - width.max(height).max(1).leading_zeros()
}

#[cfg(test)]
mod tests {
    use super::full_mip_count;

    #[test]
    fn hiz_full_mip_count_handles_non_power_of_two_extents() {
        assert_eq!(full_mip_count(1, 1), 1);
        assert_eq!(full_mip_count(2, 1), 2);
        assert_eq!(full_mip_count(3, 2), 2);
        assert_eq!(full_mip_count(4, 4), 3);
        assert_eq!(full_mip_count(1920, 1080), 11);
    }
}
