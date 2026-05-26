use crate::Result;
use glam::UVec2;
use super::*;

/// All v0 Radiance Stabilizer pipelines and SRD-owned resources for one denoiser.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdRadianceStabilizerResources {
    pub clear_pipeline: usize,
    pub surface_mask: SrdRadianceSurfaceMaskResources,
    pub reproject: SrdRadianceReprojectResources,
    pub accumulate: SrdRadianceAccumulateResources,
    pub reconstruct: SrdRadianceReconstructResources,
    pub outlier_suppress: SrdRadianceOutlierSuppressResources,
}

/// The SRD-owned resource that contains the final v0 Radiance Stabilizer output.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SrdRadianceOutputResource {
    Reconstruct { scratch_index: u16 },
    OutlierSuppress { scratch_index: u16 },
}

/// Summary returned by `SrdInstance::plan_radiance_stabilizer_passes`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdRadianceStabilizerPlan {
    pub final_output: SrdRadianceOutputResource,
    pub dispatch_count: usize,
}

impl SrdInstance {
    /// Prepare all v0 Radiance Stabilizer resources and pipelines.
    pub fn prepare_radiance_stabilizer(
        &mut self,
        denoiser_id: SrdDenoiserId,
    ) -> Result<SrdRadianceStabilizerResources> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::RadianceStabilizer) {
            return Err(crate::Error::InvalidInput(format!(
                "SRD denoiser id {} is not a RadianceStabilizer denoiser",
                denoiser_id.get()
            )));
        }

        let clear_pipeline = self.register_clear_pipeline()?;
        let surface_mask = self.prepare_radiance_surface_mask(denoiser_id)?;
        let reproject = self.prepare_radiance_reproject(denoiser_id)?;
        let accumulate = self.prepare_radiance_accumulate(denoiser_id)?;
        let reconstruct = self.prepare_radiance_reconstruct(denoiser_id)?;
        let outlier_suppress = self.prepare_radiance_outlier_suppress(denoiser_id)?;

        Ok(SrdRadianceStabilizerResources {
            clear_pipeline,
            surface_mask,
            reproject,
            accumulate,
            reconstruct,
            outlier_suppress,
        })
    }

    /// Plan the v0 Radiance Stabilizer pass graph for one frame.
    ///
    /// Emitted order: optional clear, surface mask, reproject, accumulate,
    /// spatial reconstruct, optional outlier suppress.
    pub fn plan_radiance_stabilizer_passes(
        &mut self,
        denoiser_id: SrdDenoiserId,
        resources: SrdRadianceStabilizerResources,
        rect_size: UVec2,
    ) -> Result<SrdRadianceStabilizerPlan> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::RadianceStabilizer) {
            return Err(crate::Error::InvalidInput(format!(
                "SRD denoiser id {} is not a RadianceStabilizer denoiser",
                denoiser_id.get()
            )));
        }
        if resources.accumulate.history_ring_index >= self.history_rings().len() {
            return Err(crate::Error::InvalidInput(format!(
                "SRD radiance stabilizer references missing history ring {}",
                resources.accumulate.history_ring_index
            )));
        }
        let ring = self.history_rings()[resources.accumulate.history_ring_index].clone();
        if ring.denoiser_id != denoiser_id {
            return Err(crate::Error::InvalidInput(format!(
                "SRD radiance stabilizer ring belongs to denoiser id {} not {}",
                ring.denoiser_id.get(),
                denoiser_id.get()
            )));
        }

        self.clear_dispatches();
        if self.common_settings().history_mode == SrdHistoryMode::ZeroHistory {
            self.push_clear_dispatches_for(denoiser_id, resources.clear_pipeline)?;
        }
        self.rotate_history_ring_at(resources.accumulate.history_ring_index)?;

        self.plan_radiance_surface_mask_passes(denoiser_id, resources.surface_mask, rect_size)?;
        self.plan_radiance_reproject_passes(
            denoiser_id,
            resources.reproject,
            Some(resources.surface_mask),
            rect_size,
        )?;
        self.plan_radiance_accumulate_passes(
            denoiser_id,
            resources.accumulate,
            resources.reproject,
            Some(resources.surface_mask),
            rect_size,
        )?;
        self.plan_radiance_reconstruct_passes(
            denoiser_id,
            resources.reconstruct,
            resources.accumulate,
            Some(resources.surface_mask),
            rect_size,
        )?;

        let before_outlier = self.dispatches().len();
        self.plan_radiance_outlier_suppress_passes(
            denoiser_id,
            resources.outlier_suppress,
            resources.reconstruct,
            Some(resources.surface_mask),
            rect_size,
        )?;
        let outlier_enabled = self.dispatches().len() > before_outlier;
        let final_output = if outlier_enabled {
            SrdRadianceOutputResource::OutlierSuppress {
                scratch_index: resources.outlier_suppress.scratch_index,
            }
        } else {
            SrdRadianceOutputResource::Reconstruct {
                scratch_index: resources.reconstruct.scratch_index,
            }
        };

        Ok(SrdRadianceStabilizerPlan {
            final_output,
            dispatch_count: self.dispatches().len(),
        })
    }
}
