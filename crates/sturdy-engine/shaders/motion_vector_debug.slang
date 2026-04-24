Texture2D<float4> motion_source;
SamplerState motion_sampler;

struct VSOut {
  float4 position : SV_POSITION;
  float2 uv : TEXCOORD0;
};

float4 main(VSOut input) : SV_TARGET {
  float2 motion = motion_source.SampleLevel(motion_sampler, input.uv, 0.0).xy;
  float magnitude = saturate(length(motion) * 160.0);
  float2 direction = motion * 80.0;
  float3 color = float3(0.5 + direction.x, 0.5 - direction.y, magnitude);
  return float4(saturate(color), 1.0);
}
