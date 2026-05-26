// SRD shared utilities.
//
// Engine-owned constants and small helpers used across every SRD pass. Keep
// this module dependency-free so it can be imported by all other SRD modules
// without cycles.
//
// See docs/srd_shaders.md sections 5.1 and 5.5 for the contract this module
// participates in.

static const float SRD_PI = 3.14159265f;
static const float SRD_INV_PI = 0.31830989f;
static const float SRD_TINY = 1.0e-6f;
static const float SRD_LUMA_MAX = 65504.0f;

// Linear view depth is positive into the scene. Values at or above the
// per-frame effective_range are treated as sky or out of denoising range and
// must be excluded from spatial filters and history validation.
bool srd_is_in_range(float view_depth, float effective_range) {
    return view_depth > 0.0f && view_depth < effective_range;
}

// Clamp a colour to a finite, non-negative range before it is written to
// storage or fed into weight calculations. NaN and Inf propagate through
// most arithmetic; this is the single chokepoint that scrubs them.
float3 srd_sanitize_colour(float3 c) {
    return clamp(select(isfinite(c), c, float3(0.0f, 0.0f, 0.0f)),
                 float3(0.0f, 0.0f, 0.0f),
                 float3(SRD_LUMA_MAX, SRD_LUMA_MAX, SRD_LUMA_MAX));
}

float srd_sanitize_scalar(float v) {
    return isfinite(v) ? clamp(v, 0.0f, SRD_LUMA_MAX) : 0.0f;
}

// Rec. 709 luminance. SRD uses the same weights across every family so
// luminance-based decisions are comparable across passes.
float srd_luminance(float3 c) {
    return dot(c, float3(0.2126f, 0.7152f, 0.0722f));
}

// Map a [0, 1] pixel index to a UV at the pixel centre for a texture of the
// given pixel extent. Pure helper; no sampler involved.
float2 srd_pixel_centre_uv(uint2 pixel, uint2 extent) {
    return (float2(pixel) + 0.5f) / float2(max(extent, uint2(1, 1)));
}

// Saturating cast from a non-negative float into the [0, 255] unorm range. The
// surface mask shader uses this to write a packed byte through an R8Unorm
// storage texture without needing a dedicated uint format.
float srd_pack_unorm_byte(uint value) {
    return float(min(value, 255u)) / 255.0f;
}

uint srd_unpack_unorm_byte(float value) {
    return uint(round(clamp(value, 0.0f, 1.0f) * 255.0f));
}
