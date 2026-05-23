[[vk::binding(0, 0)]]
RWTexture2D<float4> output_image;
[[vk::binding(1, 0)]]
RaytracingAccelerationStructure scene_tlas;

struct RtConstants {
  uint frameIndex;
  uint width;
  uint height;
  float aspect;
};
[[vk::binding(2, 0)]]
ConstantBuffer<RtConstants> rt_constants;

struct RayPayload {
  float3 radiance;
  float3 albedo;
  float3 normal;
  float hitT;
  uint material;
  uint hit;
  uint seed;
};

struct SpectralPhoton {
  float3 position;
  float3 normal;
  float3 incidentDirection;
  float throughput;
  float geometry;
};

struct SpectralPathResult {
  float throughput;
  float radiance;
};

static const float PI = 3.14159265359;
static const uint MATERIAL_DIFFUSE = 0u;
static const uint MATERIAL_LIGHT = 1u;
static const uint MATERIAL_GLASS = 2u;
static const float3 GLASS_CENTER = float3(0.0, 0.84, 0.34);
static const float GLASS_RADIUS = 0.32;
static const uint MAX_PATH_BOUNCES = 10u;
static const uint PHOTON_MAP_LIGHT_SUBPATH_SAMPLES = 8u;
static const float PHOTON_MAP_GATHER_RADIUS = 0.10;
static const float DIFFUSE_GLASS_GUIDE_PROBABILITY = 0.35;
static const float SPEED_OF_LIGHT_NM_THZ = 299792.458;
static const float VISIBLE_WAVELENGTH_MIN_NM = 380.0;
static const float VISIBLE_WAVELENGTH_MAX_NM = 740.0;
static const float VISIBLE_FREQUENCY_MIN_THZ =
    SPEED_OF_LIGHT_NM_THZ / VISIBLE_WAVELENGTH_MAX_NM;
static const float VISIBLE_FREQUENCY_MAX_THZ =
    SPEED_OF_LIGHT_NM_THZ / VISIBLE_WAVELENGTH_MIN_NM;
static const float RED_GREEN_BOUNDARY_THZ = SPEED_OF_LIGHT_NM_THZ / 585.0;
static const float GREEN_BLUE_BOUNDARY_THZ = SPEED_OF_LIGHT_NM_THZ / 505.0;
// Frequencies are sampled from three equal-probability RGB bands. Multiplying
// each single-band contribution by 3 keeps RGB scene colors energy-preserving
// under progressive random-frequency accumulation.
static const float SPECTRAL_RGB_RECONSTRUCTION_WEIGHT = 3.0;
// Clear high-dispersion N-SF11 dense flint optical glass. The spectral solver
// evaluates a smooth IOR curve-map at the ray frequency instead of tracing
// fixed RGB wavelengths, which makes chromatic caustic separation visible in
// the box.
static const float GLASS_IOR_380NM = 1.860741;
static const float GLASS_IOR_420NM = 1.833285;
static const float GLASS_IOR_460NM = 1.815267;
static const float GLASS_IOR_500NM = 1.802555;
static const float GLASS_IOR_540NM = 1.793137;
static const float GLASS_IOR_580NM = 1.785902;
static const float GLASS_IOR_620NM = 1.780185;
static const float GLASS_IOR_660NM = 1.775562;
static const float GLASS_IOR_700NM = 1.771751;
static const float GLASS_IOR_740NM = 1.768555;

uint wangHash(uint s) {
  s = (s ^ 61u) ^ (s >> 16u);
  s *= 9u;
  s = s ^ (s >> 4u);
  s *= 0x27d4eb2du;
  s = s ^ (s >> 15u);
  return s;
}

float rand(inout uint seed) {
  seed = wangHash(seed);
  return float(seed & 0x00ffffffu) / 16777216.0;
}

float fresnelDielectric(float cosThetaI, float etaI, float etaT) {
  cosThetaI = saturate(cosThetaI);
  float sinThetaI = sqrt(max(0.0, 1.0 - cosThetaI * cosThetaI));
  float sinThetaT = (etaI / etaT) * sinThetaI;
  if (sinThetaT >= 1.0)
    return 1.0;

  float cosThetaT = sqrt(max(0.0, 1.0 - sinThetaT * sinThetaT));
  float rParallel = ((etaT * cosThetaI) - (etaI * cosThetaT)) /
                    max((etaT * cosThetaI) + (etaI * cosThetaT), 1e-5);
  float rPerpendicular = ((etaI * cosThetaI) - (etaT * cosThetaT)) /
                         max((etaI * cosThetaI) + (etaT * cosThetaT), 1e-5);
  return 0.5 * (rParallel * rParallel + rPerpendicular * rPerpendicular);
}

float frequencyToWavelengthNm(float frequencyThz) {
  return SPEED_OF_LIGHT_NM_THZ / clamp(frequencyThz, VISIBLE_FREQUENCY_MIN_THZ,
                                       VISIBLE_FREQUENCY_MAX_THZ);
}

float sampleRandomVisibleFrequency(inout uint seed) {
  float band = rand(seed) * 3.0;
  float u = rand(seed);
  if (band < 1.0)
    return lerp(VISIBLE_FREQUENCY_MIN_THZ, RED_GREEN_BOUNDARY_THZ, u);
  if (band < 2.0)
    return lerp(RED_GREEN_BOUNDARY_THZ, GREEN_BLUE_BOUNDARY_THZ, u);
  return lerp(GREEN_BLUE_BOUNDARY_THZ, VISIBLE_FREQUENCY_MAX_THZ, u);
}

float smoothCurve01(float t) {
  t = saturate(t);
  return t * t * (3.0 - 2.0 * t);
}

float glassIorCurveSegment(float wavelengthNm, float wavelength0Nm, float ior0,
                           float wavelength1Nm, float ior1) {
  float t = smoothCurve01((wavelengthNm - wavelength0Nm) /
                          max(wavelength1Nm - wavelength0Nm, 1e-5));
  return lerp(ior0, ior1, t);
}

float glassIorForWavelength(float wavelengthNm) {
  wavelengthNm =
      clamp(wavelengthNm, VISIBLE_WAVELENGTH_MIN_NM, VISIBLE_WAVELENGTH_MAX_NM);
  if (wavelengthNm < 420.0)
    return glassIorCurveSegment(wavelengthNm, 380.0, GLASS_IOR_380NM, 420.0,
                                GLASS_IOR_420NM);
  if (wavelengthNm < 460.0)
    return glassIorCurveSegment(wavelengthNm, 420.0, GLASS_IOR_420NM, 460.0,
                                GLASS_IOR_460NM);
  if (wavelengthNm < 500.0)
    return glassIorCurveSegment(wavelengthNm, 460.0, GLASS_IOR_460NM, 500.0,
                                GLASS_IOR_500NM);
  if (wavelengthNm < 540.0)
    return glassIorCurveSegment(wavelengthNm, 500.0, GLASS_IOR_500NM, 540.0,
                                GLASS_IOR_540NM);
  if (wavelengthNm < 580.0)
    return glassIorCurveSegment(wavelengthNm, 540.0, GLASS_IOR_540NM, 580.0,
                                GLASS_IOR_580NM);
  if (wavelengthNm < 620.0)
    return glassIorCurveSegment(wavelengthNm, 580.0, GLASS_IOR_580NM, 620.0,
                                GLASS_IOR_620NM);
  if (wavelengthNm < 660.0)
    return glassIorCurveSegment(wavelengthNm, 620.0, GLASS_IOR_620NM, 660.0,
                                GLASS_IOR_660NM);
  if (wavelengthNm < 700.0)
    return glassIorCurveSegment(wavelengthNm, 660.0, GLASS_IOR_660NM, 700.0,
                                GLASS_IOR_700NM);
  return glassIorCurveSegment(wavelengthNm, 700.0, GLASS_IOR_700NM, 740.0,
                              GLASS_IOR_740NM);
}

float glassIorForFrequency(float frequencyThz) {
  return glassIorForWavelength(frequencyToWavelengthNm(frequencyThz));
}

float glassAbsorptionForWavelength(float wavelengthNm) {
  // Clear glass should not add a color cast; let wavelength-dependent IOR,
  // not absorption, produce the caustic rainbow.
  return 0.0025 + wavelengthNm * 0.0;
}

bool refractDielectricBoundary(float3 incidentDir, float3 orientedNormal,
                               float etaI, float etaT, out float3 refractedDir,
                               out float fresnel) {
  float eta = etaI / etaT;
  float cosI = saturate(dot(-incidentDir, orientedNormal));
  float sin2T = eta * eta * max(0.0, 1.0 - cosI * cosI);
  fresnel = fresnelDielectric(cosI, etaI, etaT);
  if (sin2T >= 1.0)
    return false;

  refractedDir = normalize(refract(incidentDir, orientedNormal, eta));
  return true;
}

float3 uniformSphere(float2 xi) {
  float z = 1.0 - 2.0 * xi.x;
  float r = sqrt(max(0.0, 1.0 - z * z));
  float phi = 2.0 * PI * xi.y;
  return float3(r * cos(phi), z, r * sin(phi));
}

void tangentFrame(float3 n, out float3 t, out float3 b) {
  float3 up = abs(n.y) < 0.99 ? float3(0.0, 1.0, 0.0) : float3(1.0, 0.0, 0.0);
  t = normalize(cross(up, n));
  b = cross(n, t);
}

float3 cosineHemisphere(float2 xi, float3 n) {
  float r = sqrt(xi.x);
  float phi = 2.0 * PI * xi.y;
  float3 t;
  float3 b;
  tangentFrame(n, t, b);
  return normalize(t * (r * cos(phi)) + b * (r * sin(phi)) +
                   n * sqrt(max(0.0, 1.0 - xi.x)));
}

float3 sampleCone(float2 xi, float3 axis, float cosThetaMax) {
  float cosTheta = lerp(1.0, cosThetaMax, xi.x);
  float sinTheta = sqrt(max(0.0, 1.0 - cosTheta * cosTheta));
  float phi = 2.0 * PI * xi.y;
  float3 t;
  float3 b;
  tangentFrame(axis, t, b);
  return normalize(t * (sinTheta * cos(phi)) + b * (sinTheta * sin(phi)) +
                   axis * cosTheta);
}

float conePdf(float cosThetaMax) {
  return 1.0 / max(2.0 * PI * (1.0 - cosThetaMax), 1e-5);
}

bool glassGuideCone(float3 p, float3 n, out float3 axis, out float cosThetaMax,
                    out float pdf) {
  float3 toGlass = GLASS_CENTER - p;
  float dist = length(toGlass);
  axis = toGlass / max(dist, 1e-5);
  float sinTheta = saturate(GLASS_RADIUS / max(dist, GLASS_RADIUS + 1e-4));
  cosThetaMax = sqrt(max(0.0, 1.0 - sinTheta * sinTheta));
  pdf = conePdf(cosThetaMax);
  return dot(n, axis) > 0.0 && dist > GLASS_RADIUS;
}

float glassGuidePdfForDirection(float3 p, float3 n, float3 wi) {
  float3 axis;
  float cosThetaMax;
  float pdf;
  if (!glassGuideCone(p, n, axis, cosThetaMax, pdf))
    return 0.0;
  return dot(wi, axis) >= cosThetaMax ? pdf : 0.0;
}

float3 sampleGuidedDiffuse(float3 p, float3 n, inout uint seed, out float pdf) {
  float3 axis;
  float cosThetaMax;
  float guidePdf;
  bool canGuideGlass = glassGuideCone(p, n, axis, cosThetaMax, guidePdf);
  bool guideGlass =
      canGuideGlass && rand(seed) < DIFFUSE_GLASS_GUIDE_PROBABILITY;

  float3 wi =
      guideGlass ? sampleCone(float2(rand(seed), rand(seed)), axis, cosThetaMax)
                 : cosineHemisphere(float2(rand(seed), rand(seed)), n);
  float cosTheta = saturate(dot(n, wi));
  float cosinePdf = cosTheta / PI;
  float glassPdf = glassGuidePdfForDirection(p, n, wi);
  float guideWeight = canGuideGlass ? DIFFUSE_GLASS_GUIDE_PROBABILITY : 0.0;
  pdf = (1.0 - guideWeight) * cosinePdf + guideWeight * glassPdf;
  return wi;
}

bool intersectSphere(float3 ro, float3 rd, float3 center, float radius,
                     out float t, out float3 n) {
  t = 0.0;
  n = float3(0.0, 1.0, 0.0);
  float3 oc = ro - center;
  float b = dot(oc, rd);
  float c = dot(oc, oc) - radius * radius;
  float h = b * b - c;
  if (h < 0.0)
    return false;
  h = sqrt(h);
  float t0 = -b - h;
  float t1 = -b + h;
  t = t0 > 0.003 ? t0 : t1;
  if (t <= 0.003)
    return false;
  n = normalize(ro + rd * t - center);
  return true;
}

float3 primitiveNormal(uint primitive) {
  uint quad = primitive / 2u;
  if (quad == 0u)
    return float3(0.0, 1.0, 0.0); // floor
  if (quad == 1u)
    return float3(0.0, -1.0, 0.0); // ceiling
  if (quad == 2u)
    return float3(0.0, 0.0, -1.0); // back
  if (quad == 3u)
    return float3(1.0, 0.0, 0.0); // red wall
  if (quad == 4u)
    return float3(-1.0, 0.0, 0.0); // green wall
  if (quad == 5u)
    return float3(0.0, -1.0, 0.0); // light

  uint face = (quad - 6u) % 6u;
  float s = 0.70710678118;
  if (face == 0u)
    return float3(-s, 0.0, s);
  if (face == 1u)
    return float3(s, 0.0, -s);
  if (face == 2u)
    return float3(-s, 0.0, -s);
  if (face == 3u)
    return float3(s, 0.0, s);
  if (face == 4u)
    return float3(0.0, -1.0, 0.0);
  return float3(0.0, 1.0, 0.0);
}

float3 primitiveAlbedo(uint primitive) {
  uint quad = primitive / 2u;
  if (quad == 3u)
    return float3(0.86, 0.12, 0.08);
  if (quad == 4u)
    return float3(0.10, 0.48, 0.16);
  if (quad >= 6u && quad < 12u)
    return float3(0.76, 0.72, 0.64);
  if (quad >= 12u)
    return float3(0.72, 0.72, 0.76);
  return float3(0.78, 0.76, 0.70);
}

uint primitiveMaterial(uint primitive) {
  uint quad = primitive / 2u;
  if (quad == 5u)
    return MATERIAL_LIGHT;
  return MATERIAL_DIFFUSE;
}

float3 primitiveEmission(uint primitive) {
  return primitiveMaterial(primitive) == MATERIAL_LIGHT
             ? float3(18.0, 14.0, 9.0)
             : float3(0.0, 0.0, 0.0);
}

float3 wavelengthToRgbBasis(float wavelengthNm) {
  if (wavelengthNm < 505.0)
    return float3(0.0, 0.0, 1.0);
  if (wavelengthNm < 585.0)
    return float3(0.0, 1.0, 0.0);
  return float3(1.0, 0.0, 0.0);
}

float sampleRgbSpectrum(float3 rgb, float wavelengthNm) {
  return dot(rgb, wavelengthToRgbBasis(wavelengthNm));
}

float primitiveAlbedoAtWavelength(uint primitive, float wavelengthNm) {
  return sampleRgbSpectrum(primitiveAlbedo(primitive), wavelengthNm);
}

float primitiveEmissionAtWavelength(uint primitive, float wavelengthNm) {
  return sampleRgbSpectrum(primitiveEmission(primitive), wavelengthNm);
}

void traceTriangles(float3 ro, float3 rd, inout RayPayload payload) {
  RayDesc ray;
  ray.Origin = ro;
  ray.Direction = rd;
  ray.TMin = 0.003;
  ray.TMax = 100.0;
  payload.hit = 0u;
  payload.hitT = ray.TMax;
  payload.radiance = float3(0.0);
  TraceRay(scene_tlas, RAY_FLAG_NONE, 0xff, 0, 0, 0, ray, payload);
}

void traceScene(float3 ro, float3 rd, inout RayPayload payload) {
  traceTriangles(ro, rd, payload);

  float sphereT;
  float3 sphereN;
  if (intersectSphere(ro, rd, GLASS_CENTER, GLASS_RADIUS, sphereT, sphereN) &&
      (payload.hit == 0u || sphereT < payload.hitT)) {
    payload.hit = 1u;
    payload.hitT = sphereT;
    payload.normal = sphereN;
    payload.albedo = float3(0.998, 0.998, 0.998);
    payload.material = MATERIAL_GLASS;
    payload.radiance = float3(0.0);
  }
}

bool traceSpectralPhotonThroughGlass(float3 lightPos, float3 sphereSampleNormal,
                                     float frequencyThz,
                                     out SpectralPhoton photon) {
  photon.position = float3(0.0);
  photon.normal = float3(0.0, 1.0, 0.0);
  photon.incidentDirection = float3(0.0, 1.0, 0.0);
  photon.throughput = 0.0;
  photon.geometry = 0.0;

  float wavelengthNm = frequencyToWavelengthNm(frequencyThz);
  float3 entryP = GLASS_CENTER + sphereSampleNormal * GLASS_RADIUS;
  float3 toEntry = entryP - lightPos;
  float dist2 = dot(toEntry, toEntry);
  float dist = sqrt(max(dist2, 1e-6));
  float3 wi = toEntry / dist;
  float cosLight = saturate(wi.y);
  float cosEntry = saturate(dot(-wi, sphereSampleNormal));
  if (cosLight <= 0.0 || cosEntry <= 0.0)
    return false;

  float glassIor = glassIorForFrequency(frequencyThz);
  float3 insideDir;
  float fresnelEnter;
  if (!refractDielectricBoundary(wi, sphereSampleNormal, 1.0, glassIor,
                                 insideDir, fresnelEnter))
    return false;

  float exitT;
  float3 exitN;
  if (!intersectSphere(entryP - sphereSampleNormal * 0.004, insideDir,
                       GLASS_CENTER, GLASS_RADIUS, exitT, exitN))
    return false;

  float3 exitOrientedN = -exitN;
  float3 exitDir;
  float fresnelExit;
  if (!refractDielectricBoundary(insideDir, exitOrientedN, glassIor, 1.0,
                                 exitDir, fresnelExit))
    return false;

  float3 exitP = entryP - sphereSampleNormal * 0.004 + insideDir * exitT;
  RayPayload payload;
  payload.seed = 0u;
  traceScene(exitP + exitDir * 0.006, exitDir, payload);
  if (payload.hit == 0u || payload.material != MATERIAL_DIFFUSE)
    return false;

  photon.position = exitP + exitDir * (payload.hitT + 0.006);
  photon.normal = normalize(payload.normal);
  if (dot(photon.normal, exitDir) > 0.0)
    photon.normal = -photon.normal;
  photon.incidentDirection = -exitDir;

  float absorption = exp(-glassAbsorptionForWavelength(wavelengthNm) * exitT);
  photon.throughput = (1.0 - fresnelEnter) * (1.0 - fresnelExit) * absorption;
  photon.geometry = cosLight * cosEntry / max(dist2, 1e-4);
  return photon.throughput > 0.0;
}

float sampleSpectralPhotonMapCaustics(float3 p, float3 n,
                                      float surfaceReflectance,
                                      float frequencyThz, inout uint seed) {
  float sum = 0.0;
  float wavelengthNm = frequencyToWavelengthNm(frequencyThz);
  float lightArea = 0.64 * 0.58;
  float sphereArea = 4.0 * PI * GLASS_RADIUS * GLASS_RADIUS;
  float kernelRadius2 = PHOTON_MAP_GATHER_RADIUS * PHOTON_MAP_GATHER_RADIUS;

  // This is a stochastic per-query photon map: light subpaths become virtual
  // spectral photons after refraction through the glass, then the camera path
  // gathers nearby photons with a density kernel for bidirectional caustics.
  [loop]
  for (uint sample = 0u; sample < PHOTON_MAP_LIGHT_SUBPATH_SAMPLES; ++sample) {
    float2 lightXi = float2(rand(seed), rand(seed));
    float3 lightPos = float3(lerp(-0.32, 0.32, lightXi.x), 1.998,
                             lerp(0.78, 1.36, lightXi.y));
    float3 sphereN = uniformSphere(float2(rand(seed), rand(seed)));

    SpectralPhoton photon;
    if (!traceSpectralPhotonThroughGlass(lightPos, sphereN, frequencyThz,
                                         photon))
      continue;

    if (dot(photon.normal, n) < 0.98)
      continue;

    float d2 = dot(photon.position - p, photon.position - p);
    if (d2 >= kernelRadius2)
      continue;

    float kernel = exp(-d2 / max(kernelRadius2, 1e-5)) / (PI * kernelRadius2);
    float emission = sampleRgbSpectrum(float3(18.0, 14.0, 9.0), wavelengthNm);
    float cosSurface = saturate(dot(n, photon.incidentDirection));
    sum += emission * photon.throughput * photon.geometry * kernel *
           surfaceReflectance * cosSurface * lightArea * sphereArea;
  }

  return sum / float(PHOTON_MAP_LIGHT_SUBPATH_SAMPLES);
}

float sampleCornellAreaLight(float3 p, float3 n, float wavelengthNm,
                             inout uint seed) {
  float sum = 0.0;
  for (int sample = 0; sample < 4; ++sample) {
    float2 cell = float2(float(sample & 1), float(sample >> 1));
    float2 xi = (cell + float2(rand(seed), rand(seed))) * 0.5;
    float3 lightPos =
        float3(lerp(-0.32, 0.32, xi.x), 1.998, lerp(0.78, 1.36, xi.y));
    float3 toLight = lightPos - p;
    float dist2 = dot(toLight, toLight);
    float dist = sqrt(dist2);
    float3 wi = toLight / max(dist, 0.0001);
    float cosSurface = saturate(dot(n, wi));
    float cosLight = saturate(wi.y);
    if (cosSurface <= 0.0 || cosLight <= 0.0)
      continue;

    RayPayload shadow;
    shadow.seed = seed;
    traceScene(p + n * 0.004, wi, shadow);

    float lightArea = 0.64 * 0.58;
    if (shadow.hit == 0u || shadow.hitT >= dist - 0.01 ||
        shadow.material == MATERIAL_LIGHT) {
      float lightScale = lightArea * cosSurface * cosLight / max(dist2, 0.001);
      sum +=
          sampleRgbSpectrum(float3(18.0, 14.0, 9.0), wavelengthNm) * lightScale;
    }
  }
  return sum * 0.25;
}

SpectralPathResult solveBidirectionalSpectralPath(uint2 pixel, uint2 extent,
                                                  float frequencyThz,
                                                  float wavelengthNm,
                                                  inout uint seed) {
  float2 jitter = float2(rand(seed), rand(seed)) - 0.5;
  float2 uv =
      ((float2(pixel) + 0.5 + jitter) / float2(max(extent, uint2(1, 1)))) *
          2.0 -
      1.0;
  uv.y = -uv.y;

  float3 ro = float3(0.0, 1.0, -3.15);
  float tanHalfFovY = 0.42;
  float3 rd = normalize(float3(uv.x * rt_constants.aspect * tanHalfFovY,
                               uv.y * tanHalfFovY, 1.0));
  SpectralPathResult result;
  result.throughput = 1.0;
  result.radiance = 0.0;

  [loop]
  for (uint bounce = 0u; bounce < MAX_PATH_BOUNCES; ++bounce) {
    RayPayload payload;
    payload.seed = seed;
    traceScene(ro, rd, payload);
    seed = payload.seed;
    if (payload.hit == 0u)
      break;

    float3 n = normalize(payload.normal);
    if (dot(n, rd) > 0.0)
      n = -n;
    float3 p = ro + rd * payload.hitT;

    if (payload.material == MATERIAL_LIGHT) {
      result.radiance +=
          result.throughput * sampleRgbSpectrum(payload.radiance, wavelengthNm);
      break;
    }

    if (payload.material == MATERIAL_GLASS) {
      bool entering = dot(rd, payload.normal) < 0.0;
      float3 orientedN = entering ? payload.normal : -payload.normal;
      float glassIor = glassIorForFrequency(frequencyThz);
      float etaI = entering ? 1.0 : glassIor;
      float etaT = entering ? glassIor : 1.0;
      float3 refractedDir;
      float fresnel;
      bool canRefract = refractDielectricBoundary(rd, orientedN, etaI, etaT,
                                                  refractedDir, fresnel);

      if (!entering)
        result.throughput *=
            exp(-glassAbsorptionForWavelength(wavelengthNm) * payload.hitT);

      bool reflectEvent = !canRefract || rand(seed) < fresnel;
      if (reflectEvent) {
        rd = normalize(reflect(rd, orientedN));
        ro = p + orientedN * 0.004;
      } else {
        rd = refractedDir;
        ro = p - orientedN * 0.004;
      }
      result.throughput *= sampleRgbSpectrum(payload.albedo, wavelengthNm);
    } else {
      float surfaceReflectance =
          sampleRgbSpectrum(payload.albedo, wavelengthNm);
      result.radiance += result.throughput * surfaceReflectance *
                         sampleCornellAreaLight(p, n, wavelengthNm, seed) / PI;
      if (bounce <= 1u) {
        result.radiance += result.throughput *
                           sampleSpectralPhotonMapCaustics(
                               p, n, surfaceReflectance, frequencyThz, seed) /
                           PI;
      }

      float diffusePdf;
      float3 nextDir = sampleGuidedDiffuse(p, n, seed, diffusePdf);
      float cosTheta = saturate(dot(n, nextDir));
      if (diffusePdf <= 1e-5 || cosTheta <= 0.0)
        break;
      rd = nextDir;
      ro = p + n * 0.004;
      result.throughput *= surfaceReflectance * cosTheta / (PI * diffusePdf);
    }

    if (bounce >= 3u) {
      float keep = min(result.throughput, 0.95);
      if (rand(seed) > keep)
        break;
      result.throughput /= max(keep, 0.001);
    }
  }

  return result;
}

float3 traceBidirectionalSpectralSample(uint2 pixel, uint2 extent,
                                        uint sampleIndex) {
  uint seed =
      wangHash(pixel.x * 1973u + pixel.y * 9277u +
               rt_constants.frameIndex * 26699u + sampleIndex * 747796405u);
  float frequencyThz = sampleRandomVisibleFrequency(seed);
  float wavelengthNm = frequencyToWavelengthNm(frequencyThz);
  SpectralPathResult result = solveBidirectionalSpectralPath(
      pixel, extent, frequencyThz, wavelengthNm, seed);
  return result.radiance * wavelengthToRgbBasis(wavelengthNm) *
         SPECTRAL_RGB_RECONSTRUCTION_WEIGHT;
}

[shader("raygeneration")]
void raygen() {
  uint2 pixel = DispatchRaysIndex().xy;
  uint2 extent = DispatchRaysDimensions().xy;

  // Each camera ray now carries one randomly sampled visible frequency. The
  // denoiser accumulates those sparse spectral samples over time, while the
  // bidirectional photon-map gather adds focused glass caustics at that same
  // frequency.
  static const uint SAMPLES_PER_PIXEL = 8u;
  float3 radiance = float3(0.0);
  [loop]
  for (uint sampleIndex = 0u; sampleIndex < SAMPLES_PER_PIXEL; ++sampleIndex) {
    radiance += traceBidirectionalSpectralSample(pixel, extent, sampleIndex);
  }
  radiance /= float(SAMPLES_PER_PIXEL);

  output_image[pixel] = float4(radiance, 1.0);
}

[shader("miss")]
void miss(inout RayPayload payload) {
  payload.hit = 0u;
  payload.hitT = 100.0;
  payload.radiance = float3(0.0);
}

[shader("closesthit")]
void closest_hit(inout RayPayload payload,
                 in BuiltInTriangleIntersectionAttributes attribs) {
  uint primitive = PrimitiveIndex();
  payload.hit = 1u;
  payload.hitT = RayTCurrent();
  payload.normal = primitiveNormal(primitive);
  payload.albedo = primitiveAlbedo(primitive);
  payload.material = primitiveMaterial(primitive);
  payload.radiance = primitiveEmission(primitive);
  payload.seed = wangHash(payload.seed ^ primitive);
}
