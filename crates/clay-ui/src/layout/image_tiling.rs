use crate::{ColorSpaceKind, UiColor};
use sturdy_engine_core::{
    Extent3d, Format, ImageDesc, ImageDimension, ImageRole, ImageUsage, Limits,
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ImageTile {
    pub index_x: u32,
    pub index_y: u32,
    pub origin: Extent3d,
    pub extent: Extent3d,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageTilingPlan {
    pub full_extent: Extent3d,
    pub max_tile_extent: u32,
    pub tiles: Vec<ImageTile>,
}

impl ImageTilingPlan {
    pub fn new_2d(width: u32, height: u32, max_tile_extent: u32) -> Self {
        let max_tile_extent = max_tile_extent.max(1);
        let mut tiles = Vec::new();
        let tiles_x = width.div_ceil(max_tile_extent).max(1);
        let tiles_y = height.div_ceil(max_tile_extent).max(1);

        for ty in 0..tiles_y {
            for tx in 0..tiles_x {
                let x = tx * max_tile_extent;
                let y = ty * max_tile_extent;
                let tile_width = width.saturating_sub(x).min(max_tile_extent).max(1);
                let tile_height = height.saturating_sub(y).min(max_tile_extent).max(1);
                tiles.push(ImageTile {
                    index_x: tx,
                    index_y: ty,
                    origin: Extent3d {
                        width: x,
                        height: y,
                        depth: 0,
                    },
                    extent: Extent3d {
                        width: tile_width,
                        height: tile_height,
                        depth: 1,
                    },
                });
            }
        }

        Self {
            full_extent: Extent3d {
                width,
                height,
                depth: 1,
            },
            max_tile_extent,
            tiles,
        }
    }

    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    pub fn to_image_descs(
        &self,
        format: Format,
        usage: ImageUsage,
        role: ImageRole,
        transient: bool,
        debug_name_prefix: Option<&'static str>,
    ) -> Vec<ImageDesc> {
        self.tiles
            .iter()
            .map(|tile| ImageDesc {
                dimension: ImageDimension::D2,
                extent: tile.extent,
                mip_levels: 1,
                layers: 1,
                samples: 1,
                format,
                usage: usage | role.default_usage(),
                transient,
                clear_value: None,
                debug_name: debug_name_prefix,
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiSurfacePlan {
    pub text_frame_info: textui::TextFrameInfo,
    pub image_tiling_plan: ImageTilingPlan,
}

impl UiSurfacePlan {
    pub fn from_limits(frame_number: u64, width: u32, height: u32, limits: &Limits) -> Self {
        let max_texture_side = limits
            .max_image_dimension_2d
            .min(limits.max_texture_2d_size)
            .max(1);
        Self {
            text_frame_info: textui::TextFrameInfo::new(frame_number, max_texture_side as usize),
            image_tiling_plan: ImageTilingPlan::new_2d(width, height, max_texture_side),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorSpaceTransformPlan {
    pub source_space: ColorSpaceKind,
    pub working_space: ColorSpaceKind,
}

impl ColorSpaceTransformPlan {
    pub fn new(source_space: ColorSpaceKind, working_space: ColorSpaceKind) -> Self {
        Self {
            source_space,
            working_space,
        }
    }

    pub fn apply(self, color: UiColor) -> UiColor {
        color
            .with_source_space(self.source_space)
            .with_transform_space(self.working_space)
    }
}
