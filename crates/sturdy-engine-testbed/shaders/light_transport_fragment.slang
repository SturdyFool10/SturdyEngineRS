struct VSOut {
  float4 position : SV_POSITION;
  float2 uv : TEXCOORD0;
};

struct LightTransportConstants {
  float time;
  uint frame;
  uint mode;
  uint _pad0;
  float2 resolution;
  float aspect;
  float exposure;
};

struct SphereHit {
  bool hit;
  float t;
  float3 normal;
};

static const float PI = 3.14159265359;
static const float3 SUN_DIR = normalize(float3(0.36, 0.82, 0.44));
static const float3 SUN_COLOR = float3(1.0, 0.86, 0.62) * 6.0;
static const float3 SPHERE_CENTER = float3(0.0, 0.88, 1.55);
static const float SPHERE_RADIUS = 0.82;

float hash12(float2 p) {
  float3 p3 = frac(float3(p.xyx) * 0.1031);
  p3 += dot(p3, p3.yzx + 33.33);
  return frac((p3.x + p3.y) * p3.z);
}

float2 rotate2(float2 p, float a) {
  float s = sin(a);
  float c = cos(a);
  return float2(c * p.x - s * p.y, s * p.x + c * p.y);
}

float3 spectralRamp(float t) {
  t = saturate(t);
  float r = exp(-pow((t - 0.82) / 0.23, 2.0));
  float g = exp(-pow((t - 0.50) / 0.20, 2.0));
  float b = exp(-pow((t - 0.18) / 0.22, 2.0));
  return normalize(float3(r, g, b) + 0.08) * 1.35;
}

float3 skyRadiance(float3 rd) {
  rd = normalize(rd);
  float up = saturate(rd.y * 0.5 + 0.5);
  float horizon = exp(-max(rd.y, 0.0) * 6.0);
  float sunCos = saturate(dot(rd, SUN_DIR));

  float3 lower = float3(0.70, 0.82, 0.96);
  float3 upper = float3(0.07, 0.22, 0.72) * 1.8;
  float3 sky = lerp(lower, upper, smoothstep(0.08, 1.0, up));
  sky += float3(1.0, 0.68, 0.38) * horizon * 0.42;
  sky += SUN_COLOR * (pow(sunCos, 550.0) * 0.85 + pow(sunCos, 48.0) * 0.08);
  return max(sky, float3(0.0));
}

SphereHit intersectSphere(float3 ro, float3 rd, float3 center, float radius) {
  SphereHit h;
  h.hit = false;
  h.t = 0.0;
  h.normal = float3(0.0, 1.0, 0.0);

  float3 oc = ro - center;
  float b = dot(oc, rd);
  float c = dot(oc, oc) - radius * radius;
  float d = b * b - c;
  if (d < 0.0)
    return h;

  float root = sqrt(d);
  float t0 = -b - root;
  float t1 = -b + root;
  float t = t0 > 0.002 ? t0 : t1;
  if (t <= 0.002)
    return h;

  h.hit = true;
  h.t = t;
  h.normal = normalize(ro + rd * t - center);
  return h;
}

bool intersectGround(float3 ro, float3 rd, out float t) {
  if (abs(rd.y) < 1e-5) {
    t = 0.0;
    return false;
  }
  t = -ro.y / rd.y;
  return t > 0.002;
}

float sphereSunOcclusion(float3 p) {
  SphereHit h = intersectSphere(p + float3(0.0, 0.015, 0.0), SUN_DIR,
                                SPHERE_CENTER, SPHERE_RADIUS);
  if (!h.hit)
    return 1.0;

  float grazing = saturate(abs(dot(h.normal, SUN_DIR)) * 5.0);
  return lerp(0.38, 0.74, grazing);
}

float3 causticEstimate(float3 p, LightTransportConstants params) {
  float3 focus3 =
      SPHERE_CENTER - SUN_DIR * (SPHERE_CENTER.y / max(SUN_DIR.y, 0.1));
  float2 q = p.xz - focus3.xz - float2(0.15, -0.20);
  q = rotate2(q, -0.34);

  float2 ellipse = q / float2(0.92, 0.30);
  float core = exp(-dot(ellipse, ellipse) * 2.15);
  float ring = exp(-abs(length(ellipse) - 0.72) * 8.0);
  float cusp = exp(-dot((q - float2(0.34, 0.02)) / float2(0.24, 0.08),
                        (q - float2(0.34, 0.02)) / float2(0.24, 0.08)) *
                   2.5);

  float3 sum = spectralRamp(0.58 + q.x * 0.18) * core * 1.8;
  sum += spectralRamp(0.25 + q.y * 0.6) * ring * 0.95;
  sum += float3(1.0, 0.86, 0.48) * cusp * 2.2;

  [unroll]
  for (int i = 0; i < 20; ++i) {
    float fi = float(i);
    float a = fi * 2.399963 + params.time * 0.018;
    float r = sqrt((fi + 0.5) / 20.0);
    float2 photon = float2(cos(a) * 0.78 * r, sin(a) * 0.22 * r);
    photon.x += 0.10 * sin(fi * 4.7);
    float2 d = (q - photon) / float2(0.075, 0.032);
    float w = exp(-dot(d, d));
    sum += spectralRamp(frac(fi * 0.137 + 0.21)) * w * 0.22;
  }

  float gate = smoothstep(3.2, 0.15, length(q / float2(1.2, 0.45)));
  return SUN_COLOR * sum * gate * 0.18;
}

float3 groundAlbedo(float3 p) {
  float checker = hash12(floor(p.xz * 0.65));
  float3 grassA = float3(0.22, 0.31, 0.18);
  float3 grassB = float3(0.34, 0.42, 0.25);
  float3 lane = float3(0.48, 0.45, 0.37);
  float path = smoothstep(0.58, 0.12, abs(p.x + sin(p.z * 0.38) * 0.55));
  return lerp(lerp(grassA, grassB, checker), lane, path * 0.55);
}

float3 shadeGround(float3 p, float3 rd, LightTransportConstants params) {
  float3 n = float3(0.0, 1.0, 0.0);
  float direct = saturate(dot(n, SUN_DIR)) * sphereSunOcclusion(p);
  float3 albedo = groundAlbedo(p);
  float3 color = albedo * (skyRadiance(n) * 0.20 + SUN_COLOR * direct * 0.18);
  color += causticEstimate(p, params);
  float viewFade = exp(-0.006 * dot(p.xz, p.xz));
  color = lerp(skyRadiance(rd), color, viewFade);
  return color;
}

float3 sampleWorldWithoutGlass(float3 ro, float3 rd,
                               LightTransportConstants params) {
  float tGround;
  if (intersectGround(ro, rd, tGround)) {
    float3 p = ro + rd * tGround;
    return shadeGround(p, rd, params);
  }
  return skyRadiance(rd);
}

float dielectricFresnel(float cosI, float etaI, float etaT) {
  cosI = saturate(cosI);
  float sinI = sqrt(max(0.0, 1.0 - cosI * cosI));
  float sinT = etaI / etaT * sinI;
  if (sinT >= 1.0)
    return 1.0;
  float cosT = sqrt(max(0.0, 1.0 - sinT * sinT));
  float rp = ((etaT * cosI) - (etaI * cosT)) /
             max((etaT * cosI) + (etaI * cosT), 1e-4);
  float rs = ((etaI * cosI) - (etaT * cosT)) /
             max((etaI * cosI) + (etaT * cosT), 1e-4);
  return saturate(0.5 * (rp * rp + rs * rs));
}

float3 refractOrReflect(float3 incident, float3 normal, float eta,
                        out bool refracted) {
  float3 r = refract(incident, normal, eta);
  refracted = dot(r, r) > 1e-6;
  return refracted ? normalize(r) : normalize(reflect(incident, normal));
}

float3 glassChannel(float3 ro, float3 rd, float eta, float absorption,
                    LightTransportConstants params) {
  SphereHit entry = intersectSphere(ro, rd, SPHERE_CENTER, SPHERE_RADIUS);
  if (!entry.hit)
    return sampleWorldWithoutGlass(ro, rd, params);

  float3 entryP = ro + rd * entry.t;
  bool entered;
  float3 insideDir = refractOrReflect(rd, entry.normal, 1.0 / eta, entered);
  if (!entered)
    return sampleWorldWithoutGlass(entryP + reflect(rd, entry.normal) * 0.01,
                                   reflect(rd, entry.normal), params);

  SphereHit exitHit = intersectSphere(entryP - entry.normal * 0.01, insideDir,
                                      SPHERE_CENTER, SPHERE_RADIUS);
  if (!exitHit.hit)
    return float3(0.0);

  float3 exitP = entryP - entry.normal * 0.01 + insideDir * exitHit.t;
  bool escaped;
  float3 outDir = refractOrReflect(insideDir, -exitHit.normal, eta, escaped);
  if (!escaped)
    outDir = normalize(reflect(insideDir, exitHit.normal));

  float travel = max(exitHit.t, 0.0);
  float transmittance = exp(-absorption * travel);
  return sampleWorldWithoutGlass(exitP + outDir * 0.02, outDir, params) *
         transmittance;
}

float3 shadeGlass(float3 ro, float3 rd, LightTransportConstants params,
                  out float3 normalOut) {
  SphereHit hit = intersectSphere(ro, rd, SPHERE_CENTER, SPHERE_RADIUS);
  normalOut = hit.normal;
  float3 p = ro + rd * hit.t;
  float cosI = saturate(dot(-rd, hit.normal));
  float fresnel = dielectricFresnel(cosI, 1.0, 1.50);

  float3 reflected = sampleWorldWithoutGlass(
      p + hit.normal * 0.02, normalize(reflect(rd, hit.normal)), params);
  float3 refracted;
  refracted.r = glassChannel(ro, rd, 1.525, 0.020, params).r;
  refracted.g = glassChannel(ro, rd, 1.505, 0.014, params).g;
  refracted.b = glassChannel(ro, rd, 1.485, 0.010, params).b;

  float rim = pow(1.0 - cosI, 3.0);
  float3 highlight =
      SUN_COLOR *
      pow(saturate(dot(normalize(reflect(-SUN_DIR, hit.normal)), -rd)), 128.0) *
      0.35;
  float3 glass = lerp(refracted, reflected, fresnel);
  glass += float3(0.65, 0.78, 1.0) * rim * 0.18 + highlight;
  return max(glass, float3(0.0));
}

void makeCameraRay(float2 uv, LightTransportConstants params, out float3 ro,
                   out float3 rd) {
  float orbit = params.time * 0.12;
  ro = float3(sin(orbit) * 3.4, 1.55 + 0.15 * sin(params.time * 0.31),
              -3.75 + cos(orbit) * 0.35);
  float3 target = float3(0.0, 0.62, 1.20);
  float3 forward = normalize(target - ro);
  float3 right = normalize(cross(forward, float3(0.0, 1.0, 0.0)));
  float3 up = normalize(cross(right, forward));
  float tanHalfFov = 0.55;
  rd = normalize(forward + right * (uv.x * params.aspect * tanHalfFov) +
                 up * (uv.y * tanHalfFov));
}

float4 main(VSOut input, uniform LightTransportConstants params) : SV_TARGET {
  float2 uv = input.uv * 2.0 - 1.0;
  uv.y = -uv.y;

  float2 pixel = input.uv * params.resolution;
  float jitter = hash12(pixel + float2(float(params.frame), params.time));

  float3 ro;
  float3 rd;
  makeCameraRay(uv, params, ro, rd);

  SphereHit sphere = intersectSphere(ro, rd, SPHERE_CENTER, SPHERE_RADIUS);
  float tGround;
  bool groundHit = intersectGround(ro, rd, tGround);

  float3 normalDebug = float3(0.0, 1.0, 0.0);
  float3 color;
  if (sphere.hit && (!groundHit || sphere.t < tGround)) {
    color = shadeGlass(ro, rd, params, normalDebug);
  } else {
    color = sampleWorldWithoutGlass(ro, rd, params);
  }

  if (params.mode == 1u) {
    if (groundHit) {
      float3 gp = ro + rd * tGround;
      color = causticEstimate(gp, params) * 1.8;
    } else {
      color = float3(0.0);
    }
  } else if (params.mode == 2u) {
    color = normalDebug * 0.5 + 0.5;
  }

  float vignette =
      smoothstep(1.65, 0.18, length(uv / float2(max(params.aspect, 0.2), 1.0)));
  float grain = (jitter - 0.5) * 0.012;
  color = max(color * params.exposure, float3(0.0));
  color *= 0.68 + 0.32 * vignette;
  color += grain;
  return float4(color, 1.0);
}
