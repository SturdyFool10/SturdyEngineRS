// SRD guide pack/unpack helpers.
//
// Centralises the surface-attribute encoding chosen by SrdShaderContract on
// the Rust side. Every SRD pass that reads or writes a normal/roughness or
// linear view-depth value goes through this module so that the encoding
// constants change in exactly one place.
//
// Default encodings (see srd_denoiser.rs):
//   * normals    — Rgba8SignedOctahedralRoughness: oct-encoded normal in RG,
//                  zero in B, roughness in A. Roughness is stored linearly.
//   * view depth — single-channel positive linear view-Z.
//   * motion     — previous-minus-current in pixel units, stored RG16F or RG8.
//
// Other encodings will be added as new specialisations land; do not branch on
// them in shipping shaders without first lifting the choice into a
// specialisation parameter at the pass entry point.

import srd_common;

// Signed octahedral pack of a unit normal into [-1, 1]^2.
float2 srd_pack_normal_oct(float3 n) {
    float l1 = abs(n.x) + abs(n.y) + abs(n.z);
    float2 p = n.xy / max(l1, SRD_TINY);
    if (n.z < 0.0f) {
        float2 abs_p = float2(abs(p.y), abs(p.x));
        p = (float2(1.0f, 1.0f) - abs_p) * float2(sign(p.x), sign(p.y));
    }
    return p;
}

float3 srd_unpack_normal_oct(float2 p) {
    float3 n = float3(p.x, p.y, 1.0f - abs(p.x) - abs(p.y));
    if (n.z < 0.0f) {
        float2 abs_p = float2(abs(n.y), abs(n.x));
        n.xy = (float2(1.0f, 1.0f) - abs_p) * float2(sign(n.x), sign(n.y));
    }
    return normalize(n);
}

struct SrdNormalRoughness {
    float3 normal;
    float  roughness;
};

float4 srd_encode_normal_roughness(SrdNormalRoughness nr) {
    float2 oct = srd_pack_normal_oct(nr.normal) * 0.5f + 0.5f;
    return float4(oct.x, oct.y, 0.0f, saturate(nr.roughness));
}

SrdNormalRoughness srd_decode_normal_roughness(float4 packed) {
    float2 oct = packed.xy * 2.0f - 1.0f;
    SrdNormalRoughness nr;
    nr.normal = srd_unpack_normal_oct(oct);
    nr.roughness = saturate(packed.w);
    return nr;
}

// Compare two roughness values for "compatible enough to share filtering".
// Used by every spatial pass; the threshold is the family stage constant.
bool srd_roughness_compatible(float a, float b, float threshold) {
    return abs(a - b) <= threshold;
}

// Compare two normals via dot product against a cosine threshold. Pre-square
// the threshold on the host side if you want a tighter angular gate.
bool srd_normal_compatible(float3 a, float3 b, float cos_threshold) {
    return dot(a, b) >= cos_threshold;
}

// Linear view-Z compatibility test. `tolerance` is a per-pixel slope-adjusted
// epsilon supplied by the caller; SRD never picks a global depth tolerance.
bool srd_depth_compatible(float a, float b, float tolerance) {
    return abs(a - b) <= tolerance;
}
