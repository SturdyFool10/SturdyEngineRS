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
