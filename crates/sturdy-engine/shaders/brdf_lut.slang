// BRDF integration LUT compute shader.
//
// Precomputes the split-sum BRDF integration table used by IBL specular:
//
//   ∫ f(l,v) * cos(θ_l) dl = F0 * R + G * (1 - F0)
//
// Output: RG channels of a 128×128 Rgba16Float image.
//   R = BRDF scale  (multiply by F0)
//   G = BRDF bias   (add regardless of F0)
//
// Reference: "Real Shading in Unreal Engine 4" — Epic 2013

RWTexture2D<float4> output;

// ── Low-discrepancy sampling ──────────────────────────────────────────────────

float radical_inverse_vdc(uint bits) {
    bits = (bits << 16u) | (bits >> 16u);
    bits = ((bits & 0x55555555u) << 1u) | ((bits & 0xAAAAAAAAu) >> 1u);
    bits = ((bits & 0x33333333u) << 2u) | ((bits & 0xCCCCCCCCu) >> 2u);
    bits = ((bits & 0x0F0F0F0Fu) << 4u) | ((bits & 0xF0F0F0F0u) >> 4u);
    bits = ((bits & 0x00FF00FFu) << 8u) | ((bits & 0xFF00FF00u) >> 8u);
    return float(bits) * 2.3283064365386963e-10; // / 0x100000000
}

float2 hammersley(uint i, uint n) {
    return float2(float(i) / float(n), radical_inverse_vdc(i));
}

// ── GGX importance sampling ───────────────────────────────────────────────────

float3 importance_sample_ggx(float2 xi, float roughness) {
    float a = roughness * roughness;
    float phi = 2.0 * 3.14159265 * xi.x;
    float cos_theta = sqrt((1.0 - xi.y) / (1.0 + (a * a - 1.0) * xi.y));
    float sin_theta = sqrt(1.0 - cos_theta * cos_theta);
    return float3(sin_theta * cos(phi), sin_theta * sin(phi), cos_theta);
}

// Smith G term (Schlick-GGX for IBL)
float G_schlick(float n_dot_v, float roughness) {
    float k = (roughness * roughness) / 2.0;
    return n_dot_v / (n_dot_v * (1.0 - k) + k);
}

float G_smith(float n_dot_v, float n_dot_l, float roughness) {
    return G_schlick(n_dot_v, roughness) * G_schlick(n_dot_l, roughness);
}

// ── Main ──────────────────────────────────────────────────────────────────────

[numthreads(8, 8, 1)]
void main(uint3 thread_id : SV_DispatchThreadID) {
    uint width, height;
    output.GetDimensions(width, height);
    if (thread_id.x >= width || thread_id.y >= height) return;

    float n_dot_v  = (float(thread_id.x) + 0.5) / float(width);
    float roughness = (float(thread_id.y) + 0.5) / float(height);

    // View vector in tangent space (N = (0,0,1)).
    float3 V = float3(sqrt(1.0 - n_dot_v * n_dot_v), 0.0, n_dot_v);
    float3 N = float3(0.0, 0.0, 1.0);

    const uint SAMPLES = 1024u;
    float scale = 0.0, bias = 0.0;

    for (uint i = 0u; i < SAMPLES; ++i) {
        float2 xi = hammersley(i, SAMPLES);
        float3 H  = importance_sample_ggx(xi, roughness);
        float3 L  = normalize(2.0 * dot(V, H) * H - V);

        float n_dot_l = max(L.z, 0.0);
        float n_dot_h = max(H.z, 0.0);
        float v_dot_h = max(dot(V, H), 0.0);

        if (n_dot_l > 0.0) {
            float G     = G_smith(n_dot_v, n_dot_l, roughness);
            float G_vis = (G * v_dot_h) / (n_dot_h * n_dot_v);
            float Fc    = pow(1.0 - v_dot_h, 5.0);
            scale += (1.0 - Fc) * G_vis;
            bias  += Fc * G_vis;
        }
    }
    scale /= float(SAMPLES);
    bias  /= float(SAMPLES);

    output[thread_id.xy] = float4(scale, bias, 0.0, 1.0);
}
