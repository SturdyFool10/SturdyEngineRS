Texture2D<float4> srd_current_signal;
SamplerState srd_current_sampler;
Texture2D<float4> srd_history_signal;
SamplerState srd_history_sampler;

struct VSOut {
  float4 position : SV_POSITION;
  float2 uv : TEXCOORD0;
};

struct SrdTemporalConstants {
  uint frameIndex;
  uint hasHistory;
  uint maxFrames;
  uint mode;
};

float3 sanitizeColor(float3 color) {
  return min(max(color, float3(0.0)), float3(65504.0));
}

float luminance(float3 color) {
  return dot(color, float3(0.2126, 0.7152, 0.0722));
}

float4 main(VSOut input, uniform SrdTemporalConstants push) : SV_TARGET {
  float3 current = sanitizeColor(
      srd_current_signal.SampleLevel(srd_current_sampler, input.uv, 0.0).rgb);
  float currentLum = luminance(current);
  float currentMoment2 = currentLum * currentLum;
  if (push.hasHistory == 0 || push.frameIndex == 0)
    return float4(current, currentMoment2);

  float4 historySample =
      srd_history_signal.SampleLevel(srd_history_sampler, input.uv, 0.0);
  float3 history = sanitizeColor(historySample.rgb);
  float historyMoment2 = max(historySample.a, 0.0);
  // Running average capped by maxFrames. The cap keeps very long sessions from
  // driving the update weight below useful precision, which can otherwise make
  // bright emitters slowly lose bloom as history becomes numerically stale.
  float effectiveFrame =
      min(float(push.frameIndex), max(float(push.maxFrames), 1.0) - 1.0);
  float blend = 1.0 / (effectiveFrame + 1.0);
  return float4(sanitizeColor(lerp(history, current, blend)),
                lerp(historyMoment2, currentMoment2, blend));
}
