// Environment map specular prefilter compute shader.
//
// Precomputes GGX-weighted specular irradiance from an equirectangular HDR
// environment map. One dispatch per roughness level; outputs a single layer
// of a RWTexture2DArray.
//
// The prefiltered specular term of the split-sum approximation:
//   ∫ Li(l) * f(l,v) * cos(θ_l) dl ≈ env_specular.Sample(reflect(N,V), roughness)
//
// Reference: "Real Shading in Unreal Engine 4" — Epic 2013.

Texture2D<float4>     env_equirect;     // source equirectangular HDR
SamplerState          env_sampler;
RWTexture2DArray<float4> output;         // target layer array

// Push constants
uniform float roughness;               // 0.0 = mirror, 1.0 = fully diffuse
uniform int   layer_index;             // which layer of the output array to write

// ── Math helpers ──────────────────────────────────────────────────────────────

static const float PI = 3.14159265359;

float radical_inverse_vdc(uint bits) {
    bits = (bits << 16u) | (bits >> 16u);
    bits = ((bits & 0x55555555u) << 1u) | ((bits & 0xAAAAAAAAu) >> 1u);
    bits = ((bits & 0x33333333u) << 2u) | ((bits & 0xCCCCCCCCu) >> 2u);
    bits = ((bits & 0x0F0F0F0Fu) << 4u) | ((bits & 0xF0F0F0F0u) >> 4u);
    bits = ((bits & 0x00FF00FFu) << 8u) | ((bits & 0xFF00FF00u) >> 8u);
    return float(bits) * 2.3283064365386963e-10;
}

float2 hammersley(uint i, uint n) {
    return float2(float(i) / float(n), radical_inverse_vdc(i));
}

float3 importance_sample_ggx(float2 xi, float3 N, float roughness) {
    float a   = roughness * roughness;
    float phi = 2.0 * PI * xi.x;
    float cos_theta = sqrt((1.0 - xi.y) / (1.0 + (a * a - 1.0) * xi.y));
    float sin_theta = sqrt(1.0 - cos_theta * cos_theta);

    float3 H_tan = float3(sin_theta * cos(phi), sin_theta * sin(phi), cos_theta);

    // Transform from tangent space to world space.
    float3 up    = abs(N.z) < 0.999 ? float3(0.0, 0.0, 1.0) : float3(1.0, 0.0, 0.0);
    float3 right = normalize(cross(up, N));
    float3 fwd   = cross(N, right);

    return normalize(right * H_tan.x + fwd * H_tan.y + N * H_tan.z);
}

// Convert equirectangular UV → direction.
float3 uv_to_dir(float2 uv) {
    float phi   = (uv.x - 0.5) * 2.0 * PI;
    float theta = uv.y * PI;
    return float3(
        sin(theta) * cos(phi),
        cos(theta),
        sin(theta) * sin(phi)
    );
}

// Convert direction → equirectangular UV.
float2 dir_to_uv(float3 d) {
    d = normalize(d);
    float u = atan2(d.z, d.x) / (2.0 * PI) + 0.5;
    float v = acos(clamp(d.y, -1.0, 1.0)) / PI;
    return float2(u, v);
}

// ── Main ──────────────────────────────────────────────────────────────────────

[numthreads(8, 8, 1)]
void main(uint3 thread_id : SV_DispatchThreadID) {
    uint width, height, layers;
    output.GetDimensions(width, height, layers);
    if (thread_id.x >= width || thread_id.y >= height) return;

    float2 uv = float2((float(thread_id.x) + 0.5) / float(width),
                       (float(thread_id.y) + 0.5) / float(height));
    float3 N = uv_to_dir(uv);
    // For IBL prefiltering: V = R = N (isotropic assumption).
    float3 V = N;

    const uint SAMPLES = 256u;
    float3 prefiltered = float3(0.0, 0.0, 0.0);
    float  total_weight = 0.0;

    // Near-mirror (roughness ≈ 0): just sample directly to avoid noise.
    if (roughness < 0.001) {
        prefiltered = env_equirect.SampleLevel(env_sampler, uv, 0.0).rgb;
        output[int3(thread_id.xy, layer_index)] = float4(prefiltered, 1.0);
        return;
    }

    for (uint i = 0u; i < SAMPLES; ++i) {
        float2 xi = hammersley(i, SAMPLES);
        float3 H  = importance_sample_ggx(xi, N, roughness);
        float3 L  = normalize(2.0 * dot(V, H) * H - V);

        float n_dot_l = max(dot(N, L), 0.0);
        if (n_dot_l > 0.0) {
            float2 sample_uv = dir_to_uv(L);
            prefiltered  += env_equirect.SampleLevel(env_sampler, sample_uv, 0.0).rgb * n_dot_l;
            total_weight += n_dot_l;
        }
    }

    if (total_weight > 0.0) {
        prefiltered /= total_weight;
    }

    output[int3(thread_id.xy, layer_index)] = float4(prefiltered, 1.0);
}
