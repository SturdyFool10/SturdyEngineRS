use crate::{Error, Result, SamplerPreset};
use glam::{Mat4, UVec2, Vec2};
use std::mem;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdDenoiserSettings {
    pub max_frames: u32,
    pub mode: SrdDenoiserMode,
    pub current_sampler: SamplerPreset,
    pub history_sampler: SamplerPreset,
}

impl SrdDenoiserSettings {
    pub fn new(max_frames: u32) -> Self {
        Self {
            max_frames: max_frames.max(1),
            mode: SrdDenoiserMode::ReferenceTemporal,
            current_sampler: SamplerPreset::Linear,
            history_sampler: SamplerPreset::Linear,
        }
    }

    pub fn normalized(mut self) -> Self {
        self.max_frames = self.max_frames.max(1);
        self
    }

    pub fn validate(self) -> Result<()> {
        if self.max_frames == 0 {
            return Err(Error::InvalidInput(
                "SRD max accumulation frame count must be at least 1".into(),
            ));
        }
        Ok(())
    }
}

impl Default for SrdDenoiserSettings {
    fn default() -> Self {
        Self::new(64)
    }
}

/// Engine-blessed SRD denoiser modes. Names describe SturdyEngine reconstruction
/// behavior; they intentionally do not mirror third-party denoiser family names.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum SrdDenoiserMode {
    /// Reference temporal accumulation for progressive samples.
    ReferenceTemporal = 0,
    /// Reserved for the engine-standard radiance stabilizer family.
    RadianceStabilizer = 1,
    /// Reserved for the engine-standard shadow stabilizer family.
    ShadowStabilizer = 2,
    /// Reserved for the engine-standard AO/directional-occlusion stabilizer family.
    OcclusionStabilizer = 3,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdTemporalBindings {
    pub current_signal: &'static str,
    pub history_signal: &'static str,
    pub current_sampler: &'static str,
    pub history_sampler: &'static str,
}

impl Default for SrdTemporalBindings {
    fn default() -> Self {
        Self {
            current_signal: "srd_current_signal",
            history_signal: "srd_history_signal",
            current_sampler: "srd_current_sampler",
            history_sampler: "srd_history_sampler",
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdTemporalConstants {
    pub frame_index: u32,
    pub has_history: u32,
    pub max_frames: u32,
    pub mode: u32,
}

pub const SRD_TEMPORAL_CONSTANTS_SIZE: usize = mem::size_of::<SrdTemporalConstants>();

/// Engine-native history lifecycle. Distinct from vendor SDK accumulation modes
/// in both naming and explicit "zero" semantics.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum SrdHistoryMode {
    /// Continue temporal integration using existing history.
    KeepAccumulating = 0,
    /// Drop history references without clearing storage; next frame bootstraps fresh.
    InvalidateHistory = 1,
    /// Drop history and explicitly zero out persistent storage this frame.
    ZeroHistory = 2,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SrdNormalPacking {
    Rgba8SignedOctahedralRoughness,
    Rgba16FloatXyzRoughness,
    Rgba32FloatXyzRoughness,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SrdMotionVectorConvention {
    PreviousMinusCurrentPixels,
    PreviousMinusCurrentUv,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SrdDepthConvention {
    LinearViewDepth,
    HardwareDepth,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SrdSpectralLayout {
    Disabled,
    Rgb,
    FixedBins { bins: u8 },
    CompactCoefficients { coefficients: u8 },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdShaderContract {
    pub normal_packing: SrdNormalPacking,
    pub motion_vectors: SrdMotionVectorConvention,
    pub depth: SrdDepthConvention,
    pub spectral_layout: SrdSpectralLayout,
}

impl Default for SrdShaderContract {
    fn default() -> Self {
        Self {
            normal_packing: SrdNormalPacking::Rgba8SignedOctahedralRoughness,
            motion_vectors: SrdMotionVectorConvention::PreviousMinusCurrentPixels,
            depth: SrdDepthConvention::LinearViewDepth,
            spectral_layout: SrdSpectralLayout::Rgb,
        }
    }
}

impl SrdShaderContract {
    pub fn validate(self) -> Result<()> {
        match self.spectral_layout {
            SrdSpectralLayout::FixedBins { bins } if !(2..=16).contains(&bins) => {
                return Err(Error::InvalidInput(format!(
                    "SRD fixed spectral bin count must be in 2..=16, got {bins}"
                )));
            }
            SrdSpectralLayout::CompactCoefficients { coefficients }
                if !(1..=8).contains(&coefficients) =>
            {
                return Err(Error::InvalidInput(format!(
                    "SRD compact spectral coefficient count must be in 1..=8, got {coefficients}"
                )));
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SrdCommonSettings {
    pub frame_index: u64,
    pub history_mode: SrdHistoryMode,
    pub resource_size: UVec2,
    pub resource_size_prev: UVec2,
    pub rect_size: UVec2,
    pub rect_size_prev: UVec2,
    pub camera_jitter: Vec2,
    pub camera_jitter_prev: Vec2,
    pub view_to_clip: Mat4,
    pub view_to_clip_prev: Mat4,
    pub world_to_view: Mat4,
    pub world_to_view_prev: Mat4,
    pub motion_vector_scale: Vec2,
    pub linear_depth_scale: f32,
    pub effective_range: f32,
    pub split_screen: f32,
    pub enable_validation: bool,
    pub shader_contract: SrdShaderContract,
}

impl Default for SrdCommonSettings {
    fn default() -> Self {
        Self {
            frame_index: 0,
            history_mode: SrdHistoryMode::ZeroHistory,
            resource_size: UVec2::ONE,
            resource_size_prev: UVec2::ONE,
            rect_size: UVec2::ONE,
            rect_size_prev: UVec2::ONE,
            camera_jitter: Vec2::ZERO,
            camera_jitter_prev: Vec2::ZERO,
            view_to_clip: Mat4::IDENTITY,
            view_to_clip_prev: Mat4::IDENTITY,
            world_to_view: Mat4::IDENTITY,
            world_to_view_prev: Mat4::IDENTITY,
            motion_vector_scale: Vec2::ONE,
            linear_depth_scale: 1.0,
            effective_range: 1.0,
            split_screen: 0.0,
            enable_validation: false,
            shader_contract: SrdShaderContract::default(),
        }
    }
}

impl SrdCommonSettings {
    pub fn validate(&self) -> Result<()> {
        validate_nonzero_size("resource_size", self.resource_size)?;
        validate_nonzero_size("resource_size_prev", self.resource_size_prev)?;
        validate_nonzero_size("rect_size", self.rect_size)?;
        validate_nonzero_size("rect_size_prev", self.rect_size_prev)?;
        validate_finite_vec2("camera_jitter", self.camera_jitter)?;
        validate_finite_vec2("camera_jitter_prev", self.camera_jitter_prev)?;
        validate_finite_vec2("motion_vector_scale", self.motion_vector_scale)?;
        validate_positive_finite("linear_depth_scale", self.linear_depth_scale)?;
        validate_positive_finite("effective_range", self.effective_range)?;
        if !(0.0..=1.0).contains(&self.split_screen) || !self.split_screen.is_finite() {
            return Err(Error::InvalidInput(
                "SRD split_screen must be finite and in the inclusive range [0, 1]".into(),
            ));
        }
        self.shader_contract.validate()?;
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdCapabilities {
    pub temporal_history: bool,
    pub compute: bool,
    pub storage_textures: bool,
    pub half_float: bool,
    pub subgroup_ops: bool,
    pub ray_tracing_guides: bool,
    pub max_workgroup_size: [u32; 3],
}

impl Default for SrdCapabilities {
    fn default() -> Self {
        Self {
            temporal_history: true,
            compute: true,
            storage_textures: true,
            half_float: true,
            subgroup_ops: false,
            ray_tracing_guides: false,
            max_workgroup_size: [1024, 1024, 64],
        }
    }
}

impl SrdCapabilities {
    pub fn minimal_reference_temporal() -> Self {
        Self {
            temporal_history: true,
            compute: false,
            storage_textures: false,
            half_float: false,
            subgroup_ops: false,
            ray_tracing_guides: false,
            max_workgroup_size: [1, 1, 1],
        }
    }

    pub fn validate(self) -> Result<()> {
        if !self.temporal_history {
            return Err(Error::InvalidInput(
                "SRD requires temporal history support for the current implementation".into(),
            ));
        }
        if self.max_workgroup_size.iter().any(|v| *v == 0) {
            return Err(Error::InvalidInput(
                "SRD max_workgroup_size must be non-zero in all dimensions".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SrdVarianceSettings {
    pub track_luminance_moments: bool,
    pub track_signal_variance: bool,
    pub neighborhood_radius: f32,
}

impl Default for SrdVarianceSettings {
    fn default() -> Self {
        Self {
            track_luminance_moments: true,
            track_signal_variance: true,
            neighborhood_radius: 1.5,
        }
    }
}

impl SrdVarianceSettings {
    pub fn validate(self) -> Result<()> {
        validate_nonnegative_finite("variance neighborhood_radius", self.neighborhood_radius)
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SrdHistoryRejectionSettings {
    pub depth_threshold: f32,
    pub normal_threshold: f32,
    pub roughness_threshold: f32,
    pub material_mismatch_rejects: bool,
}

impl Default for SrdHistoryRejectionSettings {
    fn default() -> Self {
        Self {
            depth_threshold: 0.02,
            normal_threshold: 0.2,
            roughness_threshold: 0.25,
            material_mismatch_rejects: true,
        }
    }
}

impl SrdHistoryRejectionSettings {
    pub fn validate(self) -> Result<()> {
        validate_nonnegative_finite("history rejection depth_threshold", self.depth_threshold)?;
        validate_nonnegative_finite("history rejection normal_threshold", self.normal_threshold)?;
        validate_nonnegative_finite(
            "history rejection roughness_threshold",
            self.roughness_threshold,
        )
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SrdOutlierClampSettings {
    pub enabled: bool,
    pub luminance_sigma: f32,
    pub max_relative_luminance: f32,
}

impl Default for SrdOutlierClampSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            luminance_sigma: 4.0,
            max_relative_luminance: 16.0,
        }
    }
}

impl SrdOutlierClampSettings {
    pub fn validate(self) -> Result<()> {
        validate_positive_finite("outlier clamp luminance_sigma", self.luminance_sigma)?;
        validate_positive_finite(
            "outlier clamp max_relative_luminance",
            self.max_relative_luminance,
        )
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum SrdFamilySettings {
    Reference(SrdReferenceSettings),
    Radiance(SrdRadianceSettings),
    Shadow(SrdShadowSettings),
    Occlusion(SrdOcclusionSettings),
}

impl SrdFamilySettings {
    pub fn default_for_mode(mode: SrdDenoiserMode) -> Self {
        match mode {
            SrdDenoiserMode::ReferenceTemporal => Self::Reference(SrdReferenceSettings::default()),
            SrdDenoiserMode::RadianceStabilizer => Self::Radiance(SrdRadianceSettings::default()),
            SrdDenoiserMode::ShadowStabilizer => Self::Shadow(SrdShadowSettings::default()),
            SrdDenoiserMode::OcclusionStabilizer => {
                Self::Occlusion(SrdOcclusionSettings::default())
            }
        }
    }

    pub fn mode(self) -> SrdDenoiserMode {
        match self {
            Self::Reference(_) => SrdDenoiserMode::ReferenceTemporal,
            Self::Radiance(_) => SrdDenoiserMode::RadianceStabilizer,
            Self::Shadow(_) => SrdDenoiserMode::ShadowStabilizer,
            Self::Occlusion(_) => SrdDenoiserMode::OcclusionStabilizer,
        }
    }

    pub fn validate(self) -> Result<()> {
        match self {
            Self::Reference(settings) => settings.validate(),
            Self::Radiance(settings) => settings.validate(),
            Self::Shadow(settings) => settings.validate(),
            Self::Occlusion(settings) => settings.validate(),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SrdReferenceSettings {
    pub history_frame_budget: u32,
}

impl Default for SrdReferenceSettings {
    fn default() -> Self {
        Self {
            history_frame_budget: 64,
        }
    }
}

impl SrdReferenceSettings {
    pub fn validate(self) -> Result<()> {
        if self.history_frame_budget == 0 {
            return Err(Error::InvalidInput(
                "SRD reference history_frame_budget must be at least 1".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SrdRadianceSettings {
    pub history_frame_budget: u32,
    pub fast_history_budget: u32,
    pub spatial_radius: f32,
    pub variance: SrdVarianceSettings,
    pub history_rejection: SrdHistoryRejectionSettings,
    pub outlier_clamp: SrdOutlierClampSettings,
}

impl Default for SrdRadianceSettings {
    fn default() -> Self {
        Self {
            history_frame_budget: 64,
            fast_history_budget: 8,
            spatial_radius: 3.0,
            variance: SrdVarianceSettings::default(),
            history_rejection: SrdHistoryRejectionSettings::default(),
            outlier_clamp: SrdOutlierClampSettings::default(),
        }
    }
}

impl SrdRadianceSettings {
    pub fn validate(self) -> Result<()> {
        validate_frame_count("radiance history_frame_budget", self.history_frame_budget)?;
        validate_frame_count("radiance fast_history_budget", self.fast_history_budget)?;
        validate_nonnegative_finite("radiance spatial_radius", self.spatial_radius)?;
        self.variance.validate()?;
        self.history_rejection.validate()?;
        self.outlier_clamp.validate()
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SrdShadowSettings {
    pub stabilization_frame_budget: u32,
    pub plane_offset_tolerance: f32,
    pub sun_direction: [f32; 3],
}

impl Default for SrdShadowSettings {
    fn default() -> Self {
        Self {
            stabilization_frame_budget: 16,
            plane_offset_tolerance: 0.02,
            sun_direction: [0.0, -1.0, 0.0],
        }
    }
}

impl SrdShadowSettings {
    pub fn validate(self) -> Result<()> {
        validate_positive_finite("shadow plane_offset_tolerance", self.plane_offset_tolerance)?;
        if !self.sun_direction.iter().all(|v| v.is_finite()) {
            return Err(Error::InvalidInput(
                "SRD shadow sun_direction must contain finite components".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SrdOcclusionSettings {
    pub history_frame_budget: u32,
    pub spatial_radius: f32,
    pub normal_weight_power: f32,
}

impl Default for SrdOcclusionSettings {
    fn default() -> Self {
        Self {
            history_frame_budget: 32,
            spatial_radius: 2.0,
            normal_weight_power: 8.0,
        }
    }
}

impl SrdOcclusionSettings {
    pub fn validate(self) -> Result<()> {
        validate_frame_count("occlusion history_frame_budget", self.history_frame_budget)?;
        validate_nonnegative_finite("occlusion spatial_radius", self.spatial_radius)?;
        validate_positive_finite("occlusion normal_weight_power", self.normal_weight_power)
    }
}

pub(super) fn validate_nonzero_size(name: &str, size: UVec2) -> Result<()> {
    if size.x == 0 || size.y == 0 {
        return Err(Error::InvalidInput(format!(
            "SRD {name} must be non-zero in both dimensions, got {}x{}",
            size.x, size.y
        )));
    }
    Ok(())
}

pub(super) fn validate_positive_finite(name: &str, value: f32) -> Result<()> {
    if !value.is_finite() || value <= 0.0 {
        return Err(Error::InvalidInput(format!(
            "SRD {name} must be positive and finite, got {value}"
        )));
    }
    Ok(())
}

pub(super) fn validate_finite_vec2(name: &str, value: Vec2) -> Result<()> {
    if !value.x.is_finite() || !value.y.is_finite() {
        return Err(Error::InvalidInput(format!(
            "SRD {name} must be finite, got ({}, {})",
            value.x, value.y
        )));
    }
    Ok(())
}

pub(super) fn validate_nonnegative_finite(name: &str, value: f32) -> Result<()> {
    if !value.is_finite() || value < 0.0 {
        return Err(Error::InvalidInput(format!(
            "SRD {name} must be non-negative and finite, got {value}"
        )));
    }
    Ok(())
}

pub(super) fn validate_frame_count(name: &str, value: u32) -> Result<()> {
    if value == 0 {
        return Err(Error::InvalidInput(format!(
            "SRD {name} must be at least 1"
        )));
    }
    Ok(())
}
