use crate::{Error, Result};
use super::resources::{SrdDenoiserId, SrdDescriptorType, SrdPoolClass, SrdResourceDesc, SrdResourceSlot};
use glam::UVec2;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdConstantRange {
    pub offset: usize,
    pub size: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SrdConstantArena {
    bytes: Vec<u8>,
}

impl SrdConstantArena {
    pub fn push(&mut self, bytes: &[u8]) -> SrdConstantRange {
        let offset = self.bytes.len();
        self.bytes.extend_from_slice(bytes);
        SrdConstantRange {
            offset,
            size: bytes.len(),
        }
    }

    pub fn get(&self, range: SrdConstantRange) -> &[u8] {
        &self.bytes[range.offset..range.offset + range.size]
    }

    pub fn clear(&mut self) {
        self.bytes.clear();
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SrdDispatchDesc {
    pub name: String,
    pub denoiser_id: SrdDenoiserId,
    pub pipeline_index: usize,
    pub resources: Vec<SrdResourceDesc>,
    pub constants_size: usize,
    pub constants_range: Option<SrdConstantRange>,
    /// Set when this dispatch's constant bytes are bit-identical to the
    /// immediately preceding dispatch. Allows executors to skip re-upload.
    pub reuses_previous_constants: bool,
    pub grid_size: [u32; 3],
}

impl SrdDispatchDesc {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(Error::InvalidInput(
                "SRD dispatch name must not be empty".into(),
            ));
        }
        if self.grid_size.iter().any(|v| *v == 0) {
            return Err(Error::InvalidInput(format!(
                "SRD dispatch '{}' grid_size must be non-zero in all dimensions",
                self.name
            )));
        }
        Ok(())
    }
}

pub struct SrdPassBuilder {
    dispatch: SrdDispatchDesc,
}

impl SrdPassBuilder {
    pub fn new(name: impl Into<String>, denoiser_id: SrdDenoiserId, pipeline_index: usize) -> Self {
        Self {
            dispatch: SrdDispatchDesc {
                name: name.into(),
                denoiser_id,
                pipeline_index,
                resources: Vec::new(),
                constants_size: 0,
                constants_range: None,
                reuses_previous_constants: false,
                grid_size: [1, 1, 1],
            },
        }
    }

    pub fn read_slot(mut self, slot: SrdResourceSlot, pool_index: Option<u16>) -> Self {
        self.dispatch.resources.push(SrdResourceDesc {
            descriptor_type: SrdDescriptorType::Texture,
            slot,
            pool_index,
        });
        self
    }

    pub fn read(mut self, slot: SrdResourceSlot) -> Self {
        self.dispatch.resources.push(SrdResourceDesc {
            descriptor_type: SrdDescriptorType::Texture,
            slot,
            pool_index: None,
        });
        self
    }

    pub fn write(mut self, slot: SrdResourceSlot) -> Self {
        self.dispatch.resources.push(SrdResourceDesc {
            descriptor_type: SrdDescriptorType::StorageTexture,
            slot,
            pool_index: None,
        });
        self
    }

    pub fn read_pool(mut self, pool: SrdPoolClass, index: u16) -> Self {
        self.dispatch.resources.push(SrdResourceDesc {
            descriptor_type: SrdDescriptorType::Texture,
            slot: pool.resource_slot(),
            pool_index: Some(index),
        });
        self
    }

    pub fn write_pool(mut self, pool: SrdPoolClass, index: u16) -> Self {
        self.dispatch.resources.push(SrdResourceDesc {
            descriptor_type: SrdDescriptorType::StorageTexture,
            slot: pool.resource_slot(),
            pool_index: Some(index),
        });
        self
    }

    pub fn constants_size(mut self, constants_size: usize) -> Self {
        self.dispatch.constants_size = constants_size;
        self
    }

    pub fn constants_range(mut self, constants_range: SrdConstantRange) -> Self {
        self.dispatch.constants_size = constants_range.size;
        self.dispatch.constants_range = Some(constants_range);
        self
    }

    pub fn reuses_previous_constants(mut self, reuses_previous: bool) -> Self {
        self.dispatch.reuses_previous_constants = reuses_previous;
        self
    }

    pub fn grid_size(mut self, grid_size: [u32; 3]) -> Self {
        self.dispatch.grid_size = grid_size;
        self
    }

    pub fn build(self) -> Result<SrdDispatchDesc> {
        self.dispatch.validate()?;
        Ok(self.dispatch)
    }
}

pub(super) fn reference_grid_size(rect_size: UVec2) -> [u32; 3] {
    [
        rect_size.x.div_ceil(8).max(1),
        rect_size.y.div_ceil(8).max(1),
        1,
    ]
}

// --- Constant structs for GPU passes ---

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdSignalMomentsConstants {
    pub frame_index: u32,
    pub history_length: u32,
    pub variance_window_radius: f32,
    pub _pad: u32,
}

/// Push constants for the radiance Surface Mask pass.
///
/// Matches `SrdRadianceSurfaceMaskConstants` in `shaders/srd_radiance_surface_mask.slang`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdRadianceSurfaceMaskConstants {
    pub input_extent: [u32; 2],
    pub mask_extent: [u32; 2],
    pub effective_range: f32,
    pub tile_size: u32,
    pub _pad: [u32; 2],
}

/// Tile size in pixels used by the radiance Surface Mask pass.
pub const SRD_RADIANCE_SURFACE_MASK_TILE_SIZE: u32 = 8;

/// Push constants for the radiance Spatial Reconstruct pass.
///
/// Matches `SrdRadianceReconstructConstants` in `shaders/srd_radiance_reconstruct.slang`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdRadianceReconstructConstants {
    pub rect_extent: [u32; 2],
    pub mask_extent: [u32; 2],
    pub short_history_cutoff: f32,
    pub depth_tolerance_relative: f32,
    pub depth_tolerance_absolute: f32,
    pub normal_sharpness: f32,
    pub roughness_tolerance: f32,
    pub use_surface_mask: u32,
    pub tile_size: u32,
    pub _pad: u32,
}

/// Push constants for the radiance Accumulate pass.
///
/// Matches `SrdRadianceAccumulateConstants` in `shaders/srd_radiance_accumulate.slang`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdRadianceAccumulateConstants {
    pub rect_extent: [u32; 2],
    pub mask_extent: [u32; 2],
    pub fast_history_budget: f32,
    pub history_frame_budget: f32,
    pub use_surface_mask: u32,
    pub tile_size: u32,
}

/// Push constants for the radiance History Clamp pass.
///
/// Matches `SrdRadianceClampConstants` in `shaders/srd_radiance_clamp.slang`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdRadianceClampConstants {
    pub rect_extent: [u32; 2],
    pub mask_extent: [u32; 2],
    pub sigma_scale: f32,
    pub use_surface_mask: u32,
    pub tile_size: u32,
    pub _pad: u32,
}

/// Push constants for one iteration of the radiance À-Trous wavelet filter.
///
/// Matches `SrdRadianceAtrousConstants` in `shaders/srd_radiance_atrous.slang`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdRadianceAtrousConstants {
    pub rect_extent: [u32; 2],
    pub mask_extent: [u32; 2],
    pub depth_tolerance_relative: f32,
    pub depth_tolerance_absolute: f32,
    pub normal_sharpness: f32,
    pub roughness_tolerance: f32,
    pub luminance_sigma: f32,
    pub stride: u32,
    pub use_surface_mask: u32,
    pub tile_size: u32,
    pub _pad0: u32,
    pub _pad1: u32,
}

/// Push constants for the radiance Spatial Filter pass.
///
/// Matches `SrdRadianceSpatialFilterConstants` in
/// `shaders/srd_radiance_spatial_filter.slang`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdRadianceSpatialFilterConstants {
    pub rect_extent: [u32; 2],
    pub mask_extent: [u32; 2],
    pub depth_tolerance_relative: f32,
    pub depth_tolerance_absolute: f32,
    pub normal_sharpness: f32,
    pub roughness_tolerance: f32,
    pub luminance_sigma: f32,
    pub stride: u32,
    pub use_surface_mask: u32,
    pub tile_size: u32,
    pub _pad0: u32,
    pub _pad1: u32,
}

/// Push constants for the radiance Post-Blur pass.
///
/// Matches `SrdRadiancePostBlurConstants` in `shaders/srd_radiance_post_blur.slang`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdRadiancePostBlurConstants {
    pub rect_extent: [u32; 2],
    pub mask_extent: [u32; 2],
    pub depth_tolerance_relative: f32,
    pub depth_tolerance_absolute: f32,
    pub normal_sharpness: f32,
    pub roughness_tolerance: f32,
    pub max_blur_sigma: f32,
    pub blur_radius: u32,
    pub use_surface_mask: u32,
    pub tile_size: u32,
}

/// Push constants for the shadow Accumulate pass.
///
/// Matches `SrdShadowAccumulateConstants` in `shaders/srd_shadow_accumulate.slang`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdShadowAccumulateConstants {
    pub rect_extent: [u32; 2],
    pub mask_extent: [u32; 2],
    pub history_frame_budget: f32,
    pub plane_offset_tolerance: f32,
    pub use_surface_mask: u32,
    pub tile_size: u32,
}

/// Push constants for the shadow spatial Filter pass.
///
/// Matches `SrdShadowFilterConstants` in `shaders/srd_shadow_filter.slang`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdShadowFilterConstants {
    pub rect_extent: [u32; 2],
    pub mask_extent: [u32; 2],
    pub normal_weight_power: f32,
    pub spatial_radius: f32,
    pub use_surface_mask: u32,
    pub tile_size: u32,
}

/// Push constants for the occlusion Accumulate pass.
///
/// Matches `SrdOcclusionAccumulateConstants` in `shaders/srd_occlusion_accumulate.slang`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdOcclusionAccumulateConstants {
    pub rect_extent: [u32; 2],
    pub mask_extent: [u32; 2],
    pub history_frame_budget: f32,
    pub use_surface_mask: u32,
    pub tile_size: u32,
    pub _pad: u32,
}

/// Push constants for the occlusion spatial Filter pass.
///
/// Matches `SrdOcclusionFilterConstants` in `shaders/srd_occlusion_filter.slang`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdOcclusionFilterConstants {
    pub rect_extent: [u32; 2],
    pub mask_extent: [u32; 2],
    pub normal_weight_power: f32,
    pub spatial_radius: f32,
    pub use_surface_mask: u32,
    pub tile_size: u32,
}

/// Push constants for the spectral radiance Accumulate pass.
///
/// `channel_count` carries the number of active spectral channels (1–3) so the
/// shader can zero unused RGB components in the history blend.
///
/// Matches `SrdSpectralAccumulateConstants` in `shaders/srd_spectral_accumulate.slang`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdSpectralAccumulateConstants {
    pub rect_extent: [u32; 2],
    pub mask_extent: [u32; 2],
    pub history_frame_budget: f32,
    pub channel_count: u32,
    pub use_surface_mask: u32,
    pub tile_size: u32,
}

/// Push constants for the spectral radiance Filter pass.
///
/// Matches `SrdSpectralFilterConstants` in `shaders/srd_spectral_filter.slang`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdSpectralFilterConstants {
    pub rect_extent: [u32; 2],
    pub mask_extent: [u32; 2],
    pub normal_weight_power: f32,
    pub spatial_radius: f32,
    pub use_surface_mask: u32,
    pub tile_size: u32,
}

/// Push constants for the translucent-shadow Accumulate pass.
///
/// Matches `SrdTranslucentShadowAccumulateConstants` in
/// `shaders/srd_translucency_accumulate.slang`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdTranslucentShadowAccumulateConstants {
    pub rect_extent: [u32; 2],
    pub mask_extent: [u32; 2],
    pub history_frame_budget: f32,
    pub use_surface_mask: u32,
    pub tile_size: u32,
    pub _pad: u32,
}

/// Push constants for the translucent-shadow spatial Filter pass.
///
/// Matches `SrdTranslucentShadowFilterConstants` in `shaders/srd_translucency_filter.slang`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdTranslucentShadowFilterConstants {
    pub rect_extent: [u32; 2],
    pub mask_extent: [u32; 2],
    pub normal_weight_power: f32,
    pub spatial_radius: f32,
    pub use_surface_mask: u32,
    pub tile_size: u32,
}

/// Push constants for the radiance Reproject pass.
///
/// Matches `SrdRadianceReprojectConstants` in `shaders/srd_radiance_reproject.slang`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdRadianceReprojectConstants {
    pub rect_extent: [u32; 2],
    pub mask_extent: [u32; 2],
    pub motion_scale: [f32; 2],
    pub depth_tolerance_relative: f32,
    pub depth_tolerance_absolute: f32,
    pub normal_cos_threshold: f32,
    pub use_surface_mask: u32,
    pub tile_size: u32,
    /// Non-zero when the shader should read and validate hit-distance textures.
    pub use_hit_distance: u32,
    /// Maximum tolerated relative hit-distance change before validity is downgraded.
    pub hit_distance_relative_threshold: f32,
}
