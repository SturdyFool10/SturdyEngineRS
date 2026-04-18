struct VSInput {
    float3 position : POSITION;
    float3 normal : NORMAL;
    float2 uv : TEXCOORD0;
};

struct VSOut {
    float4 position : SV_POSITION;
    float3 normal : NORMAL;
    float2 uv : TEXCOORD0;
    float3 world_pos : TEXCOORD1;
};

VSOut main(VSInput input) {
    VSOut o;
    o.position = float4(input.position, 1.0);
    o.normal = input.normal;
    o.uv = input.uv;
    o.world_pos = input.position;
    return o;
}
