// Ground Truth Ambient Occlusion (GTAO) compute shader.
//
// Implements the horizon-based ambient occlusion algorithm from
// Jimenez et al. 2016, with optimisations from the Intel XeGTAO
// implementation (2021) and Epic's Lumen adaptation.
//
// ## Algorithm
//
// For each pixel P:
//   1. Reconstruct view-space position from the depth buffer.
//   2. Decode view-space normal from the G-Buffer.
//   3. For each angular slice (N = slice_count):
//        a. March K steps (steps_per_slice) in screen space along
//           the slice direction, sampling depth at each step.
//        b. For each depth sample, compute the elevation angle to the
//           view-space horizon candidate.
//        c. Track the maximum horizon angle in both +/- directions.
//        d. Integrate: ao_slice = 1 - 0.5*(sin(h+)+sin(h-)) where
//           h± are the horizon angles clamped against the normal angle.
//   4. Accumulate and normalise over all slices.
//   5. Apply falloff, intensity, and write to the AO image.
//
// ## G-Buffer layout expected
//
//   gbuffer_depth        Texture2D<float>  — hardware NDC depth [0, 1]
//   gbuffer_normal_rough Texture2D<float4> — oct-packed normal in xy,
//                                            roughness in z, 0 in w
//
// ## Output
//
//   gtao_out  RWTexture2D<float>  — R32Float AO value [0, 1],
//                                   1 = no occlusion, 0 = fully occluded

Texture2D<float>  gbuffer_depth;
Texture2D<float4> gbuffer_normal_rough;
SamplerState      gtao_sampler;
RWTexture2D<float> gtao_out;

struct GtaoConstants {
    float4x4 inv_proj;          // inverse projection matrix
    float4x4 inv_view;          // inverse view matrix (world = inv_view * view_pos)
    float4x4 view;              // view matrix
    float4   viewport;          // xy = 1/width, 1/height; zw = width, height
    float    radius_world;      // world-space AO search radius
    float    falloff_start;     // start of falloff zone (0..1 * radius)
    float    falloff_end;       // end of falloff zone (0..1 * radius)
    float    intensity;         // multiplier on the final AO value
    uint     slice_count;       // number of angular slices (default 8)
    uint     steps_per_slice;   // depth samples per slice (default 8)
    float    _pad0;
    float    _pad1;
};
uniform GtaoConstants push;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Reconstruct view-space position from a hardware NDC depth value.
float3 reconstruct_view_pos(float2 uv, float ndc_depth) {
    float4 ndc   = float4(uv * 2.0 - 1.0, ndc_depth, 1.0);
    float4 vs    = mul(push.inv_proj, ndc);
    return vs.xyz / vs.w;
}

/// Decode oct-encoded normal stored in xy of the G-Buffer normal+rough texture.
float3 decode_normal_gbuffer(float2 enc) {
    float3 n;
    n.xy = enc * 2.0 - 1.0;
    n.z  = sqrt(max(0.0, 1.0 - dot(n.xy, n.xy)));
    // G-Buffer normals are world-space; transform to view-space for GTAO.
    return normalize(mul((float3x3)push.view, n));
}

/// Smooth falloff between [near, far]. Returns 1 at near, 0 at far.
float falloff(float dist, float near, float far) {
    return 1.0 - smoothstep(near, far, dist);
}

// ── GTAO main ─────────────────────────────────────────────────────────────────

[numthreads(8, 8, 1)]
void main(uint3 tid : SV_DispatchThreadID) {
    uint2 dims = uint2(push.viewport.zw);
    if (any(tid.xy >= dims)) return;

    float2 pixel  = float2(tid.xy) + 0.5;
    float2 uv     = pixel * push.viewport.xy;          // [0,1] screen UV
    float2 inv_res = push.viewport.xy;

    // Sample depth and decode position and normal.
    float depth_ndc = gbuffer_depth.Load(int3(tid.xy, 0)).r;
    if (depth_ndc >= 0.9999) {                         // sky / background
        gtao_out[tid.xy] = 1.0;
        return;
    }

    float3 P_vs = reconstruct_view_pos(uv, depth_ndc); // view-space position
    float2 enc_n = gbuffer_normal_rough.Load(int3(tid.xy, 0)).xy;
    float3 N_vs  = decode_normal_gbuffer(enc_n);       // view-space normal

    // View direction (from pixel toward camera origin in view space).
    float3 V_vs = normalize(-P_vs);

    // Screen-space projection of the search radius at this depth.
    // radius_ss ≈ radius_world / view_depth * focal_length_pixels
    float view_depth = -P_vs.z;                        // positive depth along -Z
    float focal_px   = push.viewport.z * abs(push.inv_proj[0][0]); // fx in pixels
    float radius_ss  = push.radius_world / max(view_depth, 0.001) * focal_px;
    radius_ss = clamp(radius_ss, 2.0, 128.0);          // clamp to [2, 128] px

    float step_px = radius_ss / float(push.steps_per_slice);

    float ao_accum = 0.0;
    uint  N        = push.slice_count;

    // Rotate slices by a spatially-varying offset to reduce banding.
    float slice_offset = frac(sin(dot(float2(tid.xy), float2(13.759, 23.341))) * 1e5);

    for (uint i = 0; i < N; ++i) {
        float phi = (float(i) + slice_offset) / float(N) * 3.14159265;
        float2 dir_ss = float2(cos(phi), -sin(phi));   // screen-space direction

        // Horizon angles in the positive and negative half-slices.
        float h_pos = -1.5707963;   // -PI/2  (sin = -1)
        float h_neg = -1.5707963;

        for (uint j = 1; j <= push.steps_per_slice; ++j) {
            float jitter = frac(sin(dot(float2(tid.xy) + float2(i, j), float2(47.1, 83.7))) * 1e5);
            float step   = (float(j) - 0.5 + jitter * 0.5) * step_px;

            // Sample in +dir direction.
            float2 uv_pos = uv + dir_ss * step * inv_res;
            if (all(uv_pos >= 0.0) && all(uv_pos <= 1.0)) {
                float2 px_pos = uv_pos / inv_res;
                float d_pos   = gbuffer_depth.SampleLevel(
                    gtao_sampler, uv_pos, 0).r;
                float3 S_pos  = reconstruct_view_pos(uv_pos, d_pos);
                float3 H_pos  = S_pos - P_vs;
                float  dist   = length(H_pos);
                float  w      = falloff(dist, push.radius_world * push.falloff_start,
                                              push.radius_world * push.falloff_end);
                float  angle  = asin(clamp(H_pos.z / max(dist, 0.001), -1.0, 1.0));
                h_pos         = max(h_pos, lerp(h_pos, angle, w));
            }

            // Sample in -dir direction.
            float2 uv_neg = uv - dir_ss * step * inv_res;
            if (all(uv_neg >= 0.0) && all(uv_neg <= 1.0)) {
                float d_neg  = gbuffer_depth.SampleLevel(
                    gtao_sampler, uv_neg, 0).r;
                float3 S_neg = reconstruct_view_pos(uv_neg, d_neg);
                float3 H_neg = S_neg - P_vs;
                float  dist  = length(H_neg);
                float  w     = falloff(dist, push.radius_world * push.falloff_start,
                                             push.radius_world * push.falloff_end);
                float  angle = asin(clamp(H_neg.z / max(dist, 0.001), -1.0, 1.0));
                h_neg        = max(h_neg, lerp(h_neg, angle, w));
            }
        }

        // Integrate this slice.
        // The projected angle of the normal onto this slice plane gives theta_N.
        float2 slice_dir3d = float2(cos(phi), sin(phi));
        float  n_proj = dot(N_vs.xy, slice_dir3d);     // projection coefficient
        float  theta_n = asin(clamp(n_proj, -1.0, 1.0));

        // Clamp horizon angles against the hemisphere defined by the normal.
        float h1 = max(h_pos, theta_n - 1.5707963);
        float h2 = max(h_neg, theta_n - 1.5707963);

        // AO contribution = 1 - visible hemisphere fraction above the horizon.
        // Using sin of angles (= height component normalised to sphere surface).
        float ao_slice = 1.0 - 0.5 * (sin(h_pos) + sin(h_neg));
        // Modulate by normal-hemisphere weight: less AO for directions tangent to N.
        ao_slice *= max(0.0, dot(N_vs, V_vs));

        ao_accum += ao_slice;
    }

    float ao = saturate(ao_accum / float(N));
    ao = pow(ao, push.intensity);                       // intensity (gamma-style)

    gtao_out[tid.xy] = ao;
}
