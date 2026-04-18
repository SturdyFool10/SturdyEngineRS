// Final composite: Hermite spline tone mapping + bloom accumulation.
// Reads scene_color (linear HDR) and bloom_base (bloom mip-chain base level),
// applies Hermite tone mapping to the scene, adds bloom, and writes to output.

Texture2D<float4> scene_color;
SamplerState scene_sampler;
Texture2D<float4> bloom_base;
SamplerState bloom_sampler;

struct VSOut {
  float4 position : SV_POSITION;
  float2 uv : TEXCOORD0;
};

struct ToneBloomConstants {
  float time;
  float2 inverseResolution;
  float bloomIntensity;
  float3 hdrColor;
  float2 _pad2;
  float _pad3;
};

// Hermite spline tone mapping curve for a single HDR value.
// Produces a smooth S-curve with soft highlight roll-off.
// Based on the Hermite interpolation technique used in modern HDR pipelines.
float hermiteTonemap1(float x) {
  // Hermite spline parameters tuned for HDR → LDR
  const float a = 1.0;  // shoulder point strength
  const float b = 0.5;  // shoulder point position
  const float c = 0.2;  // linear center gain
  const float d = 0.35; // knee point
  const float e = 0.25; // knee width

  // Linear region (below knee)
  if (x < d - e) {
    return c * x;
  }

  // Hermite blend region
  if (x < d + e) {
    float t = (x - (d - e)) / (2.0 * e);
    float t2 = t * t;
    float t3 = t2 * t;
    // Hermite interpolation: (2t³ - 3t² + 1) · p0 + (t³ - 2t² + t) · m0
    //                    + (-2t³ + 3t²) · p1 + (t³ - t²) · m1
    float p0 = c * (d - e);
    float m0 = c;
    float p1 = d + e + (a * e * e) / (1.0 - b);
    float m1 = c + (2.0 * a * e) / (1.0 - b);
    return (2.0 * t3 - 3.0 * t2 + 1.0) * p0 + (t3 - 2.0 * t2 + t) * m0 +
           (-2.0 * t3 + 3.0 * t2) * p1 + (t3 - t2) * m1;
  }

  // Shoulder region: asymptotic approach to max luminance
  float h = max(0.0, x - d);
  return (x + a * h * h) / (1.0 + a * x - b * a * h * h);
}

// Per-component Hermite tone mapping.
float3 hermiteTonemap(float3 x) {
  return float3(hermiteTonemap1(x.r), hermiteTonemap1(x.g),
                hermiteTonemap1(x.b));
}

float4 main(VSOut input, uniform ToneBloomConstants push) : SV_TARGET {
  // Sample scene color (linear HDR)
  float3 scene = scene_color.SampleLevel(scene_sampler, input.uv, 0.0).rgb;

  // Sample bloom (accumulate from mip-chain base level)
  float3 bloom = bloom_base.SampleLevel(bloom_sampler, input.uv, 0.0).rgb;

  // Apply Hermite spline tone mapping to scene
  float3 toneMapped = hermiteTonemap(scene);

  // Add bloom with configurable intensity
  toneMapped += bloom * push.bloomIntensity;

  // Slight vignette for cinematic feel
  float2 uvCenter = input.uv - 0.5;
  float vignette = 1.0 - dot(uvCenter, uvCenter) * 0.15;
  toneMapped *= vignette;

  // Convert to sRGB output
  toneMapped =
      pow(saturate(toneMapped), float3(1.0 / 2.2, 1.0 / 2.2, 1.0 / 2.2));

  return float4(toneMapped, 1.0);
}
