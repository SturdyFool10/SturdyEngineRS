mod api;
mod clear_history;
mod dispatch;
mod occlusion;
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
    SrdOcclusionAccumulateConstants, SrdOcclusionFilterConstants,
    SrdRadianceAccumulateConstants, SrdRadianceAtrousConstants, SrdRadianceClampConstants,
    SrdRadiancePostBlurConstants, SrdRadianceReconstructConstants, SrdRadianceReprojectConstants,
    SrdRadianceSpatialFilterConstants, SrdRadianceSurfaceMaskConstants,
    SRD_RADIANCE_SURFACE_MASK_TILE_SIZE, SrdSignalMomentsConstants,
};
pub use occlusion::{
    SrdOcclusionPlan, SrdOcclusionStabilizerExecutor, SrdOcclusionStabilizerInputs,
    SrdOcclusionStabilizerPrograms,
};
pub use pipeline::{
    SrdOcclusionAccumulateResources, SrdOcclusionFilterResources, SrdOcclusionResources,
    SrdPipelineDesc, SrdRadianceAccumulateResources, SrdRadianceAtrousResources,
    SrdRadianceClampResources, SrdRadianceCombinedResources, SrdRadianceDiffuseSpecularResources,
    SrdRadiancePostBlurResources, SrdRadianceReconstructResources, SrdRadianceReprojectResources,
    SrdRadianceSpatialFilterResources, SrdRadianceSurfaceMaskResources, SrdReferenceTemporalPipelines,
};
pub use radiance_outlier::{SrdRadianceOutlierSuppressConstants, SrdRadianceOutlierSuppressResources};
pub use radiance_stabilizer::{
    SrdRadianceCombinedPlan, SrdRadianceDiffuseSpecularPlan, SrdRadianceOutputResource,
    SrdRadianceStabilizerPlan, SrdRadianceStabilizerResources,
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
    SrdAtrousSettings, SrdCapabilities, SrdCommonSettings, SrdDenoiserMode, SrdDenoiserSettings,
    SrdDepthConvention, SrdFamilySettings, SrdHistoryClampSettings, SrdHistoryMode,
    SrdHistoryRejectionSettings, SrdHitDistanceSettings, SrdMotionVectorConvention, SrdNormalPacking,
    SrdOcclusionSettings, SrdOutlierClampSettings, SrdPostBlurSettings, SrdRadianceSettings,
    SrdReferenceSettings, SrdShadowSettings, SrdShaderContract, SrdSpatialFilterSettings,
    SrdSpectralLayout, SRD_TEMPORAL_CONSTANTS_SIZE, SrdTemporalBindings, SrdTemporalConstants,
    SrdVarianceSettings,
};

#[deprecated(
    since = "0.1.0",
    note = "use SrdDenoiser; SRD is the SturdyEngine-standard denoiser API"
)]
pub type RealtimeRayTracingDenoiser = SrdDenoiser;

#[cfg(test)]
#[path = "srd_denoiser_tests.rs"]
mod srd_denoiser_tests;
