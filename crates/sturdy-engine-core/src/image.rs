use crate::{Error, Result};

#[repr(u32)]
#[derive(Copy, Clone, Debug, Default, Eq, Hash, PartialEq)]
pub enum Format {
    #[default]
    Unknown = 0,
    Rgba8Unorm = 1,
    Bgra8Unorm = 2,
    Rgba16Float = 3,
    Rgba32Float = 4,
    Depth32Float = 100,
    Depth24Stencil8 = 101,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct FormatCapabilities {
    pub sampled: bool,
    pub storage: bool,
    pub color_attachment: bool,
    pub depth_stencil_attachment: bool,
    pub copy_src: bool,
    pub copy_dst: bool,
    pub linear_filter: bool,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct ImageUsage(pub u32);

impl ImageUsage {
    pub const SAMPLED: Self = Self(1 << 0);
    pub const STORAGE: Self = Self(1 << 1);
    pub const RENDER_TARGET: Self = Self(1 << 2);
    pub const DEPTH_STENCIL: Self = Self(1 << 3);
    pub const PRESENT: Self = Self(1 << 4);
    pub const COPY_SRC: Self = Self(1 << 5);
    pub const COPY_DST: Self = Self(1 << 6);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn contains(self, flag: Self) -> bool {
        (self.0 & flag.0) == flag.0
    }
}

impl std::ops::BitOr for ImageUsage {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Extent3d {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}

impl Default for Extent3d {
    fn default() -> Self {
        Self {
            width: 1,
            height: 1,
            depth: 1,
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum ImageDimension {
    D1,
    #[default]
    D2,
    D3,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ImageClearValue {
    ColorFloatBits([u32; 4]),
    DepthStencil { depth_bits: u32, stencil: u32 },
}

impl ImageClearValue {
    pub fn color_f32(rgba: [f32; 4]) -> Self {
        Self::ColorFloatBits(rgba.map(f32::to_bits))
    }

    pub fn depth_stencil(depth: f32, stencil: u32) -> Self {
        Self::DepthStencil {
            depth_bits: depth.to_bits(),
            stencil,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ImageDesc {
    pub dimension: ImageDimension,
    pub extent: Extent3d,
    pub mip_levels: u16,
    pub layers: u16,
    pub samples: u8,
    pub format: Format,
    pub usage: ImageUsage,
    pub transient: bool,
    pub clear_value: Option<ImageClearValue>,
    pub debug_name: Option<&'static str>,
}

impl ImageDesc {
    pub fn validate(&self) -> Result<()> {
        if self.extent.width == 0 || self.extent.height == 0 || self.extent.depth == 0 {
            return Err(Error::InvalidInput("image extent must be non-zero".into()));
        }
        match self.dimension {
            ImageDimension::D1 if self.extent.height != 1 || self.extent.depth != 1 => {
                return Err(Error::InvalidInput(
                    "1D image extent must have height=1 and depth=1".into(),
                ));
            }
            ImageDimension::D2 if self.extent.depth != 1 => {
                return Err(Error::InvalidInput(
                    "2D image extent must have depth=1".into(),
                ));
            }
            ImageDimension::D3 if self.layers != 1 => {
                return Err(Error::InvalidInput(
                    "3D images must use layers=1; encode depth in extent.depth".into(),
                ));
            }
            ImageDimension::D1 | ImageDimension::D2 | ImageDimension::D3 => {}
        }
        if self.mip_levels == 0 {
            return Err(Error::InvalidInput(
                "image mip_levels must be non-zero".into(),
            ));
        }
        if self.layers == 0 {
            return Err(Error::InvalidInput("image layers must be non-zero".into()));
        }
        if self.samples == 0 {
            return Err(Error::InvalidInput("image samples must be non-zero".into()));
        }
        if self.format == Format::Unknown {
            return Err(Error::InvalidInput("image format must be specified".into()));
        }
        if self.usage == ImageUsage::empty() {
            return Err(Error::InvalidInput("image usage must be non-empty".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desc() -> ImageDesc {
        ImageDesc {
            dimension: ImageDimension::D2,
            extent: Extent3d {
                width: 16,
                height: 16,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Rgba8Unorm,
            usage: ImageUsage::SAMPLED,
            transient: false,
            clear_value: Some(ImageClearValue::color_f32([0.0, 0.0, 0.0, 1.0])),
            debug_name: Some("image-desc-test"),
        }
    }

    #[test]
    fn image_desc_accepts_expanded_fields() {
        desc().validate().unwrap();
    }

    #[test]
    fn image_desc_rejects_invalid_dimension_extent() {
        let invalid = ImageDesc {
            dimension: ImageDimension::D2,
            extent: Extent3d {
                width: 16,
                height: 16,
                depth: 4,
            },
            ..desc()
        };

        assert!(matches!(invalid.validate(), Err(Error::InvalidInput(_))));
    }
}
