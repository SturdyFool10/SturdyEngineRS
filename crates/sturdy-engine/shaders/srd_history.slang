// SRD reprojection helpers.
//
// One place that knows how to take a current-frame pixel and find its
// previous-frame footprint. Every SRD family that needs temporal history goes
// through here so the validation rules stay consistent.
//
// Convention: motion vectors are previous-minus-current in pixel units, as
// promised by SrdShaderContract::motion_vectors. The renderer compensates for
// jitter before SRD reads them. Out-of-bounds previous coordinates downgrade
// to "no valid tap"; depth mismatch downgrades to bilinear fallback; material
// mismatch downgrades to bilinear fallback as well.

import srd_common;
import srd_packing;

struct SrdReprojection {
  // Previous-frame pixel-index coordinate (sub-pixel). Sampling code adds the
  // half-pixel centre offset when converting this value to UV space.
  // Negative components mean the lookup landed off-screen.
  float2 prev_pixel;
  // Footprint validity. 0 means reject entirely; 1 means usable. The shader
  // is free to gate bicubic on a higher threshold than bilinear.
  float validity;
  // Whether the caller should fall back to bilinear filtering instead of
  // bicubic (depth/material mismatch downgraded the tap).
  bool bilinear_fallback;
};

// Project a current-frame pixel back into the previous frame using the
// supplied previous-minus-current motion vector and an immediate validity test.
SrdReprojection srd_reproject_pixel(uint2 pixel, float2 motion, uint2 extent) {
  SrdReprojection r;
  r.prev_pixel = float2(pixel) + motion;
  r.bilinear_fallback = false;

  bool inside = r.prev_pixel.x >= 0.0f && r.prev_pixel.y >= 0.0f &&
                r.prev_pixel.x < float(extent.x) &&
                r.prev_pixel.y < float(extent.y);
  r.validity = inside ? 1.0f : 0.0f;
  return r;
}

// Validate a reprojected tap against the previous-frame guide. Returns the
// updated SrdReprojection with reduced validity if a soft failure occurred,
// or zeroed validity if a hard failure occurred.
SrdReprojection
srd_validate_against_guide(SrdReprojection candidate, float current_view_depth,
                           float prev_view_depth, float depth_tolerance,
                           float3 current_normal, float3 prev_normal,
                           float normal_cos_threshold, bool material_match) {
  if (candidate.validity <= 0.0f)
    return candidate;

  if (!srd_depth_compatible(current_view_depth, prev_view_depth,
                            depth_tolerance)) {
    candidate.bilinear_fallback = true;
  }
  if (!srd_normal_compatible(current_normal, prev_normal,
                             normal_cos_threshold)) {
    candidate.validity *= 0.5f;
    candidate.bilinear_fallback = true;
  }
  if (!material_match) {
    candidate.bilinear_fallback = true;
  }
  return candidate;
}

// Compute a per-pixel depth tolerance that scales with view depth. Constants
// were chosen so that a 1% relative change passes at far range. Replace with
// a plane-distance test once we have per-pixel slopes available.
float srd_depth_tolerance(float view_depth, float relative_epsilon,
                          float absolute_epsilon) {
  return max(view_depth * relative_epsilon, absolute_epsilon);
}
