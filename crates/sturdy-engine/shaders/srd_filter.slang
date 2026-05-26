// SRD bilateral filtering helpers.
//
// Edge-stopping weight functions used by every SRD spatial pass. Each weight
// is a unit-less multiplier in [0, 1] applied to a tap before accumulation.
// Combine weights multiplicatively; the caller normalises by the summed
// weight at the end.
//
// SRD favours a single variance-guided reconstruction pass with an adaptive
// radius rather than multi-iteration wavelet schemes. The kernel offsets here
// are designed for that style; new passes can introduce new offset patterns
// without breaking shared weight functions.

import srd_common;
import srd_packing;

// Plane-distance weight. Falls off as the candidate's view-depth diverges
// from the centre's view-depth, scaled by `tolerance` from the family stage
// constants.
float srd_weight_depth(float depth_centre, float depth_tap, float tolerance) {
    float t = abs(depth_centre - depth_tap) / max(tolerance, SRD_TINY);
    return exp(-t * t);
}

// Normal-angle weight. `sharpness` is the power applied to max(0, dot); larger
// values reject taps faster as the normal diverges. Typical sharpness: 8..32.
float srd_weight_normal(float3 n_centre, float3 n_tap, float sharpness) {
    float c = max(dot(n_centre, n_tap), 0.0f);
    return pow(c, sharpness);
}

// Roughness compatibility weight. Used by the radiance family for specular
// taps; diffuse paths can pass roughness=0 on both sides and effectively
// disable this weight.
float srd_weight_roughness(float r_centre, float r_tap, float tolerance) {
    float t = abs(r_centre - r_tap) / max(tolerance, SRD_TINY);
    return exp(-t * t);
}

// Material-class equality. Soft because the host-supplied material id is a
// hint, not a hard wall.
float srd_weight_material(uint m_centre, uint m_tap) {
    return m_centre == m_tap ? 1.0f : 0.25f;
}

// Luminance edge-stop. Uses the running standard deviation so the weight
// loosens as variance grows and tightens as variance shrinks. Add a small
// epsilon to avoid a divide by zero before the first frame of variance has
// been recorded.
float srd_weight_luminance(float l_centre, float l_tap, float sigma) {
    float t = abs(l_centre - l_tap) / max(sigma + SRD_TINY, SRD_TINY);
    return exp(-t * t);
}

// Static 5x5 cross-with-corners offsets used by the radiance spatial
// reconstruct pass. The diamond shape avoids the regular look that a full
// 5x5 box produces while still touching enough neighbours to handle
// short-history pixels.
static const int2 SRD_CROSS5_OFFSETS[9] = {
    int2( 0,  0),
    int2(-2,  0), int2( 2,  0),
    int2( 0, -2), int2( 0,  2),
    int2(-1, -1), int2( 1, -1),
    int2(-1,  1), int2( 1,  1),
};

// Scale a kernel offset by an adaptive stride. The caller chooses the stride
// based on per-pixel history length and local variance.
int2 srd_scaled_offset(int2 offset, uint stride) {
    return offset * int(max(stride, 1u));
}
