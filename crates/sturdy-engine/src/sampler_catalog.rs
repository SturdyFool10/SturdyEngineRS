use std::collections::HashMap;

use sturdy_engine_core as core;

use crate::{
    AddressMode, BorderColor, CompareOp, Engine, FilterMode, MipmapMode, Result, Sampler,
    SamplerDesc,
};

/// Named sampler presets covering the common cases for 2D, 3D, and post-process work.
///
/// Pick one when registering a sampler name on a `RenderFrame`:
/// ```rust,ignore
/// frame.set_sampler("sprite_sampler", SamplerPreset::PixelArt);
/// frame.set_sampler("terrain_sampler", SamplerPreset::Aniso16x);
/// ```
/// The engine resolves sampler bindings by matching the Slang variable name
/// against names registered on the current frame, falling back to `Linear`.
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub enum SamplerPreset {
    /// Bilinear, clamp to edge. The default when no name is registered.
    Linear,
    /// Bilinear, repeat. Good for seamlessly tiling textures.
    LinearRepeat,
    /// Bilinear, mirrored repeat.
    LinearMirror,
    /// Trilinear (linear mip interpolation), clamp. Standard for 3D surfaces.
    Trilinear,
    /// 4× anisotropic, clamp. Better quality on oblique viewing angles.
    Aniso4x,
    /// 16× anisotropic, clamp. Highest quality for 3D surfaces.
    Aniso16x,
    /// Nearest-neighbor, clamp to edge. Crisp pixel art with no blurring.
    PixelArt,
    /// Nearest-neighbor, repeat. Tiled pixel art and retro terrain.
    PixelArtRepeat,
    /// Nearest-neighbor, mirrored repeat. Mirrored pixel art tiles.
    PixelArtMirror,
    /// Linear, clamp to border (black), depth compare ≤. Shadow map PCF.
    Shadow,
}

impl SamplerPreset {
    pub fn desc(self) -> SamplerDesc {
        let linear_clamp = SamplerDesc {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_mode: MipmapMode::Linear,
            address_u: AddressMode::ClampToEdge,
            address_v: AddressMode::ClampToEdge,
            address_w: AddressMode::ClampToEdge,
            ..SamplerDesc::default()
        };
        match self {
            Self::Linear => linear_clamp,
            Self::LinearRepeat => SamplerDesc {
                address_u: AddressMode::Repeat,
                address_v: AddressMode::Repeat,
                address_w: AddressMode::Repeat,
                ..linear_clamp
            },
            Self::LinearMirror => SamplerDesc {
                address_u: AddressMode::MirroredRepeat,
                address_v: AddressMode::MirroredRepeat,
                address_w: AddressMode::MirroredRepeat,
                ..linear_clamp
            },
            Self::Trilinear => linear_clamp,
            Self::Aniso4x => SamplerDesc {
                max_anisotropy: Some(4.0),
                ..linear_clamp
            },
            Self::Aniso16x => SamplerDesc {
                max_anisotropy: Some(16.0),
                ..linear_clamp
            },
            Self::PixelArt => SamplerDesc {
                mag_filter: FilterMode::Nearest,
                min_filter: FilterMode::Nearest,
                mipmap_mode: MipmapMode::Nearest,
                address_u: AddressMode::ClampToEdge,
                address_v: AddressMode::ClampToEdge,
                address_w: AddressMode::ClampToEdge,
                min_lod: 0.0,
                max_lod: 0.0,
                ..SamplerDesc::default()
            },
            Self::PixelArtRepeat => SamplerDesc {
                mag_filter: FilterMode::Nearest,
                min_filter: FilterMode::Nearest,
                mipmap_mode: MipmapMode::Nearest,
                address_u: AddressMode::Repeat,
                address_v: AddressMode::Repeat,
                address_w: AddressMode::Repeat,
                min_lod: 0.0,
                max_lod: 0.0,
                ..SamplerDesc::default()
            },
            Self::PixelArtMirror => SamplerDesc {
                mag_filter: FilterMode::Nearest,
                min_filter: FilterMode::Nearest,
                mipmap_mode: MipmapMode::Nearest,
                address_u: AddressMode::MirroredRepeat,
                address_v: AddressMode::MirroredRepeat,
                address_w: AddressMode::MirroredRepeat,
                min_lod: 0.0,
                max_lod: 0.0,
                ..SamplerDesc::default()
            },
            Self::Shadow => SamplerDesc {
                mag_filter: FilterMode::Linear,
                min_filter: FilterMode::Linear,
                mipmap_mode: MipmapMode::Nearest,
                address_u: AddressMode::ClampToBorder,
                address_v: AddressMode::ClampToBorder,
                address_w: AddressMode::ClampToBorder,
                compare: Some(CompareOp::LessOrEqual),
                border_color: BorderColor::FloatOpaqueWhite,
                min_lod: 0.0,
                max_lod: 0.0,
                ..SamplerDesc::default()
            },
        }
    }
}

pub(crate) struct SamplerCatalog {
    samplers: HashMap<SamplerPreset, Sampler>,
}

impl SamplerCatalog {
    pub(crate) fn empty() -> Self {
        Self {
            samplers: HashMap::new(),
        }
    }

    pub(crate) fn build(engine: &Engine) -> Result<Self> {
        let caps = engine.caps();
        let has_anisotropy = caps
            .raw_feature_names
            .iter()
            .any(|n| n == "sampler_anisotropy");

        let presets = [
            SamplerPreset::Linear,
            SamplerPreset::LinearRepeat,
            SamplerPreset::LinearMirror,
            SamplerPreset::Trilinear,
            SamplerPreset::Aniso4x,
            SamplerPreset::Aniso16x,
            SamplerPreset::PixelArt,
            SamplerPreset::PixelArtRepeat,
            SamplerPreset::PixelArtMirror,
            SamplerPreset::Shadow,
        ];
        let mut samplers = HashMap::with_capacity(presets.len());
        for preset in presets {
            let mut desc = preset.desc();
            if desc.max_anisotropy.is_some() && !has_anisotropy {
                desc.max_anisotropy = None;
            }
            samplers.insert(preset, engine.create_sampler(desc)?);
        }
        Ok(Self { samplers })
    }

    pub(crate) fn handle(&self, preset: SamplerPreset) -> core::SamplerHandle {
        self.samplers[&preset].handle()
    }
}
