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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ImageDesc {
    pub extent: Extent3d,
    pub mip_levels: u16,
    pub layers: u16,
    pub samples: u8,
    pub format: Format,
    pub usage: ImageUsage,
}

impl ImageDesc {
    pub fn validate(&self) -> Result<()> {
        if self.extent.width == 0 || self.extent.height == 0 || self.extent.depth == 0 {
            return Err(Error::InvalidInput("image extent must be non-zero".into()));
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
