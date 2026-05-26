// SRD Radiance Stabilizer — Surface Mask pass.
//
// First pass of the radiance pipeline. Classifies each 8x8 tile by whether
// at least one pixel is in denoising range, so later passes can early-out on
// fully-sky/out-of-range tiles.
//
// First-cut implementation emits a single-channel R8Unorm flag (0 = skip,
// 1 = process). The richer per-tile bit layout described in
// docs/srd_shaders.md §5.5 (variance bits, etc.) will land as additional
// channels once the spatial-reconstruct pass needs them.
//
// Reads:    srd_guide_view_depth   (Texture2D<float>)
// Writes:   srd_radiance_surface_mask  (RWTexture2D<float>, tile resolution)
// Constants: SrdRadianceSurfaceMaskConstants
// Dispatch: 8x8x1 thread groups; one group per output tile

import srd_common;

Texture2D<float>   srd_guide_view_depth;
RWTexture2D<float> srd_radiance_surface_mask;

struct SrdRadianceSurfaceMaskConstants {
    uint2 input_extent;       // pixel extent of the view-depth guide
    uint2 mask_extent;        // tile extent of the output mask
    float effective_range;    // SrdCommonSettings::effective_range
    uint  tile_size;          // pixels per side per tile (matches numthreads.x)
    uint2 _pad;
};
uniform SrdRadianceSurfaceMaskConstants push;

groupshared uint srd_in_range_count;

[numthreads(8, 8, 1)]
void main(uint3 gid : SV_GroupID, uint3 lid : SV_GroupThreadID) {
    if (lid.x == 0 && lid.y == 0) {
        srd_in_range_count = 0;
    }
    GroupMemoryBarrierWithGroupSync();

    uint2 pixel = gid.xy * push.tile_size + lid.xy;
    if (all(pixel < push.input_extent)) {
        float view_depth = srd_guide_view_depth.Load(int3(pixel, 0));
        if (srd_is_in_range(view_depth, push.effective_range)) {
            InterlockedAdd(srd_in_range_count, 1u);
        }
    }
    GroupMemoryBarrierWithGroupSync();

    if (lid.x == 0 && lid.y == 0) {
        if (all(gid.xy < push.mask_extent)) {
            float mask = srd_in_range_count > 0u ? 1.0f : 0.0f;
            srd_radiance_surface_mask[gid.xy] = mask;
        }
    }
}
