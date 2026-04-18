struct VSInput {
    float2 position : POSITION;
    float2 uv : TEXCOORD0;
    float4 color : COLOR0;
};

struct VSOut {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
    float4 color : COLOR0;
};

VSOut main(VSInput input) {
    VSOut o;
    o.position = float4(input.position, 0.0, 1.0);
    o.uv = input.uv;
    o.color = input.color;
    return o;
}
