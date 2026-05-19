use super::{LayerMask, VisibilityFlags};

/// ECS component controlling render visibility, shadow participation, and layers.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RenderVisibility {
    pub flags: VisibilityFlags,
    pub layer_mask: LayerMask,
}

impl RenderVisibility {
    pub fn new(flags: VisibilityFlags, layer_mask: LayerMask) -> Self {
        Self { flags, layer_mask }
    }

    pub fn hidden() -> Self {
        Self {
            flags: VisibilityFlags::NONE,
            layer_mask: LayerMask::ALL,
        }
    }

    pub fn visible(mut self, visible: bool) -> Self {
        if visible {
            self.flags.insert(VisibilityFlags::VISIBLE);
        } else {
            self.flags.remove(VisibilityFlags::VISIBLE);
        }
        self
    }

    pub fn shadow_caster(mut self, casts_shadow: bool) -> Self {
        if casts_shadow {
            self.flags.insert(VisibilityFlags::CAST_SHADOW);
        } else {
            self.flags.remove(VisibilityFlags::CAST_SHADOW);
        }
        self
    }

    pub fn receive_shadows(mut self, receives_shadow: bool) -> Self {
        if receives_shadow {
            self.flags.insert(VisibilityFlags::RECEIVE_SHADOW);
        } else {
            self.flags.remove(VisibilityFlags::RECEIVE_SHADOW);
        }
        self
    }

    pub fn with_layer_mask(mut self, layer_mask: LayerMask) -> Self {
        self.layer_mask = layer_mask;
        self
    }
}

impl Default for RenderVisibility {
    fn default() -> Self {
        Self {
            flags: VisibilityFlags::default(),
            layer_mask: LayerMask::default(),
        }
    }
}
