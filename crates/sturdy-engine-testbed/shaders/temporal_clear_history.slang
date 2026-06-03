struct VSOut {
  float4 position : SV_POSITION;
  float2 uv : TEXCOORD0;
};

float4 main(VSOut input) : SV_TARGET { return float4(0.0, 0.0, 0.0, 0.0); }
