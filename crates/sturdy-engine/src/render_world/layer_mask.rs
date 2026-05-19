/// Visibility layer mask for render extraction and culling.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct LayerMask(u32);

impl LayerMask {
    pub const NONE: Self = Self(0);
    pub const ALL: Self = Self(u32::MAX);

    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    pub const fn single(layer: u8) -> Self {
        if layer >= 32 {
            Self::NONE
        } else {
            Self(1_u32 << layer)
        }
    }

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn intersects(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }
}

impl Default for LayerMask {
    fn default() -> Self {
        Self::ALL
    }
}
