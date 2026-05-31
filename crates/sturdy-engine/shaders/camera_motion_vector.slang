// Camera-local motion vector generation.
//
// Reconstructs the world-space position of each G-Buffer pixel from depth,
// then reprojects it into the previous frame's clip space to produce a
// UV-space velocity for camera motion.
//
// Does NOT account for per-object motion — objects that moved between frames
// will have camera-only velocity assigned. G-Buffer MRT motion vectors from the
// vertex shader are needed for correct per-object temporal reprojection.
//
// ## Output convention
//
//   motion = prev_uv_unjittered - curr_uv_unjittered
//
// Both UVs are in unjittered screen UV space [0, 1].  This matches the TAA
// reprojection convention in anti_aliasing.slang:
//
//   history_sample_uv = curr_uv + jitter_delta + motion
//
// For a static camera, motion == 0 and the jitter_delta term alone corrects
// for sub-pixel alignment.  For a moving camera the motion term captures the
// bulk screen-space displacement so the history samples stay aligned.

Texture2D<float>  gbuffer_depth;
SamplerState      depth_sampler;

struct CameraMotionConstants {
    float4x4 inv_proj;       // inverse of current-frame unjittered projection
    float4x4 inv_view;       // inverse of current-frame view (view → world)
    float4x4 prev_view_proj; // previous-frame jittered view-projection
    float2   jitter_curr;    // current-frame jitter in UV space [0, 1]
    float2   jitter_prev;    // previous-frame jitter in UV space [0, 1]
};

struct VSOut {
    float4 position : SV_POSITION;
    float2 uv       : TEXCOORD0;
};

float2 main(VSOut input, uniform CameraMotionConstants cam) : SV_TARGET {
    float2 uv = input.uv;

    // Skip sky / background pixels (hardware NDC depth == 1.0 for far plane).
    float depth = gbuffer_depth.SampleLevel(depth_sampler, uv, 0.0);
    if (depth >= 0.9999) {
        return float2(0.0, 0.0);
    }

    // Convert rendered UV to unjittered NDC.
    // The projection jitter shifts NDC by jitter * 2 in each axis; undo that.
    // The depth value is unaffected by the jitter (jitter only shifts xy).
    float2 ndc_unjit = uv * 2.0 - 1.0 - cam.jitter_curr * 2.0;
    float2 uv_curr_unjit = ndc_unjit * 0.5 + 0.5;

    // Reconstruct view-space position using the unjittered inverse projection.
    float4 clip = float4(ndc_unjit.x, ndc_unjit.y, depth, 1.0);
    float4 view = mul(cam.inv_proj, clip);
    view /= view.w;

    // Transform to world space.
    float4 world = mul(cam.inv_view, float4(view.xyz, 1.0));

    // Reproject into previous-frame jittered clip space.
    float4 prev_clip = mul(cam.prev_view_proj, world);
    float2 prev_ndc_jit = prev_clip.xy / prev_clip.w;
    float2 prev_uv_jit  = prev_ndc_jit * 0.5 + 0.5;

    // Remove previous-frame jitter to get the unjittered previous UV.
    float2 prev_uv_unjit = prev_uv_jit - cam.jitter_prev;

    // Motion = unjittered delta (prev − curr).
    // Adding this to curr_uv + jitter_delta in the TAA shader yields the
    // previous-frame jittered UV, which is where the history was written.
    return prev_uv_unjit - uv_curr_unjit;
}
