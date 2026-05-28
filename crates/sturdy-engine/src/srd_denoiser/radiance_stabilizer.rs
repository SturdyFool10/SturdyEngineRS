use crate::{Error, Format, Result};
use glam::UVec2;
use super::*;

/// All v0 Radiance Stabilizer pipelines and SRD-owned resources for one denoiser.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdRadianceStabilizerResources {
    pub clear_pipeline: usize,
    pub surface_mask: SrdRadianceSurfaceMaskResources,
    pub reproject: SrdRadianceReprojectResources,
    pub accumulate: SrdRadianceAccumulateResources,
    pub clamp: SrdRadianceClampResources,
    pub reconstruct: SrdRadianceReconstructResources,
    pub outlier_suppress: SrdRadianceOutlierSuppressResources,
    pub spatial_filter: SrdRadianceSpatialFilterResources,
    pub atrous: SrdRadianceAtrousResources,
    pub post_blur: SrdRadiancePostBlurResources,
}

/// The SRD-owned resource that contains the final v0 Radiance Stabilizer output.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SrdRadianceOutputResource {
    Reconstruct { scratch_index: u16 },
    OutlierSuppress { scratch_index: u16 },
    SpatialFilter { scratch_index: u16 },
    Atrous { scratch_index: u16 },
    PostBlur { scratch_index: u16 },
}

impl SrdRadianceOutputResource {
    pub fn scratch_index(self) -> u16 {
        match self {
            Self::Reconstruct { scratch_index }
            | Self::OutlierSuppress { scratch_index }
            | Self::SpatialFilter { scratch_index }
            | Self::Atrous { scratch_index }
            | Self::PostBlur { scratch_index } => scratch_index,
        }
    }
}

/// Summary returned by `SrdInstance::plan_radiance_stabilizer_passes`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdRadianceStabilizerPlan {
    pub final_output: SrdRadianceOutputResource,
    pub dispatch_count: usize,
}

/// Summary returned by `SrdInstance::plan_radiance_combined_passes`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdRadianceCombinedPlan {
    pub final_output: SrdRadianceOutputResource,
    pub dispatch_count: usize,
}

/// Summary returned by `SrdInstance::plan_radiance_diffuse_specular_passes`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdRadianceDiffuseSpecularPlan {
    pub diffuse_output: SrdRadianceOutputResource,
    pub specular_output: SrdRadianceOutputResource,
    pub dispatch_count: usize,
}

impl SrdInstance {
    /// Prepare the combined fast-path resources.
    ///
    /// Allocates the five core passes (surface mask, reproject, accumulate, clamp,
    /// reconstruct) without optional tail passes. Use this when you have a single
    /// combined radiance signal and do not need separate diffuse/specular handling.
    pub fn prepare_radiance_combined(
        &mut self,
        denoiser_id: SrdDenoiserId,
    ) -> Result<SrdRadianceCombinedResources> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::RadianceStabilizer) {
            return Err(Error::InvalidInput(format!(
                "SRD denoiser id {} is not a RadianceStabilizer denoiser",
                denoiser_id.get()
            )));
        }
        let clear_pipeline = self.register_clear_pipeline()?;
        let surface_mask = self.prepare_radiance_surface_mask(denoiser_id)?;
        let reproject = self.prepare_radiance_reproject(denoiser_id)?;
        let accumulate = self.prepare_radiance_accumulate(denoiser_id)?;
        let clamp = self.prepare_radiance_clamp(denoiser_id)?;
        let reconstruct = self.prepare_radiance_reconstruct(denoiser_id)?;
        Ok(SrdRadianceCombinedResources {
            clear_pipeline,
            surface_mask,
            reproject,
            accumulate,
            clamp,
            reconstruct,
        })
    }

    /// Plan one frame of combined radiance reconstruction (fast path).
    ///
    /// Always runs all five core passes. For optional tail passes (outlier
    /// suppress, spatial filter, atrous, post-blur) use the full stabilizer API.
    pub fn plan_radiance_combined_passes(
        &mut self,
        denoiser_id: SrdDenoiserId,
        resources: SrdRadianceCombinedResources,
        rect_size: UVec2,
    ) -> Result<SrdRadianceCombinedPlan> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::RadianceStabilizer) {
            return Err(Error::InvalidInput(format!(
                "SRD denoiser id {} is not a RadianceStabilizer denoiser",
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
        self.push_radiance_accumulate_dispatch(
            denoiser_id,
            resources.accumulate,
            resources.reproject,
            Some(resources.surface_mask),
            rect_size,
            SrdResourceSlot::CombinedRadianceInput,
            "SRD Radiance Accumulate",
        )?;
        self.push_radiance_clamp_dispatch(
            denoiser_id,
            resources.clamp,
            resources.accumulate,
            Some(resources.surface_mask),
            rect_size,
            SrdResourceSlot::CombinedRadianceInput,
            "SRD Radiance Clamp",
        )?;
        self.plan_radiance_reconstruct_passes(
            denoiser_id,
            resources.reconstruct,
            resources.accumulate,
            Some(resources.clamp),
            Some(resources.surface_mask),
            rect_size,
        )?;

        Ok(SrdRadianceCombinedPlan {
            final_output: SrdRadianceOutputResource::Reconstruct {
                scratch_index: resources.reconstruct.scratch_index,
            },
            dispatch_count: self.dispatches().len(),
        })
    }

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
        let clamp = self.prepare_radiance_clamp(denoiser_id)?;
        let reconstruct = self.prepare_radiance_reconstruct(denoiser_id)?;
        let outlier_suppress = self.prepare_radiance_outlier_suppress(denoiser_id)?;
        let spatial_filter = self.prepare_radiance_spatial_filter(denoiser_id)?;
        let atrous = self.prepare_radiance_atrous(denoiser_id)?;
        let post_blur = self.prepare_radiance_post_blur(denoiser_id)?;

        Ok(SrdRadianceStabilizerResources {
            clear_pipeline,
            surface_mask,
            reproject,
            accumulate,
            clamp,
            reconstruct,
            outlier_suppress,
            spatial_filter,
            atrous,
            post_blur,
        })
    }

    /// Plan the v0 Radiance Stabilizer pass graph for one frame.
    ///
    /// Emitted order: optional clear, surface mask, reproject, accumulate,
    /// clamp, reconstruct, optional outlier suppress, optional spatial filter,
    /// optional atrous, optional post blur.
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

        let before_clamp = self.dispatches().len();
        self.plan_radiance_clamp_passes(
            denoiser_id,
            resources.clamp,
            resources.accumulate,
            Some(resources.surface_mask),
            rect_size,
        )?;
        let clamp_active = self.dispatches().len() > before_clamp;

        self.plan_radiance_reconstruct_passes(
            denoiser_id,
            resources.reconstruct,
            resources.accumulate,
            if clamp_active { Some(resources.clamp) } else { None },
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
        let mut final_output = if outlier_enabled {
            SrdRadianceOutputResource::OutlierSuppress {
                scratch_index: resources.outlier_suppress.scratch_index,
            }
        } else {
            SrdRadianceOutputResource::Reconstruct {
                scratch_index: resources.reconstruct.scratch_index,
            }
        };

        let before_filter = self.dispatches().len();
        self.plan_radiance_spatial_filter_passes(
            denoiser_id,
            resources.spatial_filter,
            final_output.scratch_index(),
            Some(resources.surface_mask),
            rect_size,
        )?;
        if self.dispatches().len() > before_filter {
            final_output = SrdRadianceOutputResource::SpatialFilter {
                scratch_index: resources.spatial_filter.scratch_index,
            };
        }

        let before_atrous = self.dispatches().len();
        let atrous_final = self.plan_radiance_atrous_passes(
            denoiser_id,
            resources.atrous,
            final_output.scratch_index(),
            Some(resources.surface_mask),
            rect_size,
        )?;
        if self.dispatches().len() > before_atrous {
            final_output = SrdRadianceOutputResource::Atrous {
                scratch_index: atrous_final,
            };
        }

        let before_post_blur = self.dispatches().len();
        self.plan_radiance_post_blur_passes(
            denoiser_id,
            resources.post_blur,
            final_output.scratch_index(),
            Some(resources.surface_mask),
            rect_size,
        )?;
        if self.dispatches().len() > before_post_blur {
            final_output = SrdRadianceOutputResource::PostBlur {
                scratch_index: resources.post_blur.scratch_index,
            };
        }

        Ok(SrdRadianceStabilizerPlan {
            final_output,
            dispatch_count: self.dispatches().len(),
        })
    }

    /// Prepare independent diffuse and specular accumulation resources.
    ///
    /// The two channels share the surface-mask and reproject pipelines but each
    /// gets its own history ring, clamp scratch, and reconstruct scratch.
    pub fn prepare_radiance_diffuse_specular(
        &mut self,
        denoiser_id: SrdDenoiserId,
    ) -> Result<SrdRadianceDiffuseSpecularResources> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::RadianceStabilizer) {
            return Err(Error::InvalidInput(format!(
                "SRD denoiser id {} is not a RadianceStabilizer denoiser",
                denoiser_id.get()
            )));
        }

        let clear_pipeline = self.register_clear_pipeline()?;
        let surface_mask = self.prepare_radiance_surface_mask(denoiser_id)?;
        let reproject = self.prepare_radiance_reproject(denoiser_id)?;

        let accumulate_pipeline = self.register_radiance_accumulate_pipeline()?;
        let clamp_pipeline = self.register_radiance_clamp_pipeline()?;
        let reconstruct_pipeline = self.register_radiance_reconstruct_pipeline()?;

        let id = denoiser_id.get();

        // Diffuse history ring
        let diffuse_write = self.add_history_texture(SrdTextureDesc {
            name: format!("radiance_diffuse_history_{id}_write"),
            debug_label: format!("SRD Radiance Diffuse History {id} Write"),
            slot: SrdResourceSlot::HistoryPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::History),
            downsample_factor: 1,
        })?;
        let diffuse_read = self.add_history_texture(SrdTextureDesc {
            name: format!("radiance_diffuse_history_{id}_read"),
            debug_label: format!("SRD Radiance Diffuse History {id} Read"),
            slot: SrdResourceSlot::HistoryPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::History),
            downsample_factor: 1,
        })?;
        self.add_history_ring(SrdHistoryRing {
            denoiser_id,
            write_index: diffuse_write,
            read_index: diffuse_read,
            label: format!("radiance_diffuse_history_{id}"),
        })?;
        let diffuse_ring_index = self.history_rings().len() - 1;

        // Specular history ring
        let specular_write = self.add_history_texture(SrdTextureDesc {
            name: format!("radiance_specular_history_{id}_write"),
            debug_label: format!("SRD Radiance Specular History {id} Write"),
            slot: SrdResourceSlot::HistoryPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::History),
            downsample_factor: 1,
        })?;
        let specular_read = self.add_history_texture(SrdTextureDesc {
            name: format!("radiance_specular_history_{id}_read"),
            debug_label: format!("SRD Radiance Specular History {id} Read"),
            slot: SrdResourceSlot::HistoryPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::History),
            downsample_factor: 1,
        })?;
        self.add_history_ring(SrdHistoryRing {
            denoiser_id,
            write_index: specular_write,
            read_index: specular_read,
            label: format!("radiance_specular_history_{id}"),
        })?;
        let specular_ring_index = self.history_rings().len() - 1;

        let diffuse_clamp_scratch = self.add_unique_scratch_texture(SrdTextureDesc {
            name: format!("radiance_diffuse_clamp_{id}"),
            debug_label: format!("SRD Radiance Diffuse Clamp {id}"),
            slot: SrdResourceSlot::ClampedRadianceInput,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::Scratch),
            downsample_factor: 1,
        })?;
        let specular_clamp_scratch = self.add_unique_scratch_texture(SrdTextureDesc {
            name: format!("radiance_specular_clamp_{id}"),
            debug_label: format!("SRD Radiance Specular Clamp {id}"),
            slot: SrdResourceSlot::ClampedRadianceInput,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::Scratch),
            downsample_factor: 1,
        })?;

        let diffuse_reconstruct_scratch = self.add_unique_scratch_texture(SrdTextureDesc {
            name: format!("radiance_diffuse_reconstruct_{id}"),
            debug_label: format!("SRD Radiance Diffuse Reconstruct {id}"),
            slot: SrdResourceSlot::ScratchPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::Scratch),
            downsample_factor: 1,
        })?;
        let specular_reconstruct_scratch = self.add_unique_scratch_texture(SrdTextureDesc {
            name: format!("radiance_specular_reconstruct_{id}"),
            debug_label: format!("SRD Radiance Specular Reconstruct {id}"),
            slot: SrdResourceSlot::ScratchPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::Scratch),
            downsample_factor: 1,
        })?;

        Ok(SrdRadianceDiffuseSpecularResources {
            clear_pipeline,
            surface_mask,
            reproject,
            diffuse_accumulate: SrdRadianceAccumulateResources {
                history_ring_index: diffuse_ring_index,
                pipeline: accumulate_pipeline,
            },
            diffuse_clamp: SrdRadianceClampResources {
                scratch_index: diffuse_clamp_scratch,
                pipeline: clamp_pipeline,
            },
            diffuse_reconstruct: SrdRadianceReconstructResources {
                scratch_index: diffuse_reconstruct_scratch,
                pipeline: reconstruct_pipeline,
            },
            specular_accumulate: SrdRadianceAccumulateResources {
                history_ring_index: specular_ring_index,
                pipeline: accumulate_pipeline,
            },
            specular_clamp: SrdRadianceClampResources {
                scratch_index: specular_clamp_scratch,
                pipeline: clamp_pipeline,
            },
            specular_reconstruct: SrdRadianceReconstructResources {
                scratch_index: specular_reconstruct_scratch,
                pipeline: reconstruct_pipeline,
            },
        })
    }

    /// Plan one frame of separate diffuse + specular radiance reconstruction.
    ///
    /// Emitted order: optional clears, surface mask, reproject, then per-channel:
    /// diffuse accumulate → diffuse clamp → diffuse reconstruct,
    /// specular accumulate → specular clamp → specular reconstruct.
    /// Both history rings are rotated at the start.
    pub fn plan_radiance_diffuse_specular_passes(
        &mut self,
        denoiser_id: SrdDenoiserId,
        resources: SrdRadianceDiffuseSpecularResources,
        rect_size: UVec2,
    ) -> Result<SrdRadianceDiffuseSpecularPlan> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::RadianceStabilizer) {
            return Err(Error::InvalidInput(format!(
                "SRD denoiser id {} is not a RadianceStabilizer denoiser",
                denoiser_id.get()
            )));
        }

        self.clear_dispatches();
        if self.common_settings().history_mode == SrdHistoryMode::ZeroHistory {
            self.push_clear_dispatches_for(denoiser_id, resources.clear_pipeline)?;
        }
        self.rotate_history_ring_at(resources.diffuse_accumulate.history_ring_index)?;
        self.rotate_history_ring_at(resources.specular_accumulate.history_ring_index)?;

        self.plan_radiance_surface_mask_passes(denoiser_id, resources.surface_mask, rect_size)?;
        self.plan_radiance_reproject_passes(
            denoiser_id,
            resources.reproject,
            Some(resources.surface_mask),
            rect_size,
        )?;

        // Diffuse chain
        self.push_radiance_accumulate_dispatch(
            denoiser_id,
            resources.diffuse_accumulate,
            resources.reproject,
            Some(resources.surface_mask),
            rect_size,
            SrdResourceSlot::DiffuseRadianceInput,
            "SRD Radiance Diffuse Accumulate",
        )?;
        self.push_radiance_clamp_dispatch(
            denoiser_id,
            resources.diffuse_clamp,
            resources.diffuse_accumulate,
            Some(resources.surface_mask),
            rect_size,
            SrdResourceSlot::DiffuseRadianceInput,
            "SRD Radiance Diffuse Clamp",
        )?;
        self.plan_radiance_reconstruct_passes(
            denoiser_id,
            resources.diffuse_reconstruct,
            resources.diffuse_accumulate,
            Some(resources.diffuse_clamp),
            Some(resources.surface_mask),
            rect_size,
        )?;

        // Specular chain
        self.push_radiance_accumulate_dispatch(
            denoiser_id,
            resources.specular_accumulate,
            resources.reproject,
            Some(resources.surface_mask),
            rect_size,
            SrdResourceSlot::SpecularRadianceInput,
            "SRD Radiance Specular Accumulate",
        )?;
        self.push_radiance_clamp_dispatch(
            denoiser_id,
            resources.specular_clamp,
            resources.specular_accumulate,
            Some(resources.surface_mask),
            rect_size,
            SrdResourceSlot::SpecularRadianceInput,
            "SRD Radiance Specular Clamp",
        )?;
        self.plan_radiance_reconstruct_passes(
            denoiser_id,
            resources.specular_reconstruct,
            resources.specular_accumulate,
            Some(resources.specular_clamp),
            Some(resources.surface_mask),
            rect_size,
        )?;

        Ok(SrdRadianceDiffuseSpecularPlan {
            diffuse_output: SrdRadianceOutputResource::Reconstruct {
                scratch_index: resources.diffuse_reconstruct.scratch_index,
            },
            specular_output: SrdRadianceOutputResource::Reconstruct {
                scratch_index: resources.specular_reconstruct.scratch_index,
            },
            dispatch_count: self.dispatches().len(),
        })
    }
}
