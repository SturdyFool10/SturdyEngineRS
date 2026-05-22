Texture2D<float4> current_sample;
SamplerState current_sampler;
Texture2D<float4> accumulation_history;
SamplerState accumulation_sampler;

struct VSOut {
  float4 position : SV_POSITION;
  float2 uv : TEXCOORD0;
};

struct CornellAccumulationConstants {
  uint frameIndex;
  uint hasHistory;
  uint maxFrames;
  uint _pad;
};

float4 main(VSOut input, uniform CornellAccumulationConstants push)
    : SV_TARGET {
  float3 current =
      current_sample.SampleLevel(current_sampler, input.uv, 0.0).rgb;
  if (push.hasHistory == 0 || push.frameIndex == 0)
    return float4(current, 1.0);

  float3 history =
      accumulation_history.SampleLevel(accumulation_sampler, input.uv, 0.0).rgb;
  float sampleCount =
      min(float(push.frameIndex), float(max(push.maxFrames, 1u)));
  float blend = 1.0 / (sampleCount + 1.0);
  return float4(lerp(history, current, blend), 1.0);
}
