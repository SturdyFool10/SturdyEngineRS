// Physically based BRDF library.
//
// Include this module in any lit shader:
//   import brdf;  (or #include "brdf.slang" depending on your Slang setup)
//
// Provides:
//   GGX specular lobe  (Trowbridge-Reitz NDF, Smith height-correlated G2, Schlick Fresnel)
//   Lambertian diffuse (energy-conserving complement)
//   Analytic point, spot, and directional light evaluation
//   Octahedral normal encode/decode helpers
//
// All functions expect pre-normalised input vectors and NdotL > 0 where noted.
// Constants: PI = 3.14159265

static const float BRDF_PI = 3.14159265f;
static const float BRDF_INV_PI = 0.31830989f;

// ── Octahedral normal helpers ─────────────────────────────────────────────────

float2 oct_encode(float3 n) {
    float3 a = abs(n);
    n /= a.x + a.y + a.z;
    if (n.z < 0.0f) {
        float2 s = float2(n.x >= 0.0f ? 1.0f : -1.0f, n.y >= 0.0f ? 1.0f : -1.0f);
        n.xy = (1.0f - abs(n.yx)) * s;
    }
    return n.xy;
}

float3 oct_decode(float2 f) {
    float3 n = float3(f.x, f.y, 1.0f - abs(f.x) - abs(f.y));
    if (n.z < 0.0f) {
        float2 s = float2(n.x >= 0.0f ? 1.0f : -1.0f, n.y >= 0.0f ? 1.0f : -1.0f);
        n.xy = (1.0f - abs(n.yx)) * s;
    }
    return normalize(n);
}

// ── GGX specular ──────────────────────────────────────────────────────────────

// Trowbridge-Reitz NDF. `alpha2` = (roughness²)².
float brdf_ggx_ndf(float NdotH, float alpha2) {
    float d = NdotH * NdotH * (alpha2 - 1.0f) + 1.0f;
    return alpha2 / (BRDF_PI * d * d);
}

// Smith height-correlated G2 / (4·NdotL·NdotV) — combined geometry + denominator.
// Multiply by D·F·NdotL to get the full specular contribution.
float brdf_ggx_vis(float NdotL, float NdotV, float alpha2) {
    float denom_l = NdotV * sqrt(alpha2 + NdotL * NdotL * (1.0f - alpha2));
    float denom_v = NdotL * sqrt(alpha2 + NdotV * NdotV * (1.0f - alpha2));
    return 0.5f / max(denom_l + denom_v, 1e-5f);
}

// Schlick Fresnel.
float3 brdf_fresnel(float VdotH, float3 F0) {
    float t = 1.0f - saturate(VdotH);
    float t2 = t * t;
    return F0 + (1.0f - F0) * (t2 * t2 * t);
}

// Full GGX specular term (D·G·F / 4·NdotL·NdotV), pre-multiplied by NdotL.
// Returns the specular radiance contribution from one light direction.
float3 brdf_specular(float NdotL, float NdotV, float NdotH, float VdotH,
                     float3 F0, float alpha2) {
    float  D   = brdf_ggx_ndf(NdotH, alpha2);
    float  Vis = brdf_ggx_vis(NdotL, NdotV, alpha2);
    float3 F   = brdf_fresnel(VdotH, F0);
    return D * Vis * F * NdotL;
}

// ── Lambertian diffuse ────────────────────────────────────────────────────────

// kd = (1 - F0) * (1 - metallic)  (energy-conserving; conductors have no diffuse)
// Returns diffuse radiance contribution from one light direction.
float3 brdf_diffuse(float NdotL, float3 albedo, float3 F0, float metallic) {
    float3 kd = (1.0f - F0) * (1.0f - metallic);
    return kd * albedo * BRDF_INV_PI * NdotL;
}

// ── Combined BRDF evaluation ──────────────────────────────────────────────────

// Evaluate the full PBR BRDF for a single analytic light direction.
// Returns (diffuse + specular) * light_radiance.
float3 brdf_eval(
    float3 N,          // surface normal (normalised)
    float3 V,          // view direction (normalised, toward camera)
    float3 L,          // light direction (normalised, toward light)
    float3 albedo,
    float  metallic,
    float  roughness,
    float3 light_radiance)  // light colour * intensity * attenuation
{
    float3 H = normalize(L + V);
    float NdotL = saturate(dot(N, L));
    if (NdotL <= 0.0f) return float3(0.0f, 0.0f, 0.0f);

    float NdotV = saturate(dot(N, V)) + 1e-5f;
    float NdotH = saturate(dot(N, H));
    float VdotH = saturate(dot(V, H));

    float  alpha2 = roughness * roughness * roughness * roughness; // (roughness²)²
    float3 F0     = lerp(float3(0.04f, 0.04f, 0.04f), albedo, metallic);

    float3 spec = brdf_specular(NdotL, NdotV, NdotH, VdotH, F0, alpha2);
    float3 diff = brdf_diffuse(NdotL, albedo, F0, metallic);

    return (diff + spec) * light_radiance;
}

// ── Light attenuation ─────────────────────────────────────────────────────────

// Physically correct inverse-square attenuation with a smooth distance cutoff.
// `range` is the light's maximum influence radius in world units.
float brdf_point_attenuation(float dist, float range) {
    if (range <= 0.0f) return 1.0f;
    float ratio = dist / range;
    // Smooth falloff: 1 at centre, 0 at range, continuous derivative.
    float num = saturate(1.0f - ratio * ratio * ratio * ratio);
    return (num * num) / max(dist * dist, 0.0001f);
}

// Spot light angular attenuation.
// `cos_inner` = cos(inner cone angle), `cos_outer` = cos(outer cone angle).
float brdf_spot_attenuation(float3 L, float3 spot_dir, float cos_inner, float cos_outer) {
    float cos_theta = dot(-L, spot_dir); // L points toward light; spot_dir points away
    return saturate((cos_theta - cos_outer) / max(cos_inner - cos_outer, 1e-4f));
}
