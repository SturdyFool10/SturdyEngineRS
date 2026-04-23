// Textured fragment shader for rendering with UV coordinates and texture
// sampling

struct VSOutput {
  float4 position : SV_POSITION;
  float2 uv : TEXCOORD0;
};

// Texture and sampler for the diffuse map
Texture2D diffuseMap : register(t0, space0);
SamplerState diffuseSampler : register(s1, space0);

float4 ps_main(VSOutput input) : SV_TARGET {
  return diffuseMap.Sample(diffuseSampler, input.uv);
}
