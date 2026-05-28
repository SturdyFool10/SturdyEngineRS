use crate::{Error, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SrdPipelineDesc {
    pub name: String,
    pub debug_label: String,
    pub shader_label: String,
    pub has_constants: bool,
    pub workgroup_size: [u32; 3],
}

impl SrdPipelineDesc {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(Error::InvalidInput(
                "SRD pipeline name must not be empty".into(),
            ));
        }
        if self.debug_label.trim().is_empty() {
            return Err(Error::InvalidInput(format!(
                "SRD pipeline '{}' debug_label must not be empty",
                self.name
            )));
        }
        if self.shader_label.trim().is_empty() {
            return Err(Error::InvalidInput(format!(
                "SRD pipeline '{}' shader_label must not be empty",
                self.name
            )));
        }
        if self.workgroup_size.iter().any(|v| *v == 0) {
            return Err(Error::InvalidInput(format!(
                "SRD pipeline '{}' workgroup_size must be non-zero in all dimensions",
                self.name
            )));
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdReferenceTemporalPipelines {
    pub temporal: usize,
    pub clear: usize,
}

/// Per-instance handles returned by `SrdInstance::prepare_radiance_surface_mask`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdRadianceSurfaceMaskResources {
    pub scratch_index: u16,
    pub pipeline: usize,
}

/// Per-instance handles returned by `SrdInstance::prepare_radiance_reproject`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdRadianceReprojectResources {
    pub scratch_index: u16,
    pub pipeline: usize,
}

/// Per-instance handles returned by `SrdInstance::prepare_radiance_accumulate`.
///
/// `history_ring_index` indexes into `SrdInstance::history_rings()`. The host
/// rotates this ring at the start of each frame's radiance plan; the
/// accumulate pass reads from `read_index` and writes to `write_index`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdRadianceAccumulateResources {
    pub history_ring_index: usize,
    pub pipeline: usize,
}

/// Per-instance handles returned by `SrdInstance::prepare_radiance_atrous`.
///
/// `ping_index` and `pong_index` name two full-resolution RGBA16F scratch
/// textures the À-Trous wavelet filter alternates between across iterations.
/// The final filtered radiance lives in whichever scratch was written by the
/// last iteration — callers receive that index from
/// `plan_radiance_atrous_passes` so the next stage can read it.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdRadianceAtrousResources {
    pub ping_index: u16,
    pub pong_index: u16,
    pub pipeline: usize,
}

/// Per-instance handles returned by `SrdInstance::prepare_radiance_spatial_filter`.
///
/// `scratch_index` names a full-resolution RGBA16F scratch texture that holds
/// the bilateral-filtered final radiance.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdRadianceSpatialFilterResources {
    pub scratch_index: u16,
    pub pipeline: usize,
}

/// Per-instance handles returned by `SrdInstance::prepare_radiance_clamp`.
///
/// `scratch_index` names a full-resolution RGBA16F scratch texture that holds
/// the de-ghosted accumulated radiance fed into the spatial reconstruct.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdRadianceClampResources {
    pub scratch_index: u16,
    pub pipeline: usize,
}

/// Per-instance handles returned by `SrdInstance::prepare_radiance_reconstruct`.
///
/// `scratch_index` names a full-resolution RGBA16F scratch texture that holds
/// the reconstructed radiance (RGB) plus a copy of the history length (alpha)
/// for downstream consumers (outlier suppress, luminance stabilize).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdRadianceReconstructResources {
    pub scratch_index: u16,
    pub pipeline: usize,
}

/// Per-instance handles returned by `SrdInstance::prepare_radiance_post_blur`.
///
/// `scratch_index` names a full-resolution RGBA16F scratch texture that holds
/// the final post-blurred radiance output.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdRadiancePostBlurResources {
    pub scratch_index: u16,
    pub pipeline: usize,
}

/// Per-instance handles for the occlusion temporal accumulate pass.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdOcclusionAccumulateResources {
    pub history_ring_index: usize,
    pub pipeline: usize,
}

/// Per-instance handles for the occlusion spatial filter pass.
///
/// `scratch_index` names a full-resolution RGBA16F scratch texture that holds
/// the spatially-filtered occlusion output.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdOcclusionFilterResources {
    pub scratch_index: u16,
    pub pipeline: usize,
}

/// All occlusion stabilizer pipelines and SRD-owned resources for one denoiser.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdOcclusionResources {
    pub clear_pipeline: usize,
    pub surface_mask: SrdRadianceSurfaceMaskResources,
    pub reproject: SrdRadianceReprojectResources,
    pub accumulate: SrdOcclusionAccumulateResources,
    pub filter: SrdOcclusionFilterResources,
}

/// Per-instance handles returned by `SrdInstance::prepare_radiance_combined`.
///
/// The combined fast path handles a single `CombinedRadianceInput` through the
/// core five passes (surface mask, reproject, accumulate, clamp, reconstruct)
/// without optional tail passes, giving simpler integrations a low-overhead entry.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdRadianceCombinedResources {
    pub clear_pipeline: usize,
    pub surface_mask: SrdRadianceSurfaceMaskResources,
    pub reproject: SrdRadianceReprojectResources,
    pub accumulate: SrdRadianceAccumulateResources,
    pub clamp: SrdRadianceClampResources,
    pub reconstruct: SrdRadianceReconstructResources,
}

/// Per-instance handles returned by `SrdInstance::prepare_radiance_diffuse_specular`.
///
/// Holds independent accumulate/clamp/reconstruct resource sets for diffuse and
/// specular channels, sharing a single surface-mask and reproject pipeline.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdRadianceDiffuseSpecularResources {
    pub clear_pipeline: usize,
    pub surface_mask: SrdRadianceSurfaceMaskResources,
    pub reproject: SrdRadianceReprojectResources,
    pub diffuse_accumulate: SrdRadianceAccumulateResources,
    pub diffuse_clamp: SrdRadianceClampResources,
    pub diffuse_reconstruct: SrdRadianceReconstructResources,
    pub specular_accumulate: SrdRadianceAccumulateResources,
    pub specular_clamp: SrdRadianceClampResources,
    pub specular_reconstruct: SrdRadianceReconstructResources,
}
