use super::DeferredPass;
use crate::oit_pass::{OitConfig, OitPass};

impl DeferredPass {
    /// Enable order-independent transparency for `Translucent`-domain materials.
    ///
    /// Without this, translucent objects are silently skipped.
    /// ```ignore
    /// deferred.set_oit(OitPass::new(&engine)?);
    /// ```
    pub fn set_oit(&mut self, oit: OitPass) {
        self.oit = Some(oit);
    }

    /// Remove the OIT pass (translucent objects will no longer be rendered).
    pub fn clear_oit(&mut self) {
        self.oit = None;
    }

    /// Expose OIT configuration for live tuning.
    pub fn oit_config_mut(&mut self) -> Option<&mut OitConfig> {
        self.oit.as_mut().map(|o| &mut o.config)
    }
}
