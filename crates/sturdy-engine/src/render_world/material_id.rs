/// Stable slot in the future GPU material table.
///
/// Today this is a lightweight public handle used by ECS render components.
/// It is intentionally separate from `MeshId` so the renderer can move toward
/// a bindless material table without changing gameplay component shapes.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct MaterialId(u32);

impl MaterialId {
    pub const INVALID: Self = Self(u32::MAX);

    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }

    pub const fn is_valid(self) -> bool {
        self.0 != Self::INVALID.0
    }
}
