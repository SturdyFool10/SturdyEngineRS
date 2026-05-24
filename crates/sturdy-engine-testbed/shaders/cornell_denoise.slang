Texture2D<float4> accumulated_radiance;
Texture2D<float4> current_guides;
Texture2D<float4> current_material_guides;

struct VSOut {
  float4 position : SV_POSITION;
  float2 uv : TEXCOORD0;
};

struct CornellDenoiseConstants {
  uint frameIndex;
  uint maxFrames;
  uint outputMode;
  uint _pad;
};

struct CornellGuide {
  float3 normal;
  float depth;
};

struct CornellMaterialGuide {
  float3 albedo;
  float material;
};

struct CornellSample {
  float3 color;
  float moment2;
};

float luminance(float3 color) {
  return dot(color, float3(0.2126, 0.7152, 0.0722));
}

int2 clampPixel(int2 pixel, int2 dims) {
  return clamp(pixel, int2(0, 0), max(dims - int2(1, 1), int2(0, 0)));
}

float3 sanitizeColor(float3 color) {
  return min(max(color, float3(0.0)), float3(65504.0));
}

float3 logRadiance(float3 color) {
  return log2(float3(1.0) + sanitizeColor(color));
}

float frameCount(CornellDenoiseConstants push) {
  return min(float(push.frameIndex) + 1.0, max(float(push.maxFrames), 1.0));
}

float historyConfidence(CornellDenoiseConstants push) {
  return saturate(log2(frameCount(push) + 1.0) /
                  log2(max(float(push.maxFrames), 1.0) + 1.0));
}

CornellSample loadSample(int2 pixel, int2 dims) {
  float4 encoded = accumulated_radiance.Load(int3(clampPixel(pixel, dims), 0));
  CornellSample s;
  s.color = sanitizeColor(encoded.rgb);
  s.moment2 = max(encoded.a, 0.0);
  return s;
}

float3 loadColor(int2 pixel, int2 dims) {
  return loadSample(pixel, dims).color;
}

CornellGuide loadGuide(int2 pixel, int2 dims) {
  float4 encoded = current_guides.Load(int3(clampPixel(pixel, dims), 0));
  CornellGuide g;
  g.normal = encoded.xyz * 2.0 - 1.0;
  float len = length(g.normal);
  g.normal = len > 1e-4 ? g.normal / len : float3(0.0, 0.0, 1.0);
  g.depth = encoded.w;
  return g;
}

CornellMaterialGuide loadMaterialGuide(int2 pixel, int2 dims) {
  float4 encoded = current_material_guides.Load(int3(clampPixel(pixel, dims), 0));
  CornellMaterialGuide m;
  m.albedo = saturate(encoded.rgb);
  m.material = encoded.a;
  return m;
}

bool isBackground(CornellGuide g, CornellMaterialGuide m) {
  return g.depth >= 99.0 || m.material >= 250.0;
}

// Separable B3-spline (Mitchell–Netravali like) kernel weights.
float kernelWeight1D(int tap) {
  int at = abs(tap);
  if (at == 0) return 0.375;
  if (at == 1) return 0.25;
  return 0.0625;
}

// Edge-stopping weight from geometry + material guides.
// normalSigma controls how quickly the normal weight drops off (higher = stricter).
float guideWeight(CornellGuide cg, CornellMaterialGuide cm,
                  CornellGuide sg, CornellMaterialGuide sm,
                  int stepWidth) {
  if (isBackground(sg, sm))
    return 0.0;

  // Normal: flat-faced Cornell walls share identical normals so we can afford
  // a tight exponent; this also sharpens box edges cleanly.
  float normalWeight = pow(saturate(dot(cg.normal, sg.normal)), 32.0);

  // Depth: sigma scales with scene depth and filter step to stay stable.
  float depthSigma = max(0.02, cg.depth * (0.025 + 0.012 * float(stepWidth)));
  float depthWeight = exp(-abs(sg.depth - cg.depth) / depthSigma);

  // Albedo: tighter than before so red/green/white walls don't bleed.
  float3 albedoDelta = sm.albedo - cm.albedo;
  float albedoWeight = exp(-dot(albedoDelta, albedoDelta) * 12.0);

  float materialWeight = abs(sm.material - cm.material) < 0.25 ? 1.0 : 0.0;

  return normalWeight * depthWeight * albedoWeight * materialWeight;
}

float temporalVariance(CornellSample s, CornellDenoiseConstants push) {
  float meanLum = luminance(s.color);
  float rawVariance = max(s.moment2 - meanLum * meanLum, 0.0);
  return rawVariance / max(frameCount(push), 1.0);
}

float spatialLogVariance(int2 pixel, int2 dims, CornellGuide cg, CornellMaterialGuide cm,
                         float centerLum) {
  float sum = centerLum;
  float sum2 = centerLum * centerLum;
  float weightSum = 1.0;

  [unroll]
  for (int y = -1; y <= 1; ++y) {
    [unroll]
    for (int x = -1; x <= 1; ++x) {
      if (x == 0 && y == 0)
        continue;
      int2 sp = pixel + int2(x, y);
      CornellGuide sg = loadGuide(sp, dims);
      CornellMaterialGuide sm = loadMaterialGuide(sp, dims);
      float w = guideWeight(cg, cm, sg, sm, 1);
      if (w <= 1e-4)
        continue;
      float lum = luminance(logRadiance(loadColor(sp, dims)));
      sum += lum * w;
      sum2 += lum * lum * w;
      weightSum += w;
    }
  }

  float mean = sum / max(weightSum, 1e-4);
  return max(sum2 / max(weightSum, 1e-4) - mean * mean, 0.0);
}

// 5x5 A-trous pass with geometry + luminance bilateral weights.
// lumaSigma gates luminance similarity; tighter = less color bleeding.
float3 guidedAtrous(int2 pixel, int2 dims, int stepWidth,
                    CornellGuide cg, CornellMaterialGuide cm,
                    float lumaSigma, out float filterTrust) {
  float3 centerColor = loadColor(pixel, dims);
  float centerLum = luminance(centerColor);
  float3 sum = float3(0.0);
  float weightSum = 0.0;
  float neighborWeightSum = 0.0;

  [unroll]
  for (int y = -2; y <= 2; ++y) {
    [unroll]
    for (int x = -2; x <= 2; ++x) {
      int2 sp = pixel + int2(x, y) * stepWidth;
      float spatial = kernelWeight1D(x) * kernelWeight1D(y);
      CornellGuide sg = loadGuide(sp, dims);
      CornellMaterialGuide sm = loadMaterialGuide(sp, dims);
      float edge = guideWeight(cg, cm, sg, sm, stepWidth);
      if (edge <= 1e-5)
        continue;

      float3 sc = loadColor(sp, dims);
      float sLum = luminance(sc);
      // Bilateral luminance weight stops filtering from blending across large
      // radiance differences (e.g. directly-lit vs shadow regions).
      float lumaWeight = exp(-abs(sLum - centerLum) / max(lumaSigma, 0.04));

      float w = spatial * edge * lumaWeight;
      sum += sc * w;
      weightSum += w;
      if (x != 0 || y != 0)
        neighborWeightSum += w;
    }
  }

  filterTrust = saturate(neighborWeightSum / 0.20);
  return weightSum > 1e-4 ? sanitizeColor(sum / weightSum) : centerColor;
}

float3 denoiseColor(int2 pixel, CornellDenoiseConstants push,
                    out float blendOut, out float noiseOut) {
  uint width, height;
  accumulated_radiance.GetDimensions(width, height);
  int2 dims = int2(int(max(width, 1u)), int(max(height, 1u)));

  CornellSample centerSample = loadSample(pixel, dims);
  float3 center = centerSample.color;
  CornellGuide cg = loadGuide(pixel, dims);
  CornellMaterialGuide cm = loadMaterialGuide(pixel, dims);
  if (isBackground(cg, cm)) {
    blendOut = 0.0;
    noiseOut = 0.0;
    return center;
  }

  float confidence = historyConfidence(push);
  float centerLum = luminance(logRadiance(center));
  float temporalVar = temporalVariance(centerSample, push);
  float spatialVar = spatialLogVariance(pixel, dims, cg, cm, centerLum);
  float relTemporalNoise = sqrt(max(temporalVar, 0.0)) /
                           max(luminance(center) + 0.05, 0.05);
  float noiseAmount = saturate(relTemporalNoise * 5.0 + sqrt(spatialVar) * 1.4);

  // Luminance sigma: looser early on so coarse structure converges quickly,
  // tighter once we have many samples to preserve fine-detail contrast.
  float lumaSigma = lerp(0.60, 0.12, confidence) * max(centerLum + 0.1, 0.1);

  // Five A-trous passes: step 1 (fine detail) → 16 (large low-freq patches).
  // Extra passes dramatically reduce the splotchy low-frequency noise that a
  // 3-pass filter leaves behind in diffuse indirect lighting.
  float t1, t2, t4, t8, t16;
  float3 f1  = guidedAtrous(pixel, dims,  1, cg, cm, lumaSigma, t1);
  float3 f2  = guidedAtrous(pixel, dims,  2, cg, cm, lumaSigma, t2);
  float3 f4  = guidedAtrous(pixel, dims,  4, cg, cm, lumaSigma, t4);
  float3 f8  = guidedAtrous(pixel, dims,  8, cg, cm, lumaSigma, t8);
  float3 f16 = guidedAtrous(pixel, dims, 16, cg, cm, lumaSigma, t16);

  // Progressive blend: coarser passes blend in proportionally to noise level
  // and their own neighbourhood trust, keeping sharp detail on quiet pixels.
  float3 filtered = f1;
  filtered = lerp(filtered, f2,  saturate(noiseAmount * 0.72) * t2);
  filtered = lerp(filtered, f4,  saturate(noiseAmount * 0.58) * t4);
  filtered = lerp(filtered, f8,  saturate(noiseAmount * 0.42) * t8);
  filtered = lerp(filtered, f16, saturate(noiseAmount * 0.28) * t16);

  float trustMax = max(t1, max(t2 * 0.88, max(t4 * 0.75, max(t8 * 0.60, t16 * 0.44))));
  float strength = (0.18 + noiseAmount * lerp(0.88, 0.38, confidence)) * trustMax;
  blendOut = min(saturate(strength), lerp(0.97, 0.52, confidence));
  noiseOut = noiseAmount;
  return sanitizeColor(lerp(center, filtered, blendOut));
}

float4 main(VSOut input, uniform CornellDenoiseConstants push) : SV_TARGET {
  int2 pixel = int2(input.position.xy);
  float blend, noise;
  float3 denoised = denoiseColor(pixel, push, blend, noise);

  if (push.outputMode == 1u) {
    uint width, height;
    accumulated_radiance.GetDimensions(width, height);
    int2 dims = int2(int(max(width, 1u)), int(max(height, 1u)));
    float3 center = loadColor(pixel, dims);
    float3 delta = abs(logRadiance(denoised) - logRadiance(center)) * 6.0;
    return float4(min(delta + float3(blend * 0.15, noise * 0.15, 0.0), float3(12.0)), 1.0);
  }

  return float4(denoised, 1.0);
}
