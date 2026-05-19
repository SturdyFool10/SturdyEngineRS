/// Stable GPU-scene object slot handle.
///
/// `GpuObjectId` identifies one persistent object slot in [`RenderWorld`](super::RenderWorld).
/// The slot is stable across frames so gameplay/ECS state can update compact
/// render-source data without re-submitting per-object draw work.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct GpuObjectId(u32);

impl GpuObjectId {
    /// Sentinel value for optional/user-authored handles.
    pub const INVALID: Self = Self(u32::MAX);

    /// Construct a handle from a raw slot value.
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// Return the raw slot value.
    pub const fn as_u32(self) -> u32 {
        self.0
    }

    /// Return the slot value as a `usize` for buffer/vector indexing.
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// Returns `false` for [`INVALID`](Self::INVALID).
    pub const fn is_valid(self) -> bool {
        self.0 != Self::INVALID.0
    }
}
