use crate::{Error, Result};

#[repr(u32)]
#[derive(Copy, Clone, Debug, Default, Eq, Hash, PartialEq)]
pub enum Format {
    #[default]
    Unknown = 0,

    // ── Uncompressed colour ───────────────────────────────────────────────────
    Rgba8Unorm = 1,
    Bgra8Unorm = 2,
    Rgba16Float = 3,
    Rgba32Float = 4,
    /// Single-channel 8-bit unsigned normalised. Useful for roughness/AO
    /// textures before BC4 compression.
    R8Unorm = 5,
    /// Two-channel 8-bit unsigned normalised. Useful for normal map XY channels
    /// before BC5 compression.
    Rg8Unorm = 6,

    // ── Block-compressed formats ──────────────────────────────────────────────
    // All BC formats require width and height that are multiples of 4.
    // They support SAMPLED | COPY_SRC | COPY_DST usage only (not STORAGE or
    // RENDER_TARGET). The memory layout is tightly-packed 4×4 blocks.
    /// BC4 — one-channel, 0.5 bytes/texel. Lossless for greyscale data.
    /// Ideal for roughness, AO, metallic, height maps. Each 4×4 block = 8 bytes.
    Bc4Unorm = 10,
    /// BC5 — two-channel, 1 byte/texel. XY normal maps (reconstruct Z in shader
    /// via `z = sqrt(1 - x² - y²)`). Each 4×4 block = 16 bytes.
    Bc5Unorm = 11,
    /// BC3 (DXT5) — four-channel RGBA, 1 byte/texel. BC1 colour + BC4 alpha.
    /// Good general-purpose colour+alpha compression. Use when BC7 is unavailable.
    /// Each 4×4 block = 16 bytes.
    Bc3Unorm = 12,
    /// BC3 sRGB-encoded. RGB decoded as sRGB on sample. Use for sRGB albedo
    /// textures when a BC7 encoder is not available.
    Bc3UnormSrgb = 13,
    /// BC7 — four-channel, 1 byte/texel. High-quality RGBA with per-block mode
    /// selection. Best choice for albedo/diffuse/emissive (linear data).
    /// Each 4×4 block = 16 bytes. Requires a BC7-capable encoder.
    Bc7Unorm = 15,
    /// BC7 sRGB-encoded. Same block layout as `Bc7Unorm` but the GPU decodes
    /// RGB channels as sRGB. Use for sRGB albedo textures (the majority of
    /// game-ready art assets). Requires a BC7-capable encoder.
    Bc7UnormSrgb = 16,
    /// BC6H unsigned float — three-channel HDR, 1 byte/texel. For emissive maps
    /// and environment maps with values outside [0, 1]. Each 4×4 block = 16 bytes.
    Bc6hUfloat = 17,

    // ── Depth ─────────────────────────────────────────────────────────────────
    Depth32Float = 100,
    Depth24Stencil8 = 101,
}

impl Format {
    /// Returns `true` for BC1–BC7 block-compressed formats.
    pub fn is_block_compressed(self) -> bool {
        matches!(
            self,
            Format::Bc3Unorm
                | Format::Bc3UnormSrgb
                | Format::Bc4Unorm
                | Format::Bc5Unorm
                | Format::Bc7Unorm
                | Format::Bc7UnormSrgb
                | Format::Bc6hUfloat
        )
    }

    /// Bytes per 4×4 block for BC formats; 0 for non-BC formats.
    pub fn bc_block_bytes(self) -> u64 {
        match self {
            Format::Bc4Unorm => 8,
            Format::Bc3Unorm
            | Format::Bc3UnormSrgb
            | Format::Bc5Unorm
            | Format::Bc7Unorm
            | Format::Bc7UnormSrgb
            | Format::Bc6hUfloat => 16,
            _ => 0,
        }
    }
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

/// Defines the intended usage of an image. This is a bitmask.
#[derive(Copy, Clone, Debug, Default, Eq, Hash, PartialEq)]
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

impl std::ops::BitOrAssign for ImageUsage {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
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
#[derive(Copy, Clone, Debug, Default, Eq, Hash, PartialEq)]
pub enum ImageDimension {
    D1,
    #[default]
    D2,
    D3,
}

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
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

/// Semantic role of an image, used by the high-level image-centric API.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum ImageRole {
    /// General-purpose sampled texture.
    #[default]
    Texture,
    /// Written as a color render target.
    ColorAttachment,
    /// Written as a depth (or depth-stencil) attachment.
    DepthAttachment,
    /// Used as a read/write storage image in compute shaders.
    Storage,
    /// GBuffer attachment in a deferred rendering pipeline.
    GBuffer,
    /// Swapchain image ready for presentation.
    Presentable,
    /// Transient intermediate image within a frame.
    Intermediate,
}

impl ImageRole {
    /// Returns the `ImageUsage` flags appropriate for this role.
    pub fn default_usage(self) -> ImageUsage {
        match self {
            Self::Texture => ImageUsage::SAMPLED | ImageUsage::COPY_DST,
            Self::ColorAttachment => ImageUsage::RENDER_TARGET | ImageUsage::SAMPLED,
            Self::DepthAttachment => ImageUsage::DEPTH_STENCIL,
            Self::Storage => ImageUsage::STORAGE,
            Self::GBuffer => ImageUsage::RENDER_TARGET | ImageUsage::SAMPLED,
            Self::Presentable => ImageUsage::RENDER_TARGET | ImageUsage::PRESENT,
            Self::Intermediate => {
                ImageUsage::RENDER_TARGET | ImageUsage::SAMPLED | ImageUsage::STORAGE
            }
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
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

impl Default for ImageDesc {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageDesc {
    pub fn new() -> Self {
        Self {
            dimension: ImageDimension::D2,
            extent: Extent3d::default(),
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Rgba8Unorm,
            usage: ImageUsage::empty(),
            transient: false,
            clear_value: None,
            debug_name: None,
        }
    }

    /// Create a 2D FP16 HDR color image descriptor at `width × height`.
    ///
    /// Equivalent to `ImageBuilder::new_2d(Format::Rgba16Float, w, h).role(ColorAttachment).build()`.
    pub fn hdr_color(width: u32, height: u32) -> Self {
        Self {
            dimension: ImageDimension::D2,
            extent: Extent3d {
                width,
                height,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Rgba16Float,
            usage: ImageUsage::SAMPLED | ImageUsage::RENDER_TARGET,
            transient: false,
            clear_value: None,
            debug_name: None,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.extent.width == 0 || self.extent.height == 0 || self.extent.depth == 0 {
            return Err(Error::InvalidInput(
                "image extent dimensions must be non-zero".into(),
            ));
        }
        match self.dimension {
            ImageDimension::D1 => {
                if self.extent.height != 1 || self.extent.depth != 1 {
                    return Err(Error::InvalidInput(
                        "D1 image must have height=1 and depth=1".into(),
                    ));
                }
            }
            ImageDimension::D2 => {
                if self.extent.depth != 1 {
                    return Err(Error::InvalidInput("D2 image must have depth=1".into()));
                }
            }
            ImageDimension::D3 => {}
        }
        if self.mip_levels == 0 {
            return Err(Error::InvalidInput("mip_levels must be at least 1".into()));
        }
        if self.layers == 0 {
            return Err(Error::InvalidInput("layers must be at least 1".into()));
        }
        if self.samples == 0 {
            return Err(Error::InvalidInput("samples must be at least 1".into()));
        }
        Ok(())
    }
}

/// Fluent builder for [`ImageDesc`].
pub struct ImageBuilder {
    desc: ImageDesc,
}

impl ImageBuilder {
    pub fn new_2d(format: Format, width: u32, height: u32) -> Self {
        Self {
            desc: ImageDesc {
                dimension: ImageDimension::D2,
                extent: Extent3d {
                    width,
                    height,
                    depth: 1,
                },
                format,
                ..ImageDesc::new()
            },
        }
    }

    pub fn new_3d(format: Format, width: u32, height: u32, depth: u32) -> Self {
        Self {
            desc: ImageDesc {
                dimension: ImageDimension::D3,
                extent: Extent3d {
                    width,
                    height,
                    depth,
                },
                format,
                ..ImageDesc::new()
            },
        }
    }

    pub fn usage(mut self, usage: ImageUsage) -> Self {
        self.desc.usage = usage;
        self
    }

    pub fn add_usage(mut self, usage: ImageUsage) -> Self {
        self.desc.usage |= usage;
        self
    }

    pub fn role(mut self, role: ImageRole) -> Self {
        self.desc.usage |= role.default_usage();
        self
    }

    pub fn mip_levels(mut self, mip_levels: u16) -> Self {
        self.desc.mip_levels = mip_levels;
        self
    }

    pub fn layers(mut self, layers: u16) -> Self {
        self.desc.layers = layers;
        self
    }

    pub fn samples(mut self, samples: u8) -> Self {
        self.desc.samples = samples;
        self
    }

    pub fn transient(mut self) -> Self {
        self.desc.transient = true;
        self
    }

    pub fn clear_color(mut self, rgba: [f32; 4]) -> Self {
        self.desc.clear_value = Some(ImageClearValue::color_f32(rgba));
        self
    }

    pub fn clear_depth(mut self, depth: f32, stencil: u32) -> Self {
        self.desc.clear_value = Some(ImageClearValue::depth_stencil(depth, stencil));
        self
    }

    pub fn debug_name(mut self, name: &'static str) -> Self {
        self.desc.debug_name = Some(name);
        self
    }

    pub fn build(self) -> Result<ImageDesc> {
        self.desc.validate()?;
        Ok(self.desc)
    }

    pub fn build_unchecked(self) -> ImageDesc {
        self.desc
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
            usage: ImageUsage::SAMPLED | ImageUsage::COPY_DST,
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

    #[test]
    fn image_builder_produces_valid_desc() {
        let desc = ImageBuilder::new_2d(Format::Rgba16Float, 1920, 1080)
            .role(ImageRole::ColorAttachment)
            .mip_levels(1)
            .debug_name("hdr-color-buffer")
            .build()
            .unwrap();

        assert_eq!(desc.format, Format::Rgba16Float);
        assert_eq!(desc.extent.width, 1920);
        assert_eq!(desc.extent.height, 1080);
        assert!(desc.usage.contains(ImageUsage::RENDER_TARGET));
        assert_eq!(desc.debug_name, Some("hdr-color-buffer"));
    }

    #[test]
    fn image_builder_rejects_zero_extent() {
        let result = ImageBuilder::new_2d(Format::Rgba8Unorm, 0, 1080).build();
        assert!(matches!(result, Err(Error::InvalidInput(_))));
    }

    #[test]
    fn image_role_default_usage_covers_expected_flags() {
        assert!(
            ImageRole::ColorAttachment
                .default_usage()
                .contains(ImageUsage::RENDER_TARGET)
        );
        assert!(
            ImageRole::DepthAttachment
                .default_usage()
                .contains(ImageUsage::DEPTH_STENCIL)
        );
        assert!(
            ImageRole::Storage
                .default_usage()
                .contains(ImageUsage::STORAGE)
        );
        assert!(
            ImageRole::Presentable
                .default_usage()
                .contains(ImageUsage::PRESENT)
        );
    }
}
