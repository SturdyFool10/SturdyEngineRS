[[vk::binding(0, 0)]] SamplerState g_bindless_samplers[];
[[vk::binding(1, 0)]] Texture2D<float4> g_bindless_textures[];

struct FsIn {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
};

float4 main(FsIn input) : SV_TARGET {
    return g_bindless_textures[NonUniformResourceIndex(0)]
        .Sample(g_bindless_samplers[NonUniformResourceIndex(0)], input.uv);
}
