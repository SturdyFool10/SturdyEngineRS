// SRD Radiance Stabilizer — Reproject pass.
//
// Locates the previous-frame footprint for every pixel in the current frame
// and writes a packed reprojection record. The accumulate pass consumes this
// in the next step; a future specular reprojection path can reuse the same
// scratch without re-running the validation.
//
// Reads:    srd_guide_motion              (Texture2D<float2>, prev - current pixels)
//           srd_guide_view_depth          (Texture2D<float>)
//           srd_guide_normal_roughness    (Texture2D<float4>, SrdShaderContract default)
//           srd_prev_guide_view_depth     (Texture2D<float>)
//           srd_prev_guide_normal_roughness (Texture2D<float4>)
//           srd_radiance_surface_mask     (Texture2D<float>, tile-resolution, R8Unorm)
//           srd_guide_hit_distance        (Texture2D<float>, optional, only when use_hit_distance != 0)
//           srd_prev_guide_hit_distance   (Texture2D<float>, optional, only when use_hit_distance != 0)
// Writes:   srd_radiance_reproject_out    (RWTexture2D<float4>: prev_x, prev_y, validity, fallback)
// Constants: SrdRadianceReprojectConstants
// Dispatch: 8x8x1 thread groups covering the rect extent

import srd_common;
import srd_packing;
import srd_history;

Texture2D<float2> srd_guide_motion;
Texture2D<float>  srd_guide_view_depth;
Texture2D<float4> srd_guide_normal_roughness;
Texture2D<float>  srd_prev_guide_view_depth;
Texture2D<float4> srd_prev_guide_normal_roughness;
Texture2D<float>  srd_radiance_surface_mask;
Texture2D<float>  srd_guide_hit_distance;
Texture2D<float>  srd_prev_guide_hit_distance;
RWTexture2D<float4> srd_radiance_reproject_out;

struct SrdRadianceReprojectConstants {
    uint2 rect_extent;
    uint2 mask_extent;
    float2 motion_scale;
    float depth_tolerance_relative;
    float depth_tolerance_absolute;
    float normal_cos_threshold;
    uint  use_surface_mask;
    uint  tile_size;
    uint  use_hit_distance;
    float hit_distance_relative_threshold;
};
uniform SrdRadianceReprojectConstants push;

[numthreads(8, 8, 1)]
void main(uint3 gid : SV_GroupID, uint3 lid : SV_GroupThreadID, uint3 tid : SV_DispatchThreadID) {
    if (any(tid.xy >= push.rect_extent)) return;

    // Tile-level early-out. The surface mask is one pixel per
    // push.tile_size×push.tile_size source tile.
    if (push.use_surface_mask != 0u) {
        uint2 tile = tid.xy / max(push.tile_size, 1u);
        if (all(tile < push.mask_extent)) {
            float mask = srd_radiance_surface_mask.Load(int3(tile, 0));
            if (mask <= 0.0f) {
                srd_radiance_reproject_out[tid.xy] = float4(0.0f, 0.0f, 0.0f, 0.0f);
                return;
            }
        }
    }

    float current_view_depth = srd_guide_view_depth.Load(int3(tid.xy, 0));
    float4 current_nr_packed = srd_guide_normal_roughness.Load(int3(tid.xy, 0));
    SrdNormalRoughness current_nr = srd_decode_normal_roughness(current_nr_packed);

    float2 motion = srd_guide_motion.Load(int3(tid.xy, 0)) * push.motion_scale;

    SrdReprojection projected = srd_reproject_pixel(tid.xy, motion, push.rect_extent);

    if (projected.validity > 0.0f) {
        int2 prev_pixel = clamp(
            int2(floor(projected.prev_pixel)),
            int2(0, 0),
            int2(push.rect_extent) - int2(1, 1));
        float prev_view_depth = srd_prev_guide_view_depth.Load(int3(prev_pixel, 0));
        float4 prev_nr_packed = srd_prev_guide_normal_roughness.Load(int3(prev_pixel, 0));
        SrdNormalRoughness prev_nr = srd_decode_normal_roughness(prev_nr_packed);

        float tolerance = srd_depth_tolerance(
            current_view_depth,
            push.depth_tolerance_relative,
            push.depth_tolerance_absolute);

        projected = srd_validate_against_guide(
            projected,
            current_view_depth,
            prev_view_depth,
            tolerance,
            current_nr.normal,
            prev_nr.normal,
            push.normal_cos_threshold,
            /* material_match */ true);

        // Optional hit-distance consistency check. A large relative change in
        // ray hit distance indicates the surface reflected/occluded a different
        // object than the previous frame, so we downgrade validity.
        if (push.use_hit_distance != 0u && projected.validity > 0.0f) {
            float current_hit = srd_guide_hit_distance.Load(int3(tid.xy, 0));
            float prev_hit    = srd_prev_guide_hit_distance.Load(int3(prev_pixel, 0));
            float denom       = max(current_hit, prev_hit);
            if (denom > 0.0f) {
                float rel_diff = abs(current_hit - prev_hit) / denom;
                if (rel_diff > push.hit_distance_relative_threshold) {
                    projected.validity = 0.0f;
                }
            }
        }
    }

    srd_radiance_reproject_out[tid.xy] = float4(
        projected.prev_pixel.x,
        projected.prev_pixel.y,
        projected.validity,
        projected.bilinear_fallback ? 1.0f : 0.0f);
}
