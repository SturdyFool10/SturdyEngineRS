struct VSInput {
  float3 position : POSITION;
};

struct VSOutput {
  float4 position : SV_POSITION;
};

struct PushConstants {
  float4 scale_bias;
  float4 color;
};

VSOutput vs_main(VSInput input, uniform PushConstants push) {
  VSOutput output;
  output.position = float4(input.position * push.scale_bias.xyz + push.scale_bias.www, 1.0);
  return output;
}
