use super::DeferredPass;
use crate::{
    PointShadowConfig, PointShadowPass, SpotShadowConfig, SpotShadowPass, shadow_pass::CsmConfig,
};

impl DeferredPass {
    /// Expose the CSM configuration for live tuning.
    pub fn csm_config_mut(&mut self) -> &mut CsmConfig {
        &mut self.csm.config
    }

    /// Enable spot light shadow maps.
    ///
    /// ```ignore
    /// let spot_shadows = SpotShadowPass::new(&engine)?;
    /// deferred.set_spot_shadows(spot_shadows);
    /// ```
    pub fn set_spot_shadows(&mut self, pass: SpotShadowPass) {
        self.spot_shadows = Some(pass);
    }

    /// Remove the spot light shadow pass.
    pub fn clear_spot_shadows(&mut self) {
        self.spot_shadows = None;
    }

    /// Expose spot shadow config for tuning.
    pub fn spot_shadow_config_mut(&mut self) -> Option<&mut SpotShadowConfig> {
        self.spot_shadows.as_mut().map(|s| &mut s.config)
    }

    /// Enable dual-paraboloid point light shadow maps for up to 4 lights.
    ///
    /// ```ignore
    /// let point_shadows = PointShadowPass::new(&engine)?;
    /// deferred.set_point_shadows(point_shadows);
    /// ```
    pub fn set_point_shadows(&mut self, pass: PointShadowPass) {
        self.point_shadows = Some(pass);
    }

    /// Remove the point light shadow pass.
    pub fn clear_point_shadows(&mut self) {
        self.point_shadows = None;
    }

    /// Expose point shadow config for tuning.
    pub fn point_shadow_config_mut(&mut self) -> Option<&mut PointShadowConfig> {
        self.point_shadows.as_mut().map(|p| &mut p.config)
    }
}
