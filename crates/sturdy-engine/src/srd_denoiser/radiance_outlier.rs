use crate::{Error, Format, Result};
use glam::UVec2;
use super::*;

/// Push constants for the radiance Outlier Suppress pass.
///
/// Matches `SrdRadianceOutlierSuppressConstants` in
/// `shaders/srd_radiance_outlier_suppress.slang`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdRadianceOutlierSuppressConstants {
    pub rect_extent: [u32; 2],
    pub mask_extent: [u32; 2],
    pub luminance_sigma: f32,
    pub max_relative_luminance: f32,
    pub use_surface_mask: u32,
    pub tile_size: u32,
}

/// Per-instance handles returned by `SrdInstance::prepare_radiance_outlier_suppress`.
///
/// `scratch_index` names a full-resolution RGBA16F scratch texture that receives
/// the optional post-reconstruct clamp result. When the pass is disabled by
/// settings, planners leave this resource unused and downstream consumers should
/// read from the reconstruct output instead.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdRadianceOutlierSuppressResources {
    pub scratch_index: u16,
    pub pipeline: usize,
}

impl SrdInstance {
    pub fn register_radiance_outlier_suppress_pipeline(&mut self) -> Result<usize> {
        self.register_pipeline(SrdPipelineDesc {
            name: "SRD Radiance Outlier Suppress".into(),
            debug_label: "SRD Radiance Outlier Suppress".into(),
            shader_label: "srd_radiance_outlier_suppress".into(),
            has_constants: true,
            workgroup_size: [8, 8, 1],
        })
    }

    /// Allocate the optional post-reconstruct outlier-suppress output scratch and
    /// register the outlier-suppress pipeline for a Radiance Stabilizer denoiser.
    pub fn prepare_radiance_outlier_suppress(
        &mut self,
        denoiser_id: SrdDenoiserId,
    ) -> Result<SrdRadianceOutlierSuppressResources> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::RadianceStabilizer) {
            return Err(Error::InvalidInput(format!(
                "SRD denoiser id {} is not a RadianceStabilizer denoiser",
                denoiser_id.get()
            )));
        }
        let scratch_index = self.add_unique_scratch_texture(SrdTextureDesc {
            name: format!("radiance_outlier_suppress_{}", denoiser_id.get()),
            debug_label: format!("SRD Radiance Outlier Suppress {}", denoiser_id.get()),
            slot: SrdResourceSlot::ScratchPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::Scratch),
            downsample_factor: 1,
        })?;
        let pipeline = self.register_radiance_outlier_suppress_pipeline()?;
        Ok(SrdRadianceOutlierSuppressResources {
            scratch_index,
            pipeline,
        })
    }

    /// Emit the optional radiance Outlier Suppress dispatch.
    ///
    /// The pass is only appended when `SrdRadianceSettings::outlier_clamp.enabled`
    /// is true. Disabled settings return the existing dispatch list unchanged, so
    /// callers can keep a stable planning sequence while selecting the reconstruct
    /// output as the final radiance when no clamp dispatch is emitted.
    pub fn plan_radiance_outlier_suppress_passes(
        &mut self,
        denoiser_id: SrdDenoiserId,
        resources: SrdRadianceOutlierSuppressResources,
        reconstruct: SrdRadianceReconstructResources,
        surface_mask: Option<SrdRadianceSurfaceMaskResources>,
        rect_size: UVec2,
    ) -> Result<&[SrdDispatchDesc]> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::RadianceStabilizer) {
            return Err(Error::InvalidInput(format!(
                "SRD denoiser id {} is not a RadianceStabilizer denoiser",
                denoiser_id.get()
            )));
        }
        let radiance_settings = match self.denoiser_settings(denoiser_id) {
            Some(SrdFamilySettings::Radiance(settings)) => *settings,
            _ => SrdRadianceSettings::default(),
        };
        let outlier = radiance_settings.outlier_clamp;
        if !outlier.enabled {
            return Ok(self.dispatches());
        }
        if resources.pipeline >= self.pipelines().len() {
            return Err(Error::InvalidInput(format!(
                "SRD radiance outlier suppress references missing pipeline index {}",
                resources.pipeline
            )));
        }
        if (resources.scratch_index as usize) >= self.scratch_pool().len() {
            return Err(Error::InvalidInput(format!(
                "SRD radiance outlier suppress references missing scratch index {}",
                resources.scratch_index
            )));
        }
        if (reconstruct.scratch_index as usize) >= self.scratch_pool().len() {
            return Err(Error::InvalidInput(format!(
                "SRD radiance outlier suppress references missing reconstruct scratch {}",
                reconstruct.scratch_index
            )));
        }

        let tile = SRD_RADIANCE_SURFACE_MASK_TILE_SIZE;
        let mask_extent = UVec2::new(
            rect_size.x.div_ceil(tile).max(1),
            rect_size.y.div_ceil(tile).max(1),
        );
        let constants = SrdRadianceOutlierSuppressConstants {
            rect_extent: [rect_size.x.max(1), rect_size.y.max(1)],
            mask_extent: [mask_extent.x, mask_extent.y],
            luminance_sigma: outlier.luminance_sigma,
            max_relative_luminance: outlier.max_relative_luminance,
            use_surface_mask: surface_mask.is_some() as u32,
            tile_size: tile,
        };
        let constants_range = self.push_typed_constants(&constants);
        let mut builder = SrdPassBuilder::new(
            "SRD Radiance Outlier Suppress",
            denoiser_id,
            resources.pipeline,
        )
        .read_pool(SrdPoolClass::Scratch, reconstruct.scratch_index);
        if let Some(mask) = surface_mask {
            if (mask.scratch_index as usize) >= self.scratch_pool().len() {
                return Err(Error::InvalidInput(format!(
                    "SRD radiance outlier suppress references missing surface-mask scratch {}",
                    mask.scratch_index
                )));
            }
            builder = builder.read_pool(SrdPoolClass::Scratch, mask.scratch_index);
        }
        let dispatch = builder
            .write_pool(SrdPoolClass::Scratch, resources.scratch_index)
            .constants_range(constants_range)
            .grid_size([
                rect_size.x.div_ceil(8).max(1),
                rect_size.y.div_ceil(8).max(1),
                1,
            ])
            .build()?;
        self.push_dispatch(dispatch)?;
        Ok(self.dispatches())
    }
}
