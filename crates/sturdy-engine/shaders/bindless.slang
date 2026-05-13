// bindless.slang — global bindless descriptor declarations (Track 8a).
//
// Include this file at the top of any shader that uses bindless resources.
// The bindless descriptor set MUST be bound at set 0 in the pipeline layout.
//
// ## Descriptor layout (set 0)
//
//   binding 0  — SamplerState[]     (up to 128 samplers)
//   binding 1  — Texture2D[]        (up to 16 384 sampled images)
//   binding 2  — RWTexture2D[]      (up to 1 024 storage images)
//   binding 3  — ByteAddressBuffer[] (up to 4 096 storage buffers)
//
// ## Usage
//
// ```slang
// #include "bindless.slang"
//
// struct MyConstants {
//     uint texture_index;
//     uint sampler_index;
// };
//
// float4 fragment_main(VSOut v, uniform MyConstants push) : SV_TARGET {
//     return g_bindless_textures[NonUniformResourceIndex(push.texture_index)]
//                .Sample(g_bindless_samplers[push.sampler_index], v.uv);
// }
// ```
//
// ## NonUniformResourceIndex
//
// When the index is not dynamically uniform (different per lane/thread), wrap
// it in `NonUniformResourceIndex(...)`. This emits the correct SPIR-V
// NonUniform decoration and avoids undefined behaviour.
// For draw-call-level indices (same for all fragments in a draw), the
// decoration is technically not required but is harmless and recommended.
//
// ## Fallback
//
// On hardware/drivers that do not support descriptor_indexing, the bindless
// descriptor set is not created and `Engine::bindless_supported()` returns
// false. Shaders that include this file must not be used in that case —
// check `Caps::supports_bindless` and use the traditional per-draw descriptor
// set path as a fallback.

[[vk::binding(0, 0)]] SamplerState       g_bindless_samplers[];
[[vk::binding(1, 0)]] Texture2D<float4>  g_bindless_textures[];
[[vk::binding(2, 0)]] RWTexture2D<float4> g_bindless_storage_images[];
[[vk::binding(3, 0)]] ByteAddressBuffer  g_bindless_buffers[];

// ── Helper functions ──────────────────────────────────────────────────────────

/// Sample a bindless texture at the given LOD-0 UV.
float4 bindless_sample(uint tex_idx, uint smp_idx, float2 uv) {
    return g_bindless_textures[NonUniformResourceIndex(tex_idx)]
               .Sample(g_bindless_samplers[NonUniformResourceIndex(smp_idx)], uv);
}

/// Sample a bindless texture with explicit LOD.
float4 bindless_sample_lod(uint tex_idx, uint smp_idx, float2 uv, float lod) {
    return g_bindless_textures[NonUniformResourceIndex(tex_idx)]
               .SampleLevel(g_bindless_samplers[NonUniformResourceIndex(smp_idx)], uv, lod);
}

/// Load a float4 from a bindless ByteAddressBuffer at byte offset.
float4 bindless_load_float4(uint buf_idx, uint byte_offset) {
    return asfloat(g_bindless_buffers[NonUniformResourceIndex(buf_idx)]
               .Load4(byte_offset));
}

/// Load a uint from a bindless ByteAddressBuffer at byte offset.
uint bindless_load_uint(uint buf_idx, uint byte_offset) {
    return g_bindless_buffers[NonUniformResourceIndex(buf_idx)].Load(byte_offset);
}
