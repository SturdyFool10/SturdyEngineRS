use crate::{Error, Format, Result};
use super::settings::SrdCommonSettings;
use glam::UVec2;

/// Stable per-instance denoiser routing handle.
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub struct SrdDenoiserId(u32);

impl SrdDenoiserId {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Public input/output slot taxonomy. Suffix-based direction tagging is chosen
/// deliberately so the namespace does not echo vendor SDK `IN_*` / `OUT_*`
/// conventions.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SrdResourceSlot {
    MotionVectorsInput,
    NormalRoughnessInput,
    LinearDepthInput,
    PrevNormalRoughnessInput,
    PrevLinearDepthInput,
    MaterialInput,
    HitDistanceInput,
    PrevHitDistanceInput,
    ConfidenceInput,
    DiffuseRadianceInput,
    SpecularRadianceInput,
    CombinedRadianceInput,
    OcclusionInput,
    DirectionalOcclusionInput,
    PenumbraInput,
    TranslucencyInput,
    SpectralRadianceInput,
    DiffuseRadianceOutput,
    SpecularRadianceOutput,
    CombinedRadianceOutput,
    OcclusionOutput,
    DirectionalOcclusionOutput,
    ShadowTranslucencyOutput,
    ValidationOutput,
    SurfaceMaskOutput,
    HistoryPool,
    ScratchPool,
    /// Clamped accumulated history scratch texture fed into the spatial reconstruct.
    /// Carries `pool_index` pointing into the scratch pool.
    ClampedRadianceInput,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SrdDescriptorType {
    Texture,
    StorageTexture,
    UniformBuffer,
    Sampler,
}

/// Texture pool classification. `History` slots persist across frames and are
/// SRD-owned; `Scratch` slots may be aliased and reused outside SRD between
/// dispatches.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SrdPoolClass {
    History,
    Scratch,
}

impl SrdPoolClass {
    pub(super) fn resource_slot(self) -> SrdResourceSlot {
        match self {
            Self::History => SrdResourceSlot::HistoryPool,
            Self::Scratch => SrdResourceSlot::ScratchPool,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SrdTextureDesc {
    pub name: String,
    pub debug_label: String,
    pub slot: SrdResourceSlot,
    pub format: Format,
    pub pool: Option<SrdPoolClass>,
    pub downsample_factor: u32,
}

impl SrdTextureDesc {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(Error::InvalidInput(
                "SRD texture name must not be empty".into(),
            ));
        }
        if self.debug_label.trim().is_empty() {
            return Err(Error::InvalidInput(format!(
                "SRD texture '{}' debug_label must not be empty",
                self.name
            )));
        }
        if self.downsample_factor == 0 {
            return Err(Error::InvalidInput(format!(
                "SRD texture '{}' downsample_factor must be at least 1",
                self.name
            )));
        }
        validate_slot_format(self.slot, self.format)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdResourceFormatDesc {
    pub slot: SrdResourceSlot,
    pub format: Format,
}

impl SrdResourceFormatDesc {
    pub fn validate(self) -> Result<()> {
        validate_slot_format(self.slot, self.format)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SrdResourceDesc {
    pub descriptor_type: SrdDescriptorType,
    pub slot: SrdResourceSlot,
    pub pool_index: Option<u16>,
}

/// Per-denoiser history rotation. Each frame the `write_index` slot becomes the
/// next frame's `read_index` slot via `rotate_history_ring`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SrdHistoryRing {
    pub denoiser_id: SrdDenoiserId,
    pub write_index: u16,
    pub read_index: u16,
    pub label: String,
}

impl SrdHistoryRing {
    pub fn validate(&self, history_pool: &[SrdTextureDesc]) -> Result<()> {
        let len = history_pool.len();
        if self.write_index as usize >= len || self.read_index as usize >= len {
            return Err(Error::InvalidInput(format!(
                "SRD history ring '{}' references pool indices {} and {} but history pool has {len} textures",
                self.label, self.write_index, self.read_index
            )));
        }
        if self.write_index == self.read_index {
            return Err(Error::InvalidInput(format!(
                "SRD history ring '{}' write and read indices must differ",
                self.label
            )));
        }
        if self.label.trim().is_empty() {
            return Err(Error::InvalidInput(
                "SRD history ring label must not be empty".into(),
            ));
        }
        Ok(())
    }
}

pub(super) fn validate_slot_format(slot: SrdResourceSlot, format: Format) -> Result<()> {
    if format == Format::Unknown {
        return Err(Error::InvalidInput(format!(
            "SRD resource slot {slot:?} requires a known texture format"
        )));
    }
    let valid = match slot {
        SrdResourceSlot::MotionVectorsInput => matches!(
            format,
            Format::Rg8Unorm | Format::Rgba16Float | Format::Rgba32Float
        ),
        SrdResourceSlot::NormalRoughnessInput
        | SrdResourceSlot::PrevNormalRoughnessInput
        | SrdResourceSlot::MaterialInput => {
            matches!(
                format,
                Format::Rgba8Unorm | Format::Rgba16Float | Format::Rgba32Float
            )
        }
        SrdResourceSlot::LinearDepthInput | SrdResourceSlot::PrevLinearDepthInput => matches!(
            format,
            Format::R8Unorm | Format::Rgba16Float | Format::Rgba32Float | Format::Depth32Float
        ),
        SrdResourceSlot::HitDistanceInput | SrdResourceSlot::PrevHitDistanceInput => matches!(
            format,
            Format::R8Unorm | Format::Rgba16Float | Format::Rgba32Float
        ),
        SrdResourceSlot::ConfidenceInput
        | SrdResourceSlot::OcclusionInput
        | SrdResourceSlot::DirectionalOcclusionInput
        | SrdResourceSlot::PenumbraInput => matches!(
            format,
            Format::R8Unorm | Format::Rgba16Float | Format::Rgba32Float
        ),
        SrdResourceSlot::DiffuseRadianceInput
        | SrdResourceSlot::SpecularRadianceInput
        | SrdResourceSlot::CombinedRadianceInput
        | SrdResourceSlot::TranslucencyInput
        | SrdResourceSlot::SpectralRadianceInput
        | SrdResourceSlot::DiffuseRadianceOutput
        | SrdResourceSlot::SpecularRadianceOutput
        | SrdResourceSlot::CombinedRadianceOutput
        | SrdResourceSlot::ShadowTranslucencyOutput
        | SrdResourceSlot::ValidationOutput
        | SrdResourceSlot::HistoryPool
        | SrdResourceSlot::ScratchPool
        | SrdResourceSlot::ClampedRadianceInput => matches!(
            format,
            Format::Rgba16Float | Format::Rgba32Float | Format::Rgba8Unorm
        ),
        SrdResourceSlot::OcclusionOutput | SrdResourceSlot::DirectionalOcclusionOutput => {
            matches!(
                format,
                Format::R8Unorm | Format::Rgba16Float | Format::Rgba32Float
            )
        }
        SrdResourceSlot::SurfaceMaskOutput => matches!(format, Format::R8Unorm),
    };
    if !valid {
        return Err(Error::InvalidInput(format!(
            "SRD resource slot {slot:?} does not accept format {format:?}"
        )));
    }
    Ok(())
}

pub(super) fn push_pool_texture(pool: &mut Vec<SrdTextureDesc>, texture: SrdTextureDesc) -> Result<u16> {
    let index = pool.len();
    if index > u16::MAX as usize {
        return Err(Error::InvalidInput(
            "SRD texture pool cannot contain more than 65535 textures".into(),
        ));
    }
    pool.push(texture);
    Ok(index as u16)
}

pub(super) fn clear_extent_for_srd_texture(
    common_settings: &SrdCommonSettings,
    texture: &SrdTextureDesc,
) -> UVec2 {
    let factor = texture.downsample_factor.max(1);
    UVec2::new(
        common_settings.rect_size.x.div_ceil(factor).max(1),
        common_settings.rect_size.y.div_ceil(factor).max(1),
    )
}
