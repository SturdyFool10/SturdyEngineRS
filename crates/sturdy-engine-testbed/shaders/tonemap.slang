// Final display tonemap pass.
// Reads "hdr_composite" (linear HDR, scene + bloom already composited),
// applies a selectable tonemapper, adds a subtle vignette, and encodes for
// the selected output mode.

Texture2D<float4> hdr_composite;
SamplerState hdr_sampler;

struct VSOut {
  float4 position : SV_POSITION;
  float2 uv : TEXCOORD0;
};

struct TonemapConstants {
  uint tonemapOp;
  uint hdrOutput;
  float exposure;
  float whitePoint;
  float displayGain;
  float outputGamma;
  float acesA;
  float acesB;
  float acesC;
  float acesD;
  float acesE;
  float reinhardWhite;
  float hermiteContrast;
  float linearWhite;
};

// ────────────────────────────────────────────────────────────────────────────
// ACES (Stephen Hill filmic approximation)
// ────────────────────────────────────────────────────────────────────────────

float3 aces(float3 x, uniform TonemapConstants p) {
  return saturate((x * (p.acesA * x + p.acesB)) /
                  (x * (p.acesC * x + p.acesD) + p.acesE));
}

float3 normalizedAces(float3 x, uniform TonemapConstants p) {
  float white = max(p.whitePoint, 0.001);
  float3 whiteMapped = max(aces(float3(white, white, white), p), float3(0.001, 0.001, 0.001));
  return saturate(aces(x, p) / whiteMapped);
}

// ────────────────────────────────────────────────────────────────────────────
// Reinhard
// ────────────────────────────────────────────────────────────────────────────

float3 reinhard(float3 x, uniform TonemapConstants p) {
  float white = max(p.reinhardWhite, 0.001);
  return saturate((x * (1.0 + x / (white * white))) / (1.0 + x));
}

// ────────────────────────────────────────────────────────────────────────────
// Hermite
// ────────────────────────────────────────────────────────────────────────────

float3 hermite(float3 x, uniform TonemapConstants p) {
  float white = max(p.whitePoint, 0.001);
  float contrast = max(p.hermiteContrast, 0.001);
  float3 y = saturate(x / white);
  y = pow(y, float3(1.0 / contrast, 1.0 / contrast, 1.0 / contrast));
  return y * y * (3.0 - 2.0 * y);
}

// ────────────────────────────────────────────────────────────────────────────
// Linear pass-through (HDR displays)
// ────────────────────────────────────────────────────────────────────────────

float3 linearMap(float3 x, uniform TonemapConstants p) {
  return saturate(x / max(p.linearWhite, 0.001));
}

// ────────────────────────────────────────────────────────────────────────────
// Khronos PBR Neutral
// Reference: https://github.com/KhronosGroup/ToneMapping/blob/main/PBR_Neutral
//
// Identity below ~0.76 nit, smooth rational compression above, slight
// highlight desaturation. Designed to leave PBR material albedos untouched
// in the display range while gracefully compressing specular peaks.
// ────────────────────────────────────────────────────────────────────────────

float3 pbrNeutral(float3 color) {
  const float startCompression = 0.8 - 0.04; // 0.76
  const float desaturation = 0.15;

  // Lift blacks: small negative offset removes a subtle colour cast that
  // would otherwise appear in deep shadows.
  float x = min(color.r, min(color.g, color.b));
  float offset = x < 0.08 ? x - 6.25 * x * x : 0.04;
  color -= offset;

  float peak = max(color.r, max(color.g, color.b));
  if (peak < startCompression) return color;

  // Rational compression: maps [startCompression, ∞) → [startCompression, 1).
  const float d = 1.0 - startCompression;
  float newPeak = 1.0 - d * d / (peak + d - startCompression);
  color *= newPeak / peak;

  // Highlight desaturation: pull highly-compressed colours toward the achromatic.
  float g = 1.0 - 1.0 / (desaturation * (peak - newPeak) + 1.0);
  return lerp(color, float3(newPeak, newPeak, newPeak), g);
}

// ────────────────────────────────────────────────────────────────────────────
// AgX  (Troy Sobotka / Blender default since 3.x)
// Reference: https://github.com/sobotka/AgX
//
// Steps:
//   1. Rotate scene-linear sRGB into AgX working space (superset of sRGB gamut,
//      handles highly-saturated/out-of-gamut scene values gracefully).
//   2. Log2-normalise so the full exposure range fits [0, 1].
//   3. Apply a per-channel sigmoid (degree-6 polynomial fit to the AgX curve).
//   4. Rotate back to display-linear sRGB.
//
// The matrices below are written row-major for HLSL/Slang mul(M, v) semantics.
// They are the transpose of the column-major GLSL originals.
// ────────────────────────────────────────────────────────────────────────────

static const float3x3 AgX_inset = float3x3(
  0.842479062253094,   0.0784335999999992, 0.0792237451477643,
  0.0423282422610123,  0.878468636469772,  0.0791661274605434,
  0.0423756549057051,  0.0784336,          0.879142973793104
);

static const float3x3 AgX_outset = float3x3(
  1.19687900512017,   -0.0980208811401368, -0.0990297440797205,
 -0.0528968517574562,  1.15190312990417,   -0.0989611768448433,
 -0.0529716355144438, -0.0980434501171241,  1.15107367264116
);

// Degree-6 polynomial sigmoid fitted to the AgX log-normalised contrast curve.
// Valid over [0, 1]. Coefficients from Blender's AgX implementation.
float3 agxSigmoid(float3 x) {
  float3 x2 = x * x;
  float3 x4 = x2 * x2;
  return  15.5      * x4 * x2
        - 40.14     * x4 * x
        + 31.96     * x4
        -  6.868    * x2 * x
        +  0.4298   * x2
        +  0.1191   * x
        -  0.00232;
}

float3 agx(float3 color) {
  // Exposure range in log2 stops (covers scene values from ~2^-12.5 to ~2^4).
  const float minEv = -12.47393;
  const float maxEv =   4.026069;

  // 1. Rotate into AgX working space.
  color = mul(AgX_inset, color);

  // 2. Log2 normalise into [0, 1].  Clamp first to avoid log2(<=0).
  color = clamp(log2(max(color, float3(1e-10, 1e-10, 1e-10))), minEv, maxEv);
  color = (color - minEv) / (maxEv - minEv);

  // 3. Per-channel sigmoid.
  color = saturate(agxSigmoid(color));

  // 4. Rotate back to display-linear sRGB.
  color = mul(AgX_outset, color);

  return saturate(color);
}

// ────────────────────────────────────────────────────────────────────────────
// Dispatch
// ────────────────────────────────────────────────────────────────────────────

float3 applyTonemap(float3 hdr, uniform TonemapConstants p) {
  // Per-operator exposure scaling calibrated so that a neutral grey scene
  // reads at roughly the same perceptual brightness across operators.
  float ev;
  if      (p.tonemapOp == 2) ev = p.exposure * 4.0;  // Hermite
  else if (p.tonemapOp == 1 || p.tonemapOp == 3) ev = p.exposure * 2.0; // Reinhard, Linear
  else    ev = p.exposure;                            // ACES, PBR Neutral, AgX

  float3 x = max(hdr * ev, float3(0.0, 0.0, 0.0));
  float3 color;
  if      (p.tonemapOp == 1) color = reinhard(x, p);
  else if (p.tonemapOp == 2) color = hermite(x, p);
  else if (p.tonemapOp == 3) color = linearMap(x, p);
  else if (p.tonemapOp == 4) color = pbrNeutral(x);
  else if (p.tonemapOp == 5) color = agx(x);
  else                       color = normalizedAces(x, p);

  return saturate(color * p.displayGain);
}

// ────────────────────────────────────────────────────────────────────────────
// Entry point
// ────────────────────────────────────────────────────────────────────────────

float4 main(VSOut input, uniform TonemapConstants push) : SV_TARGET {
  float3 hdr = hdr_composite.SampleLevel(hdr_sampler, input.uv, 0.0).rgb;

  float2 uv = input.uv - 0.5;
  float vignette = 1.0 - dot(uv, uv) * 0.3;
  hdr *= vignette;

  float3 color = applyTonemap(hdr, push);

  if (push.hdrOutput == 0) {
    color = pow(saturate(color), float3(1.0 / max(push.outputGamma, 0.001),
                                        1.0 / max(push.outputGamma, 0.001),
                                        1.0 / max(push.outputGamma, 0.001)));
  }

  return float4(color, 1.0);
}
