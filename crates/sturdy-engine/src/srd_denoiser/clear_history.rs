use glam::UVec2;

/// Workgroup shape used by `shaders/srd_clear_history.slang`.
pub const SRD_CLEAR_HISTORY_WORKGROUP_SIZE: [u32; 3] = [8, 8, 1];

/// Push constants for the global SRD history-clear pass.
///
/// Matches `SrdClearConstants` in `shaders/srd_clear_history.slang`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdClearConstants {
    pub extent: [u32; 2],
    pub _pad: [u32; 2],
}

pub(super) fn clear_grid_size(extent: UVec2) -> [u32; 3] {
    [
        extent
            .x
            .div_ceil(SRD_CLEAR_HISTORY_WORKGROUP_SIZE[0])
            .max(1),
        extent
            .y
            .div_ceil(SRD_CLEAR_HISTORY_WORKGROUP_SIZE[1])
            .max(1),
        1,
    ]
}
