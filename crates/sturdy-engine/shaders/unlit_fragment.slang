// Built-in unlit fragment shader.
// Visualises the interpolated world-space normal as a colour (remapped from
// [-1,1] to [0,1]).  No lighting, no textures, no push constants required.
//
// Useful for verifying geometry, normals, and the camera/instance pipeline
// without needing to write any shader code.

struct VSOut {
    float4 position  : SV_POSITION;
    float3 normal    : NORMAL;
    float2 uv        : TEXCOORD0;
    float3 world_pos : TEXCOORD1;
};

float4 main(VSOut input) : SV_TARGET {
    float3 n = normalize(input.normal) * 0.5 + 0.5;
    return float4(n, 1.0);
}
