use crate::push_constants;

/// Push constants for `deferred_lighting.slang` (128 bytes).
/// Keep this at the Vulkan-spec minimum for broad driver compatibility.
#[push_constants]
pub(super) struct DeferredLightingConstants {
    pub(super) camera_world_pos: [f32; 4],   // 16
    pub(super) ambient: [f32; 3],            // 12
    pub(super) dir_light_count: u32,         //  4  -> 32
    pub(super) ibl_strength: f32,            //  4
    pub(super) ibl_max_layer: f32,           //  4
    pub(super) bvh_root: u32,                //  4
    pub(super) sky_enabled: u32,             //  4  -> 48
    pub(super) inv_view_proj: [[f32; 4]; 4], // 64  -> 112
    pub(super) sky_turbidity: f32,           //  4
    pub(super) sky_exposure: f32,            //  4
    pub(super) sky_sun_size: f32,            //  4  -> 124
    pub(super) _pad: f32,                    //  4  -> 128
}

/// Uniform buffer (48 bytes) bound as `"forward_lighting"` for the OIT forward lit pass.
/// Pre-bound by `DeferredPass::draw()` before the OIT collect phase runs.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct ForwardLightingUniforms {
    pub(super) camera_world_pos: [f32; 4], // 16
    pub(super) ambient: [f32; 3],          // 12
    pub(super) dir_light_count: u32,       //  4  -> 32
    pub(super) ibl_strength: f32,          //  4
    pub(super) ibl_max_layer: f32,         //  4
    pub(super) bvh_root: u32,              //  4
    pub(super) _pad: f32,                  //  4  -> 48
}
