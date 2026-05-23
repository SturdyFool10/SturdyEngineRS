use super::DeferredPass;
use crate::{
    PointShadowConfig, PointShadowPass, SpotShadowConfig, SpotShadowPass, shadow_pass::CsmConfig,
    shadow_pipeline::ShadowPipeline,
};

impl DeferredPass {
    /// Mutable access to the shadow pipeline for direct configuration.
    pub fn shadows_mut(&mut self) -> &mut ShadowPipeline {
        &mut self.shadows
    }

    /// Expose the CSM configuration for live tuning.
    pub fn csm_config_mut(&mut self) -> &mut CsmConfig {
        self.shadows.csm_config_mut()
    }

    /// Enable spot light shadow maps.
    ///
    /// ```ignore
    /// let spot_shadows = SpotShadowPass::new(&engine)?;
    /// deferred.set_spot_shadows(spot_shadows);
    /// ```
    pub fn set_spot_shadows(&mut self, pass: SpotShadowPass) {
        self.shadows.set_spot_shadows(pass);
    }

    /// Remove the spot light shadow pass.
    pub fn clear_spot_shadows(&mut self) {
        self.shadows.clear_spot_shadows();
    }

    /// Expose spot shadow config for tuning.
    pub fn spot_shadow_config_mut(&mut self) -> Option<&mut SpotShadowConfig> {
        self.shadows.spot_shadow_config_mut()
    }

    /// Enable dual-paraboloid point light shadow maps for up to 4 lights.
    ///
    /// ```ignore
    /// let point_shadows = PointShadowPass::new(&engine)?;
    /// deferred.set_point_shadows(point_shadows);
    /// ```
    pub fn set_point_shadows(&mut self, pass: PointShadowPass) {
        self.shadows.set_point_shadows(pass);
    }

    /// Remove the point light shadow pass.
    pub fn clear_point_shadows(&mut self) {
        self.shadows.clear_point_shadows();
    }

    /// Expose point shadow config for tuning.
    pub fn point_shadow_config_mut(&mut self) -> Option<&mut PointShadowConfig> {
        self.shadows.point_shadow_config_mut()
    }
}
