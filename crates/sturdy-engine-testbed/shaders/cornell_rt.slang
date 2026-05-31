[[vk::binding(0, 0)]]
RWTexture2D<float4> output_image;
[[vk::binding(1, 0)]]
RaytracingAccelerationStructure scene_tlas;
[[vk::binding(3, 0)]]
RWTexture2D<float4> guide_image;
[[vk::binding(4, 0)]]
RWTexture2D<float4> material_guide_image;
[[vk::binding(6, 0)]]
RWTexture2D<float4> motion_vector_image;
[[vk::binding(7, 0)]]
RWTexture2D<float4> normal_image;
[[vk::binding(8, 0)]]
RWTexture2D<float4> depth_image;

struct TriangleInfo {
  float3 normal;
  uint material;
  float3 albedo;
  uint shape;
  float3 emission;
  uint _pad0;
  float3 sphereCenter;
  uint _pad1;
};
[[vk::binding(5, 0)]]
StructuredBuffer<TriangleInfo> triangle_infos;

struct RtConstants {
  uint frameIndex;
  uint width;
  uint height;
  uint sceneId;
  float aspect;
  float tanHalfFovY;
  uint _pad0;
  uint _pad1;
  float4 cameraOrigin;
  float4 cameraForward;
  float4 cameraRight;
  float4 cameraUp;
  float4 previousCameraOrigin;
  float4 previousCameraForward;
  float4 previousCameraRight;
  float4 previousCameraUp;
};
[[vk::binding(2, 0)]]
ConstantBuffer<RtConstants> rt_constants;

struct RayPayload {
  float3 radiance;
  float3 albedo;
  float3 normal;
  float hitT;
  float3 geometricNormal;
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
static const uint SCENE_CORNELL = 0u;
static const uint SCENE_OUTDOOR = 1u;
static const uint MATERIAL_DIFFUSE = 0u;
static const uint MATERIAL_LIGHT = 1u;
static const uint MATERIAL_GLASS = 2u;
static const uint SHAPE_FLAT = 0u;
static const uint SHAPE_SPHERE = 1u;
static const float3 GLASS_CENTER = float3(0.0, 0.84, 0.34);
static const float GLASS_RADIUS = 0.32;
static const uint MAX_PATH_BOUNCES = 16u;
static const uint PHOTON_MAP_LIGHT_SUBPATH_SAMPLES = 16u;
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
// Frequencies are sampled from three equal-probability RGB bands. The binary
// wavelengthToRgbBasis drives sampleRgbSpectrum for exact per-band energy; the
// final reconstruction step uses the full CIE 1931 XYZ response (Wyman, Sloan,
// Shirley — JCGT 2013) converted to linear sRGB for physically correct hues.
// Per-channel weights normalise a spectrally-flat emitter to (1,1,1): derived
// by numerically integrating CIE sRGB over the three frequency-uniform bands.
static const float3 CIE_RECONSTRUCTION_WEIGHTS = float3(2.110, 2.234, 3.602);
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

float3 outdoorSunDirection() { return normalize(float3(0.34, 0.72, 0.61)); }

float3 proceduralSkyRadiance(float3 direction) {
  float3 rd = normalize(direction);
  float up = saturate(rd.y * 0.5 + 0.5);
  float horizon = exp(-max(rd.y, 0.0) * 5.5);
  float belowHorizon = saturate(-rd.y * 4.0);

  float3 zenithColor = float3(0.10, 0.28, 0.82) * 1.6;
  float3 midColor = float3(0.38, 0.58, 0.95) * 1.15;
  float3 horizonColor = float3(1.00, 0.72, 0.42) * 0.95;
  float3 groundBounce = float3(0.34, 0.42, 0.27) * belowHorizon;

  float3 sky = lerp(horizonColor, midColor, smoothstep(0.02, 0.55, up));
  sky = lerp(sky, zenithColor, smoothstep(0.48, 1.0, up));
  sky += horizonColor * horizon * 0.55;

  float3 sunDir = outdoorSunDirection();
  float sunCos = saturate(dot(rd, sunDir));
  float antiSun = saturate(
      dot(rd, normalize(float3(-sunDir.x, sunDir.y * 0.35, -sunDir.z))));
  float rayleigh =
      0.38 * (1.0 + sunCos * sunCos) * pow(saturate(rd.y + 0.12), 0.35);
  float mie = pow(sunCos, 32.0) * 2.5 + pow(sunCos, 650.0) * 55.0;
  float warmBand = pow(antiSun, 6.0) * horizon * 0.35;

  float azimuth = atan2(rd.z, rd.x);
  float stratosphere = (sin(azimuth * 5.0 + rd.y * 11.0) * 0.5 + 0.5) *
                       (sin(azimuth * 2.0 - rd.y * 7.0) * 0.5 + 0.5);
  sky *= 0.94 + 0.08 * stratosphere * smoothstep(0.05, 0.95, up);

  sky += float3(0.32, 0.48, 0.85) * rayleigh;
  sky += float3(1.0, 0.78, 0.48) * warmBand;
  sky += float3(1.0, 0.86, 0.62) * mie;
  sky = lerp(sky, groundBounce, belowHorizon);
  return max(sky, float3(0.0));
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

// Beer-Lambert absorption coefficient for a given glass albedo and wavelength.
//
// The albedo encodes per-unit transmittance: a glass with albedo c transmits
// fraction c of light per unit length.  Absorption coefficient = -log(c).
// Near-neutral glass (albedo ≈ 0.998) gives ~0.002/unit; saturated colored
// glass (albedo ≈ (0.9, 0.1, 0.1)) produces strong blue/green absorption that
// intensifies with thickness, correctly tinting caustics and transmitted light.
float beerLambertAbsorption(float3 albedo, float wavelengthNm) {
  float3 safe = clamp(albedo, 0.001, 0.9999);
  float3 absorption = -log(safe);
  return max(0.0, sampleRgbSpectrum(absorption, wavelengthNm));
}

bool refractDielectricBoundary(float3 incidentDir, float3 orientedNormal,
                               float etaI, float etaT, out float3 refractedDir,
                               out float fresnel) {
  float eta = etaI / etaT;
  float cosI = saturate(dot(-incidentDir, orientedNormal));
  float sin2T = eta * eta * max(0.0, 1.0 - cosI * cosI);
  fresnel = fresnelDielectric(cosI, etaI, etaT);
  if (sin2T >= 1.0) {
    refractedDir = float3(0.0);
    return false;
  }

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
  if (rt_constants.sceneId != SCENE_CORNELL) {
    float3 wi = cosineHemisphere(float2(rand(seed), rand(seed)), n);
    pdf = saturate(dot(n, wi)) / PI;
    return wi;
  }

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

float3 wavelengthToRgbBasis(float wavelengthNm) {
  // Binary three-band basis matching the equal-probability RGB sampling bands.
  // Used by sampleRgbSpectrum for every surface/emission evaluation along the
  // path; must stay a strict partition of unity so scene colors are
  // energy-correct.
  if (wavelengthNm < 505.0)
    return float3(0.0, 0.0, 1.0);
  if (wavelengthNm < 585.0)
    return float3(0.0, 1.0, 0.0);
  return float3(1.0, 0.0, 0.0);
}

float3 wavelengthToLinearSrgb(float wavelengthNm) {
  // CIE 1931 2-degree observer — Wyman, Sloan, Shirley (JCGT 2013) multi-lobe
  // Gaussian fit. Converted to linear sRGB via IEC 61966-2-1 D65 matrix.
  // Used ONLY for the final per-sample colour reconstruction, never for path
  // throughput, so it does not affect scene colour energy balance.
  float wl = wavelengthNm;
  float dX1 = (wl - 442.0) * (wl < 442.0 ? 0.0624 : 0.0374);
  float dX2 = (wl - 599.8) * (wl < 599.8 ? 0.0264 : 0.0323);
  float dX3 = (wl - 501.1) * (wl < 501.1 ? 0.0490 : 0.0382);
  float X = 0.362 * exp(-0.5 * dX1 * dX1) + 1.056 * exp(-0.5 * dX2 * dX2) -
            0.065 * exp(-0.5 * dX3 * dX3);

  float dY1 = (wl - 568.8) * (wl < 568.8 ? 0.0213 : 0.0247);
  float dY2 = (wl - 530.9) * (wl < 530.9 ? 0.0613 : 0.0322);
  float Y = 0.821 * exp(-0.5 * dY1 * dY1) + 0.286 * exp(-0.5 * dY2 * dY2);

  float dZ1 = (wl - 437.0) * (wl < 437.0 ? 0.0845 : 0.0278);
  float dZ2 = (wl - 459.0) * (wl < 459.0 ? 0.0385 : 0.0725);
  float Z = 1.217 * exp(-0.5 * dZ1 * dZ1) + 0.681 * exp(-0.5 * dZ2 * dZ2);

  // Negative sRGB values are wavelengths outside the sRGB gamut (imaginary
  // primaries). Clamping rather than wrapping keeps violet/cyan from going
  // black.
  return max(float3(0.0), float3(3.2406 * X - 1.5372 * Y - 0.4986 * Z,
                                 -0.9689 * X + 1.8758 * Y + 0.0415 * Z,
                                 0.0557 * X - 0.2040 * Y + 1.0570 * Z));
}

float3 uniformUpperHemisphere(float2 xi) {
  // Uniform sampling of the hemisphere with y > 0. Used for glass entry points
  // because the ceiling light is always above the sphere — lower hemisphere
  // samples are always rejected, so sampling the full sphere wastes half the
  // budget.
  float z = xi.x;
  float r = sqrt(max(0.0, 1.0 - z * z));
  float phi = 2.0 * PI * xi.y;
  return float3(r * cos(phi), z, r * sin(phi));
}

float sampleRgbSpectrum(float3 rgb, float wavelengthNm) {
  return dot(rgb, wavelengthToRgbBasis(wavelengthNm));
}

void traceTriangles(float3 ro, float3 rd, inout RayPayload payload) {
  RayDesc ray;
  ray.Origin = ro;
  ray.Direction = rd;
  ray.TMin = 0.0005;
  ray.TMax = 100.0;
  payload.hit = 0u;
  payload.hitT = ray.TMax;
  payload.radiance = float3(0.0);
  TraceRay(scene_tlas, RAY_FLAG_NONE, 0xff, 0, 0, 0, ray, payload);
}

void traceScene(float3 ro, float3 rd, inout RayPayload payload) {
  traceTriangles(ro, rd, payload);

  if (rt_constants.sceneId == SCENE_CORNELL) {
    float sphereT;
    float3 sphereN;
    if (intersectSphere(ro, rd, GLASS_CENTER, GLASS_RADIUS, sphereT, sphereN) &&
        (payload.hit == 0u || sphereT < payload.hitT)) {
      payload.hit = 1u;
      payload.hitT = sphereT;
      payload.normal = sphereN;
      payload.geometricNormal = sphereN;
      payload.albedo = float3(0.998, 0.998, 0.998);
      payload.material = MATERIAL_GLASS;
      payload.radiance = float3(0.0);
    }
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

  // Cornell glass sphere is nearly clear (albedo ≈ 0.998), so Beer-Lambert
  // absorption is effectively the same as the old constant but now consistent
  // with the albedo-driven system used for all other glass.
  static const float3 CORNELL_GLASS_ALBEDO = float3(0.998, 0.998, 0.998);
  float absorption = exp(-beerLambertAbsorption(CORNELL_GLASS_ALBEDO, wavelengthNm) * exitT);
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
  // Upper hemisphere only: the ceiling light never illuminates the glass sphere
  // from below, so we restrict sphere-normal samples to y > 0 and use the
  // matching half-sphere area to keep the estimator unbiased.
  float sphereArea = 2.0 * PI * GLASS_RADIUS * GLASS_RADIUS;
  float kernelRadius2 = PHOTON_MAP_GATHER_RADIUS * PHOTON_MAP_GATHER_RADIUS;

  // Stochastic per-query photon map: light subpaths become virtual spectral
  // photons after refraction through the glass, then the camera path gathers
  // nearby photons with a cone density kernel for bidirectional caustics.
  [loop]
  for (uint sample = 0u; sample < PHOTON_MAP_LIGHT_SUBPATH_SAMPLES; ++sample) {
    float2 lightXi = float2(rand(seed), rand(seed));
    float3 lightPos = float3(lerp(-0.32, 0.32, lightXi.x), 1.998,
                             lerp(0.78, 1.36, lightXi.y));
    float3 sphereN = uniformUpperHemisphere(float2(rand(seed), rand(seed)));

    SpectralPhoton photon;
    if (!traceSpectralPhotonThroughGlass(lightPos, sphereN, frequencyThz,
                                         photon))
      continue;

    if (dot(photon.normal, n) < 0.98)
      continue;

    float d2 = dot(photon.position - p, photon.position - p);
    if (d2 >= kernelRadius2)
      continue;

    // Cone kernel K(d) = 2/πR² * max(0, 1 - d²/R²). Compared to the previous
    // Gaussian this is properly area-normalized and gives a sharper peak at the
    // caustic focus while rolling off smoothly to zero at radius R.
    float kernel =
        2.0 * max(0.0, 1.0 - d2 / kernelRadius2) / (PI * kernelRadius2);
    float emission = sampleRgbSpectrum(float3(18.0, 14.0, 9.0), wavelengthNm);
    float cosSurface = saturate(dot(n, photon.incidentDirection));
    sum += emission * photon.throughput * photon.geometry * kernel *
           surfaceReflectance * cosSurface * lightArea * sphereArea;
  }

  return sum / float(PHOTON_MAP_LIGHT_SUBPATH_SAMPLES);
}

// gn = geometric (flat) normal, used to offset the shadow ray origin so that
// near sharp mesh corners the origin is never placed inside an adjacent face.
float sampleCornellAreaLight(float3 p, float3 n, float3 gn, float wavelengthNm,
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
    traceScene(p + gn * 0.010, wi, shadow);

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

// Direct sun sample for outdoor diffuse surfaces.
//
// Traces a shadow ray toward the sun (with a small angular jitter for soft
// penumbra) and accumulates the sun radiance contribution.  Glass objects along
// the shadow ray are treated as transparent: the ray continues through each
// glass hit, accumulating Beer-Lambert attenuation, so caustic energy from
// glass objects focusing sunlight onto diffuse surfaces is captured directly
// rather than relying on slow Monte Carlo convergence.
float sampleOutdoorSun(float3 p, float3 n, float3 gn, float wavelengthNm,
                       inout uint seed) {
  float3 sunDir = outdoorSunDirection();
  float cosSurface = dot(n, sunDir);
  if (cosSurface <= 0.0)
    return 0.0;

  // Soft solar disc: angular radius ~0.26 degrees (real sun ≈ 0.265).
  float2 xi = float2(rand(seed), rand(seed));
  float3 t; float3 b;
  tangentFrame(sunDir, t, b);
  float r = sqrt(xi.x) * 0.00455;   // sin(0.26 deg) ≈ 0.00455
  float phi = 2.0 * PI * xi.y;
  float3 jitteredDir = normalize(sunDir + t * (r * cos(phi)) + b * (r * sin(phi)));

  // Walk through glass layers accumulating Beer-Lambert transmittance.
  float3 shadowO = p + gn * 0.010;
  float throughput = 1.0;
  [loop]
  for (int layer = 0; layer < 6; ++layer) {
    RayPayload shadow;
    shadow.seed = seed;
    traceScene(shadowO, jitteredDir, shadow);
    if (shadow.hit == 0u)
      break;  // clear path to sun
    if (shadow.material == MATERIAL_DIFFUSE || shadow.material == MATERIAL_LIGHT) {
      throughput = 0.0;  // opaque blocker
      break;
    }
    // Glass: attenuate by Beer-Lambert and advance through.
    throughput *= exp(-beerLambertAbsorption(shadow.albedo, wavelengthNm) * shadow.hitT);
    shadowO = shadowO + jitteredDir * (shadow.hitT + 0.010);
    if (throughput < 0.001) { throughput = 0.0; break; }
  }

  if (throughput <= 0.0)
    return 0.0;

  // Sun spectral radiance: approximate white-body at ~5800 K with the sky's
  // Mie peak driving the warm colour.
  float sunMie = pow(saturate(dot(jitteredDir, sunDir)), 650.0) * 55.0;
  float sunRadiance = sampleRgbSpectrum(float3(1.0, 0.86, 0.62) * sunMie, wavelengthNm);
  return throughput * sunRadiance * cosSurface;
}

void makeLookAtCameraRay(float2 uv, float3 origin, float3 target,
                         float tanHalfFovY, out float3 ro, out float3 rd) {
  float3 forward = normalize(target - origin);
  float3 right = normalize(cross(forward, float3(0.0, 1.0, 0.0)));
  float3 up = normalize(cross(right, forward));
  ro = origin;
  rd = normalize(forward + right * (uv.x * rt_constants.aspect * tanHalfFovY) +
                 up * (uv.y * tanHalfFovY));
}

void makeCameraRay(uint2 pixel, uint2 extent, float2 jitterPixels,
                   out float3 ro, out float3 rd) {
  float2 uv = ((float2(pixel) + 0.5 + jitterPixels) /
               float2(max(extent, uint2(1, 1)))) *
                  2.0 -
              1.0;
  uv.y = -uv.y;

  if (rt_constants.sceneId == SCENE_OUTDOOR) {
    ro = rt_constants.cameraOrigin.xyz;
    rd = normalize(rt_constants.cameraForward.xyz +
                   rt_constants.cameraRight.xyz *
                       (uv.x * rt_constants.aspect * rt_constants.tanHalfFovY) +
                   rt_constants.cameraUp.xyz *
                       (uv.y * rt_constants.tanHalfFovY));
    return;
  }

  ro = float3(0.0, 1.0, -3.15);
  float tanHalfFovY = 0.42;
  rd = normalize(float3(uv.x * rt_constants.aspect * tanHalfFovY,
                        uv.y * tanHalfFovY, 1.0));
}

float2 projectWorldToCameraUv(float3 worldPosition, bool previousCamera) {
  float3 origin = previousCamera ? rt_constants.previousCameraOrigin.xyz
                                 : rt_constants.cameraOrigin.xyz;
  float3 forward = previousCamera ? rt_constants.previousCameraForward.xyz
                                  : rt_constants.cameraForward.xyz;
  float3 right = previousCamera ? rt_constants.previousCameraRight.xyz
                                : rt_constants.cameraRight.xyz;
  float3 up = previousCamera ? rt_constants.previousCameraUp.xyz
                             : rt_constants.cameraUp.xyz;

  if (rt_constants.sceneId != SCENE_OUTDOOR) {
    origin = float3(0.0, 1.0, -3.15);
    forward = float3(0.0, 0.0, 1.0);
    right = float3(1.0, 0.0, 0.0);
    up = float3(0.0, 1.0, 0.0);
  }

  float3 local = worldPosition - origin;
  float z = max(dot(local, normalize(forward)), 1e-4);
  float x = dot(local, normalize(right));
  float y = dot(local, normalize(up));
  float tanHalfFovY =
      rt_constants.sceneId == SCENE_OUTDOOR ? rt_constants.tanHalfFovY : 0.42;
  float2 ndc = float2(x / (z * rt_constants.aspect * tanHalfFovY),
                      y / (z * tanHalfFovY));
  return float2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
}

void tracePrimaryGuides(uint2 pixel, uint2 extent, out float4 geometricGuide,
                        out float4 materialGuide, out float4 motionGuide,
                        out float4 normalGuide, out float4 depthGuide) {
  float3 ro;
  float3 rd;
  makeCameraRay(pixel, extent, float2(0.0), ro, rd);

  RayPayload payload;
  payload.seed = 0u;
  traceScene(ro, rd, payload);
  if (payload.hit == 0u) {
    geometricGuide = float4(0.5, 0.5, 0.5, 100.0);
    materialGuide = float4(0.0, 0.0, 0.0, 255.0);
    motionGuide = float4(0.0, 0.0, 0.0, 0.0);
    normalGuide = float4(0.5, 0.5, 0.5, 0.0);
    depthGuide = float4(100.0, 0.0, 0.0, 0.0);
    return;
  }

  float3 n = normalize(payload.normal);
  if (dot(n, rd) > 0.0)
    n = -n;
  float3 worldPosition = ro + rd * payload.hitT;
  float2 currentUv =
      (float2(pixel) + 0.5) / float2(max(extent.x, 1u), max(extent.y, 1u));
  float2 previousUv = projectWorldToCameraUv(worldPosition, true);
  float2 motionUv = previousUv - currentUv;

  geometricGuide = float4(n * 0.5 + 0.5, payload.hitT);
  materialGuide = float4(saturate(payload.albedo), float(payload.material));
  motionGuide = float4(motionUv, payload.hitT, 1.0);
  normalGuide = float4(n * 0.5 + 0.5, 1.0);
  depthGuide = float4(payload.hitT, 0.0, 0.0, 1.0);
}

SpectralPathResult solveBidirectionalSpectralPath(uint2 pixel, uint2 extent,
                                                  float frequencyThz,
                                                  float wavelengthNm,
                                                  inout uint seed) {
  float3 ro;
  float3 rd;
  makeCameraRay(pixel, extent, float2(rand(seed), rand(seed)) - 0.5, ro, rd);
  SpectralPathResult result;
  result.throughput = 1.0;
  result.radiance = 0.0;

  [loop]
  for (uint bounce = 0u; bounce < MAX_PATH_BOUNCES; ++bounce) {
    RayPayload payload;
    payload.seed = seed;
    traceScene(ro, rd, payload);
    seed = payload.seed;
    if (payload.hit == 0u) {
      result.radiance +=
          result.throughput * sampleRgbSpectrum(payload.radiance, wavelengthNm);
      break;
    }

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
      float3 shadingN = normalize(payload.normal);
      float3 geometricN = normalize(payload.geometricNormal);
      bool entering = dot(rd, geometricN) < 0.0;
      float3 orientedGeometricN = entering ? geometricN : -geometricN;
      float3 orientedN = entering ? shadingN : -shadingN;
      if (dot(orientedN, orientedGeometricN) < 0.0)
        orientedN = -orientedN;

      float glassIor = glassIorForFrequency(frequencyThz);
      float etaI = entering ? 1.0 : glassIor;
      float etaT = entering ? glassIor : 1.0;
      float3 refractedDir;
      float fresnel;
      bool canRefract = refractDielectricBoundary(rd, orientedN, etaI, etaT,
                                                  refractedDir, fresnel);
      if (canRefract && dot(refractedDir, orientedGeometricN) >= -1e-4) {
        canRefract = refractDielectricBoundary(rd, orientedGeometricN, etaI,
                                               etaT, refractedDir, fresnel);
      }

      // Beer-Lambert colored absorption: the glass albedo encodes per-unit
      // transmittance, so thicker glass produces stronger tinting of caustics
      // and transmitted light.  Apply on the exiting boundary using the
      // distance actually traveled inside the glass (payload.hitT).
      if (!entering)
        result.throughput *=
            exp(-beerLambertAbsorption(payload.albedo, wavelengthNm) * payload.hitT);

      bool reflectEvent = !canRefract || rand(seed) < fresnel;
      if (reflectEvent) {
        float3 reflectedDir = normalize(reflect(rd, orientedN));
        if (dot(reflectedDir, orientedGeometricN) <= 1e-4)
          reflectedDir = normalize(reflect(rd, orientedGeometricN));
        rd = reflectedDir;
        ro = p + orientedGeometricN * 0.006;
      } else {
        rd = refractedDir;
        ro = p - orientedGeometricN * 0.006;
      }
      // No additional flat tint here: color is already handled by Beer-Lambert above.
    } else {
      float surfaceReflectance =
          sampleRgbSpectrum(payload.albedo, wavelengthNm);
      // Geometric normal used for all ray offsets — avoids placing origins
      // inside adjacent faces at sharp mesh corners.
      float3 geoN = normalize(payload.geometricNormal);
      if (dot(geoN, n) < 0.0) geoN = -geoN;
      if (rt_constants.sceneId == SCENE_CORNELL) {
        result.radiance += result.throughput * surfaceReflectance *
                           sampleCornellAreaLight(p, n, geoN, wavelengthNm, seed) /
                           PI;
        if (bounce <= 1u) {
          result.radiance += result.throughput *
                             sampleSpectralPhotonMapCaustics(
                                 p, n, surfaceReflectance, frequencyThz, seed) /
                             PI;
        }
      } else if (rt_constants.sceneId == SCENE_OUTDOOR) {
        // Direct sun NEE: glass objects along the shadow ray are traversed
        // transparently with Beer-Lambert attenuation so focused sunlight
        // through glass (caustics) converges quickly instead of relying on
        // unguided secondary bounces.
        result.radiance += result.throughput * surfaceReflectance *
                           sampleOutdoorSun(p, n, geoN, wavelengthNm, seed) /
                           PI;
      }

      float diffusePdf;
      float3 nextDir = sampleGuidedDiffuse(p, n, seed, diffusePdf);
      float cosTheta = saturate(dot(n, nextDir));
      if (diffusePdf <= 1e-5 || cosTheta <= 0.0)
        break;
      rd = nextDir;
      ro = p + geoN * 0.010;
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
  return result.radiance * wavelengthToLinearSrgb(wavelengthNm) *
         CIE_RECONSTRUCTION_WEIGHTS;
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

  float4 geometricGuide;
  float4 materialGuide;
  float4 motionGuide;
  float4 normalGuide;
  float4 depthGuide;
  tracePrimaryGuides(pixel, extent, geometricGuide, materialGuide, motionGuide,
                     normalGuide, depthGuide);
  guide_image[pixel] = geometricGuide;
  material_guide_image[pixel] = materialGuide;
  motion_vector_image[pixel] = motionGuide;
  normal_image[pixel] = normalGuide;
  depth_image[pixel] = depthGuide;
}

[shader("miss")]
void miss(inout RayPayload payload) {
  payload.hit = 0u;
  payload.hitT = 100.0;
  payload.radiance = rt_constants.sceneId == SCENE_OUTDOOR
                         ? proceduralSkyRadiance(WorldRayDirection())
                         : float3(0.0);
}

[shader("closesthit")]
void closest_hit(inout RayPayload payload,
                 in BuiltInTriangleIntersectionAttributes attribs) {
  uint primitive = PrimitiveIndex();
  TriangleInfo info = triangle_infos[primitive];
  float3 geometricNormal = normalize(info.normal);
  float3 normal = geometricNormal;
  if (info.shape == SHAPE_SPHERE) {
    float3 p = WorldRayOrigin() + WorldRayDirection() * RayTCurrent();
    normal = normalize(p - info.sphereCenter);
    if (dot(normal, geometricNormal) < 0.0)
      normal = -normal;
  }

  payload.hit = 1u;
  payload.hitT = RayTCurrent();
  payload.normal = normal;
  payload.geometricNormal = geometricNormal;
  payload.albedo = info.albedo;
  payload.material = info.material;
  payload.radiance = info.emission;
  payload.seed = wangHash(payload.seed ^ primitive);
}
