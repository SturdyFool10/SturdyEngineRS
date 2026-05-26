mod api;
mod clear_history;
mod dispatch;
mod pipeline;
mod radiance_outlier;
mod radiance_stabilizer;
mod radiance_stabilizer_executor;
mod reference_temporal_executor;
mod resources;
mod settings;

pub use api::{SrdDenoiser, SrdDenoiserDesc, SrdInstance, SrdInstanceDesc};
pub use clear_history::{SRD_CLEAR_HISTORY_WORKGROUP_SIZE, SrdClearConstants};
pub use dispatch::{
    SrdConstantArena, SrdConstantRange, SrdDispatchDesc, SrdPassBuilder,
    SrdRadianceAccumulateConstants, SrdRadianceReconstructConstants,
    SrdRadianceReprojectConstants, SrdRadianceSurfaceMaskConstants,
    SRD_RADIANCE_SURFACE_MASK_TILE_SIZE, SrdSignalMomentsConstants,
};
pub use pipeline::{
    SrdPipelineDesc, SrdRadianceAccumulateResources, SrdRadianceReconstructResources,
    SrdRadianceReprojectResources, SrdRadianceSurfaceMaskResources, SrdReferenceTemporalPipelines,
};
pub use radiance_outlier::{SrdRadianceOutlierSuppressConstants, SrdRadianceOutlierSuppressResources};
pub use radiance_stabilizer::{
    SrdRadianceOutputResource, SrdRadianceStabilizerPlan, SrdRadianceStabilizerResources,
};
pub use radiance_stabilizer_executor::{
    SrdRadianceStabilizerExecutor, SrdRadianceStabilizerInputs, SrdRadianceStabilizerPrograms,
};
pub use reference_temporal_executor::{SrdReferenceTemporalExecutor, SrdReferenceTemporalPrograms};
pub use resources::{
    SrdDenoiserId, SrdDescriptorType, SrdHistoryRing, SrdPoolClass, SrdResourceDesc,
    SrdResourceFormatDesc, SrdResourceSlot, SrdTextureDesc,
};
pub use settings::{
    SrdCapabilities, SrdCommonSettings, SrdDenoiserMode, SrdDenoiserSettings, SrdDepthConvention,
    SrdFamilySettings, SrdHistoryMode, SrdHistoryRejectionSettings, SrdMotionVectorConvention,
    SrdNormalPacking, SrdOcclusionSettings, SrdOutlierClampSettings, SrdRadianceSettings,
    SrdReferenceSettings, SrdShadowSettings, SrdShaderContract, SrdSpectralLayout,
    SRD_TEMPORAL_CONSTANTS_SIZE, SrdTemporalBindings, SrdTemporalConstants, SrdVarianceSettings,
};

#[deprecated(
    since = "0.1.0",
    note = "use SrdDenoiser; SRD is the SturdyEngine-standard denoiser API"
)]
pub type RealtimeRayTracingDenoiser = SrdDenoiser;

#[cfg(test)]
#[path = "srd_denoiser_tests.rs"]
mod srd_denoiser_tests;
