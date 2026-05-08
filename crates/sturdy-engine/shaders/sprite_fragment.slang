// Sprite fragment shader — texture sample with tint and alpha blend.
//
// Samples a 2D texture and multiplies by the per-vertex color tint.
// Alpha channel controls transparency; zero-alpha fragments are discarded
// so sprite batches render correctly without depth sorting for solid sprites.
//
// For sprites without a texture, bind a 1×1 white pixel texture.

Texture2D<float4> sprite_texture;
SamplerState      sprite_sampler;

struct VSOut {
    float4 position : SV_POSITION;
    float2 uv       : TEXCOORD0;
    float4 color    : COLOR0;
};

float4 main(VSOut input) : SV_TARGET {
    float4 tex = sprite_texture.Sample(sprite_sampler, input.uv);
    float4 out_color = tex * input.color;
    if (out_color.a < 1.0 / 255.0) discard;
    return out_color;
}
