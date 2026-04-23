struct VSOut {
  float4 position : SV_POSITION;
  float2 uv : TEXCOORD0;
};

struct FrameGraphConstants {
  float time;
  float aspect;
  float2 resolution;
};

float4 main(VSOut input, uniform FrameGraphConstants graph) : SV_TARGET {
  float2 uv = input.uv * 2.0 - 1.0;
  float radius = length(uv);

  float orbital = sin(graph.time * 1.25 + radius * 5.0);
  float2 tangent = normalize(float2(-uv.y, uv.x) + float2(0.0001, 0.0));
  float2 drift = float2(cos(graph.time * 0.7), sin(graph.time * 0.9)) * 0.0015;
  float2 motion = tangent * orbital * 0.004 + drift;

  float mask = smoothstep(1.35, 0.15, radius);
  return float4(motion * mask, 0.0, 1.0);
}
