// Alpha-tested depth-only shader for MaterialDomain::Masked shadow casting.
//
// Discards fragments whose material alpha is below the cutoff threshold (0.5),
// producing correct "punched-through" shadow silhouettes for foliage, fences,
// and other alpha-masked geometry.
//
// Vertex stage: shadow_depth.slang (transforms to light clip space, outputs UV).
// Fragment stage: this file — reads material_desc[0].albedo.a, discards below 0.5.
//
// Roadmap: Track 6b — ShadowVariant (alpha-test for Masked domain in CSM).

struct MaterialConstants {
    float4 albedo;             // a = opacity / alpha cutoff mask value
    float4 emissive_metallic;
    float4 roughness_flags;
};
StructuredBuffer<MaterialConstants> material_desc;

// Alpha cutoff: fragments with alpha below this threshold are discarded.
// Standard value matches glTF alphaCutoff default.
static const float ALPHA_CUTOFF = 0.5;

void main() {
    MaterialConstants M = material_desc[0];
    if (M.albedo.a < ALPHA_CUTOFF) {
        discard;
    }
    // Depth is written automatically by the rasterizer.
}
