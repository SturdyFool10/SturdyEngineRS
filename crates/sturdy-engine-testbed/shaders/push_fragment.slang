struct PSInput {
  float4 position : SV_POSITION;
};

float4 ps_main(PSInput input) : SV_TARGET {
  // Final minimal placeholder fragment shader to allow build completion
  return float4(1.0, 0.5, 0.5, 1.0);
}
