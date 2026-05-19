use std::ops::{BitOr, BitOrAssign};

/// Bitset of render-world object data that changed since the last snapshot.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct RenderDirtyFlags(u32);

impl RenderDirtyFlags {
    pub const NONE: Self = Self(0);
    pub const STRUCTURAL: Self = Self(1 << 0);
    pub const TRANSFORM: Self = Self(1 << 1);
    pub const PREVIOUS_TRANSFORM: Self = Self(1 << 2);
    pub const MESH: Self = Self(1 << 3);
    pub const MATERIAL: Self = Self(1 << 4);
    pub const BOUNDS: Self = Self(1 << 5);
    pub const VISIBILITY: Self = Self(1 << 6);

    pub const ALL: Self = Self(
        Self::STRUCTURAL.0
            | Self::TRANSFORM.0
            | Self::PREVIOUS_TRANSFORM.0
            | Self::MESH.0
            | Self::MATERIAL.0
            | Self::BOUNDS.0
            | Self::VISIBILITY.0,
    );

    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn contains(self, flag: Self) -> bool {
        (self.0 & flag.0) == flag.0
    }

    pub fn insert(&mut self, flag: Self) {
        self.0 |= flag.0;
    }

    pub fn clear(&mut self) {
        self.0 = 0;
    }
}

impl BitOr for RenderDirtyFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for RenderDirtyFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}
