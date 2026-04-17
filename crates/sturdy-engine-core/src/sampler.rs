use crate::{Error, Result};

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum FilterMode {
    Nearest,
    #[default]
    Linear,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum MipmapMode {
    Nearest,
    #[default]
    Linear,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum AddressMode {
    #[default]
    Repeat,
    MirroredRepeat,
    ClampToEdge,
    ClampToBorder,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum CompareOp {
    #[default]
    Never,
    Less,
    Equal,
    LessOrEqual,
    Greater,
    NotEqual,
    GreaterOrEqual,
    Always,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum BorderColor {
    FloatTransparentBlack,
    #[default]
    IntTransparentBlack,
    FloatOpaqueBlack,
    IntOpaqueBlack,
    FloatOpaqueWhite,
    IntOpaqueWhite,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SamplerDesc {
    pub mag_filter: FilterMode,
    pub min_filter: FilterMode,
    pub mipmap_mode: MipmapMode,
    pub address_u: AddressMode,
    pub address_v: AddressMode,
    pub address_w: AddressMode,
    pub mip_lod_bias: f32,
    pub max_anisotropy: Option<f32>,
    pub compare: Option<CompareOp>,
    pub min_lod: f32,
    pub max_lod: f32,
    pub border_color: BorderColor,
    pub unnormalized_coordinates: bool,
}

impl Default for SamplerDesc {
    fn default() -> Self {
        Self {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_mode: MipmapMode::Linear,
            address_u: AddressMode::Repeat,
            address_v: AddressMode::Repeat,
            address_w: AddressMode::Repeat,
            mip_lod_bias: 0.0,
            max_anisotropy: None,
            compare: None,
            min_lod: 0.0,
            max_lod: f32::MAX,
            border_color: BorderColor::IntTransparentBlack,
            unnormalized_coordinates: false,
        }
    }
}

impl SamplerDesc {
    pub fn validate(&self) -> Result<()> {
        if !self.mip_lod_bias.is_finite() {
            return Err(Error::InvalidInput(
                "sampler mip_lod_bias must be finite".into(),
            ));
        }
        if !self.min_lod.is_finite() || !self.max_lod.is_finite() {
            return Err(Error::InvalidInput(
                "sampler LOD range must be finite".into(),
            ));
        }
        if self.min_lod > self.max_lod {
            return Err(Error::InvalidInput(
                "sampler min_lod must be <= max_lod".into(),
            ));
        }
        if let Some(max_anisotropy) = self.max_anisotropy {
            if !max_anisotropy.is_finite() || max_anisotropy < 1.0 {
                return Err(Error::InvalidInput(
                    "sampler max_anisotropy must be finite and >= 1.0".into(),
                ));
            }
        }
        if self.unnormalized_coordinates {
            if self.mipmap_mode != MipmapMode::Nearest
                || self.min_lod != 0.0
                || self.max_lod != 0.0
                || self.compare.is_some()
                || self.max_anisotropy.is_some()
            {
                return Err(Error::InvalidInput(
                    "unnormalized-coordinate samplers require nearest mip mode, lod 0, no compare, and no anisotropy".into(),
                ));
            }
            if !matches!(
                self.address_u,
                AddressMode::ClampToEdge | AddressMode::ClampToBorder
            ) || !matches!(
                self.address_v,
                AddressMode::ClampToEdge | AddressMode::ClampToBorder
            ) {
                return Err(Error::InvalidInput(
                    "unnormalized-coordinate samplers require clamp address modes for u/v".into(),
                ));
            }
        }
        Ok(())
    }
}
