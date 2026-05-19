use std::ops::{BitOr, BitOrAssign};

/// Per-object visibility/classification flags used by render extraction.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct VisibilityFlags(u32);

impl VisibilityFlags {
    pub const NONE: Self = Self(0);
    pub const VISIBLE: Self = Self(1 << 0);
    pub const CAST_SHADOW: Self = Self(1 << 1);
    pub const RECEIVE_SHADOW: Self = Self(1 << 2);
    pub const DYNAMIC: Self = Self(1 << 3);
    pub const STATIC: Self = Self(1 << 4);

    pub const DEFAULT_RENDERABLE: Self =
        Self(Self::VISIBLE.0 | Self::CAST_SHADOW.0 | Self::RECEIVE_SHADOW.0 | Self::DYNAMIC.0);

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

    pub fn remove(&mut self, flag: Self) {
        self.0 &= !flag.0;
    }
}

impl Default for VisibilityFlags {
    fn default() -> Self {
        Self::DEFAULT_RENDERABLE
    }
}

impl BitOr for VisibilityFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for VisibilityFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}
