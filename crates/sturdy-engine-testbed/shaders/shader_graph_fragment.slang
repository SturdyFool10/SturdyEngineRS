Texture2D<float4> color_lut;
SamplerState color_lut_sampler;
Texture2D<float4> procedural_mask;
SamplerState procedural_mask_sampler;

struct VSOut {
  float4 position : SV_POSITION;
  float2 uv : TEXCOORD0;
};

struct FrameGraphConstants {
  float time;
  float aspect;
  float2 resolution;
  uint scene;
  uint frameIndex;
};

static const float PI = 3.14159265;

float2 rotate2(float2 p, float angle) {
  float s = sin(angle);
  float c = cos(angle);
  return float2(c * p.x - s * p.y, s * p.x + c * p.y);
}

float hash12(float2 p) {
  float3 p3 = frac(float3(p.xyx) * 0.1031);
  p3 += dot(p3, p3.yzx + 33.33);
  return frac((p3.x + p3.y) * p3.z);
}

float noise(float2 p) {
  float2 i = floor(p);
  float2 f = frac(p);
  float2 u = f * f * (3.0 - 2.0 * f);
  return lerp(lerp(hash12(i + float2(0, 0)), hash12(i + float2(1, 0)), u.x),
              lerp(hash12(i + float2(0, 1)), hash12(i + float2(1, 1)), u.x),
              u.y);
}

float fbm(float2 p) {
  float v = 0.0;
  float a = 0.5;
  for (int i = 0; i < 5; ++i) {
    v += noise(p) * a;
    p = rotate2(p * 2.05, 0.55);
    a *= 0.5;
  }
  return v;
}

float sdBox(float2 p, float2 b) {
  float2 d = abs(p) - b;
  return length(max(d, 0.0)) + min(max(d.x, d.y), 0.0);
}

float roundedPanel(float2 uv, float2 center, float2 halfSize, float radius) {
  float d = sdBox(uv - center, halfSize - radius) - radius;
  return 1.0 - smoothstep(0.0, 0.008, d);
}

float3 lut(float x) {
  return color_lut.SampleLevel(color_lut_sampler, float2(saturate(x), 0.5), 0)
      .rgb;
}

float3 skySunDirection(float time) {
  float azimuth = time * 0.08 + 0.65;
  float elevation = 0.34 + 0.16 * sin(time * 0.06);
  return normalize(float3(cos(azimuth) * cos(elevation), sin(elevation),
                          sin(azimuth) * cos(elevation)));
}

float3 skyboxRadiance(float3 dir, float3 sunDir) {
  float horizon = saturate(dir.y * 0.5 + 0.5);
  float rayleigh = pow(saturate(1.0 - horizon), 2.0);
  float mie = pow(saturate(dot(dir, sunDir)), 18.0);
  float sunDisc = smoothstep(cos(0.018), cos(0.006), dot(dir, sunDir));
  float horizonGlow = exp(-abs(dir.y) * 6.0);

  float3 zenith = float3(0.015, 0.105, 0.42) * 1.8;
  float3 midSky = float3(0.14, 0.34, 0.78) * 1.15;
  float3 horizonCol = float3(0.85, 0.54, 0.26) * 1.2;
  float3 sky = lerp(horizonCol, midSky, smoothstep(0.0, 0.62, horizon));
  sky = lerp(sky, zenith, smoothstep(0.58, 1.0, horizon));
  sky += float3(0.55, 0.72, 1.0) * rayleigh * 0.16;
  sky += float3(1.0, 0.72, 0.38) * (mie * 0.8 + horizonGlow * 0.10);
  sky += float3(1.0, 0.86, 0.58) * sunDisc * 18.0;
  return sky;
}

float3 viewRay(float2 uv) { return normalize(float3(uv.x, uv.y * 0.78, 1.25)); }

float3 proceduralSky(float2 uv, float time) {
  float3 ray = viewRay(uv);
  float3 sunDir = skySunDirection(time);
  float3 sky = skyboxRadiance(ray, sunDir);

  // Subtle latitude/longitude sampling rings are intentionally absent: this is
  // a clean shader-driven skybox, not a cloud/card/grid scene.
  float groundFade = smoothstep(-0.10, -0.55, ray.y);
  float3 groundBounce = float3(0.045, 0.052, 0.055) +
                        float3(0.10, 0.075, 0.05) * saturate(dot(ray, sunDir));
  return lerp(sky, groundBounce, groundFade);
}

float ggxDistribution(float ndh, float roughness) {
  float a = roughness * roughness;
  float a2 = a * a;
  float d = ndh * ndh * (a2 - 1.0) + 1.0;
  return a2 / max(PI * d * d, 0.0001);
}

float smithG1(float ndv, float roughness) {
  float r = roughness + 1.0;
  float k = (r * r) / 8.0;
  return ndv / max(ndv * (1.0 - k) + k, 0.0001);
}

float3 fresnelSchlick(float cosTheta, float3 f0) {
  return f0 + (1.0 - f0) * pow(1.0 - saturate(cosTheta), 5.0);
}

float3 pbrShade(float3 n, float3 v, float3 baseColor, float metallic,
                float roughness, float ao, float3 sunDir, float3 skyColor) {
  float3 l = sunDir;
  float3 h = normalize(v + l);
  float ndl = saturate(dot(n, l));
  float ndv = saturate(dot(n, v));
  float ndh = saturate(dot(n, h));
  float vdh = saturate(dot(v, h));
  float3 f0 = lerp(float3(0.04, 0.04, 0.04), baseColor, metallic);
  float d = ggxDistribution(ndh, roughness);
  float g = smithG1(ndv, roughness) * smithG1(ndl, roughness);
  float3 f = fresnelSchlick(vdh, f0);
  float3 specular = d * g * f / max(4.0 * ndv * ndl, 0.001);
  float3 kd = (1.0 - f) * (1.0 - metallic);
  float3 diffuse = kd * baseColor / PI;
  float3 direct = (diffuse + specular) * ndl * float3(7.0, 5.8, 4.4);

  float fresnelEnv = pow(1.0 - ndv, 5.0);
  float3 ambientDiffuse =
      baseColor * skyColor * (0.18 + 0.26 * n.y) * ao * (1.0 - metallic);
  float3 ambientSpec = skyColor * lerp(0.04, 1.0, metallic) *
                       (0.06 + 0.55 * fresnelEnv) * (1.0 - roughness * 0.72);
  return direct + ambientDiffuse + ambientSpec;
}

float3 proceduralMaterialParams(float2 tuv, float time, int materialKind,
                                out float metallic, out float roughness,
                                out float ao) {
  float pattern;
  metallic = 0.0;
  roughness = 0.5;
  ao = 1.0;
  if (materialKind == 0) {
    pattern = fbm(tuv * 9.0 + float2(time * 0.04, 0.0));
    float vein = smoothstep(0.42, 0.76, fbm(tuv * 22.0 + pattern));
    roughness = lerp(0.28, 0.72, pattern);
    ao = lerp(0.82, 1.0, pattern);
    return lerp(float3(0.11, 0.055, 0.025), float3(0.92, 0.42, 0.12), pattern) +
           vein * float3(0.18, 0.09, 0.03);
  }
  if (materialKind == 1) {
    pattern =
        sin((tuv.x + fbm(tuv * 4.0) * 0.16) * 42.0 + time * 1.2) * 0.5 + 0.5;
    roughness = 0.34;
    ao = 0.95;
    return lerp(float3(0.015, 0.012, 0.010), float3(1.0, 0.28, 0.045) * 2.4,
                pow(pattern, 4.0));
  }
  pattern = fbm(tuv * 18.0);
  metallic = 1.0;
  roughness = lerp(0.07, 0.22, pattern);
  ao = lerp(0.75, 1.0, pattern);
  return lerp(float3(0.58, 0.52, 0.44), float3(0.96, 0.86, 0.66), pattern);
}

float3 materialSphere(float2 uv, float2 center, float radius, float time,
                      int materialKind) {
  float2 p = (uv - center) / radius;
  float rr = dot(p, p);
  if (rr > 1.0)
    return float3(0.0, 0.0, 0.0);

  float z = sqrt(1.0 - rr);
  float3 n = normalize(float3(p.x, p.y, z));
  float3 v = float3(0.0, 0.0, 1.0);
  float2 tuv = float2(atan2(n.z, n.x) / (2.0 * PI) + 0.5, asin(n.y) / PI + 0.5);
  float metallic, roughness, ao;
  float3 base = proceduralMaterialParams(tuv, time, materialKind, metallic,
                                         roughness, ao);
  float3 sunDir = normalize(float3(-0.36, 0.62, 0.70));
  float3 sky = skyboxRadiance(reflect(-v, n), sunDir);
  float3 color = pbrShade(n, v, base, metallic, roughness, ao, sunDir, sky);
  float edge = smoothstep(1.0, 0.92, rr);
  return color * edge;
}

float3 proceduralMaterialsScene(float2 uv, float time) {
  float3 ray = viewRay(uv);
  float3 sunDir = normalize(float3(-0.36, 0.62, 0.70));
  float3 color = skyboxRadiance(ray, sunDir) * 0.18;
  color += materialSphere(uv, float2(-0.62, 0.00), 0.42, time, 0);
  color += materialSphere(uv, float2(0.00, 0.00), 0.42, time, 1);
  color += materialSphere(uv, float2(0.62, 0.00), 0.42, time, 2);
  return color;
}

float softShadowFromColumn(float2 groundPoint, float2 casterBase,
                           float casterHeight, float3 sunDir) {
  float2 shadowDir = normalize(-sunDir.xz);
  float shadowLen = casterHeight / max(sunDir.y, 0.08) * 0.32;
  float2 pa = groundPoint - casterBase;
  float along = dot(pa, shadowDir);
  float across = length(pa - shadowDir * along);
  float body = smoothstep(0.20, 0.02, across) * smoothstep(0.0, 0.16, along) *
               smoothstep(shadowLen, shadowLen * 0.28, along);
  return saturate(body);
}

float3 dynamicLightingScene(float2 uv, float time) {
  float3 sunDir = skySunDirection(time + 8.0);
  float3 ray = viewRay(uv);
  float3 color = skyboxRadiance(ray, sunDir) * 0.26;

  float ground = smoothstep(-0.15, -0.28, uv.y);
  float perspective = max(0.18, -uv.y + 0.42);
  float2 gp = float2(uv.x / perspective, uv.y * 1.8 + 1.0);

  float shadow = 0.0;
  shadow += softShadowFromColumn(gp, float2(-0.62, 0.12), 1.25, sunDir);
  shadow += softShadowFromColumn(gp, float2(0.04, 0.32), 1.75, sunDir);
  shadow += softShadowFromColumn(gp, float2(0.58, 0.03), 0.95, sunDir);
  shadow = saturate(shadow);

  float3 groundNormal = normalize(float3(0.0, 1.0, 0.0));
  float3 groundBase = lerp(float3(0.34, 0.32, 0.29), float3(0.18, 0.17, 0.16),
                           saturate(gp.y * 0.35));
  float3 litGround = pbrShade(groundNormal, float3(0.0, 0.45, 0.89), groundBase,
                              0.0, 0.62, 1.0 - shadow * 0.72, sunDir,
                              skyboxRadiance(groundNormal, sunDir));
  litGround *= lerp(1.0, 0.24, shadow);
  color = lerp(color, litGround, ground);

  float2 columnCenters[3] = { float2(-0.52, -0.12), float2(0.06, 0.02),
                              float2(0.52, -0.17) };
  float columnHeights[3] = { 0.52, 0.74, 0.43 };
  for (int i = 0; i < 3; ++i) {
    float2 c = columnCenters[i];
    float h = columnHeights[i];
    float body = smoothstep(0.16, 0.14, abs(uv.x - c.x)) *
                 smoothstep(c.y - h, c.y - h + 0.035, uv.y) *
                 smoothstep(c.y, c.y - 0.035, uv.y);
    float3 n = normalize(float3((uv.x - c.x) * 4.0, 0.18, 0.92));
    float sideShadow = i == 1 ? 0.15 : 0.0;
    float3 base = i == 1 ? float3(0.52, 0.49, 0.43) : float3(0.38, 0.35, 0.31);
    float3 shaded =
        pbrShade(n, float3(0.0, 0.0, 1.0), base, 0.0, 0.48, 1.0 - sideShadow,
                 sunDir, skyboxRadiance(n, sunDir));
    color = lerp(color, shaded, body);
  }

  float sunSpot = smoothstep(cos(0.024), cos(0.008), dot(ray, sunDir));
  color += sunSpot * float3(8.0, 6.4, 3.8);
  return color;
}

float3 temporalPostScene(float2 uv, float time) {
  float3 ray = viewRay(uv);
  float3 color = skyboxRadiance(ray, normalize(float3(-0.4, 0.5, 0.7))) * 0.08;
  float2 p = uv;
  float r = length(p);
  float ang = atan2(p.y, p.x);
  float spokes = pow(abs(sin(ang * 10.0 + r * 15.0 - time * 3.0)), 20.0);
  float movingEdge = smoothstep(
      0.015, 0.0,
      abs(length(p - float2(sin(time) * 0.32, cos(time * 0.8) * 0.20)) - 0.28));
  color += smoothstep(1.0, 0.1, r) * float3(0.08, 0.08, 0.18);
  color += spokes * float3(0.0, 1.0, 0.82) * 1.8;
  color += movingEdge * float3(0.9, 0.55, 1.6);
  return color;
}

float3 proceduralTextureScene(float2 uv, float time) {
  float2 sampleUv = rotate2(uv * 0.5, sin(time * 0.35) * 0.55) + 0.5;
  float4 mask =
      procedural_mask.SampleLevel(procedural_mask_sampler, sampleUv, 0);
  float3 ramp = lut(uv.x * 0.5 + 0.5 + time * 0.04);
  float rings = 0.5 + 0.5 * sin(length(uv) * 48.0 - time * 5.0);
  return lerp(float3(0.025, 0.04, 0.07), ramp * (0.65 + rings), mask.a);
}

float3 bloomScene(float2 uv, float time) {
  float3 ray = viewRay(uv);
  float3 color =
      skyboxRadiance(ray, normalize(float3(-0.28, 0.42, 0.86))) * 0.06;
  float2 p = uv;
  float3 glow = float3(0.0, 0.0, 0.0);
  for (int i = 0; i < 10; ++i) {
    float fi = float(i);
    float angle = time * (0.35 + fi * 0.035) + fi * 2.399;
    float2 c = float2(cos(angle), sin(angle * 1.37)) * float2(0.76, 0.46);
    float d = length(p - c);
    float3 hue = lut(frac(fi / 10.0 + time * 0.025));
    glow += hue * (0.026 / max(d * d, 0.0016));
    glow += smoothstep(0.030, 0.0, d) * hue * 18.0;
  }
  float core = smoothstep(0.18, 0.0, length(p));
  glow += core * float3(5.8, 4.2, 2.4);
  return color + glow;
}

float lcg(inout uint state) {
  state = state * 1664525u + 1013904223u;
  return float(state & 0x00ffffffu) / 16777216.0;
}

float3 cosineHemisphere(float2 xi, float3 n) {
  float r = sqrt(xi.x);
  float phi = 2.0 * PI * xi.y;
  float3 up = abs(n.y) < 0.99 ? float3(0.0, 1.0, 0.0) : float3(1.0, 0.0, 0.0);
  float3 t = normalize(cross(up, n));
  float3 b = cross(n, t);
  return normalize(t * (r * cos(phi)) + b * (r * sin(phi)) +
                   n * sqrt(max(0.0, 1.0 - xi.x)));
}

bool intersectBox(float3 ro, float3 rd, float3 bmin, float3 bmax,
                  out float tHit, out float3 normal) {
  float3 inv = 1.0 / rd;
  float3 t0 = (bmin - ro) * inv;
  float3 t1 = (bmax - ro) * inv;
  float3 tsm = min(t0, t1);
  float3 tbig = max(t0, t1);
  float tNear = max(max(tsm.x, tsm.y), tsm.z);
  float tFar = min(min(tbig.x, tbig.y), tbig.z);
  tHit = 0.0;
  normal = float3(0.0, 1.0, 0.0);
  if (tNear <= 0.001 || tNear > tFar)
    return false;
  tHit = tNear;
  float3 p = ro + rd * tHit;
  float eps = 0.002;
  if (abs(p.x - bmin.x) < eps)
    normal = float3(-1, 0, 0);
  else if (abs(p.x - bmax.x) < eps)
    normal = float3(1, 0, 0);
  else if (abs(p.y - bmin.y) < eps)
    normal = float3(0, -1, 0);
  else if (abs(p.y - bmax.y) < eps)
    normal = float3(0, 1, 0);
  else if (abs(p.z - bmin.z) < eps)
    normal = float3(0, 0, -1);
  else
    normal = float3(0, 0, 1);
  return true;
}

static const uint CORNELL_MATERIAL_DIFFUSE = 0u;
static const uint CORNELL_MATERIAL_GLASS = 1u;
static const float3 CORNELL_GLASS_CENTER = float3(0.0, 0.32, 0.34);
static const float CORNELL_GLASS_RADIUS = 0.32;
static const float CORNELL_GLASS_IOR = 1.52;

bool intersectSphere(float3 ro, float3 rd, float3 center, float radius,
                     out float tHit, out float3 normal) {
  float3 oc = ro - center;
  float b = dot(oc, rd);
  float c = dot(oc, oc) - radius * radius;
  float h = b * b - c;
  tHit = 0.0;
  normal = float3(0.0, 1.0, 0.0);
  if (h < 0.0)
    return false;
  h = sqrt(h);
  float t = -b - h;
  if (t <= 0.001)
    t = -b + h;
  if (t <= 0.001)
    return false;
  tHit = t;
  normal = normalize(ro + rd * t - center);
  return true;
}

bool intersectCornellShell(float3 ro, float3 rd, out float closest,
                           out float3 normal, out float3 albedo,
                           out float3 emission, out uint material) {
  closest = 1e20;
  normal = float3(0.0, 1.0, 0.0);
  albedo = float3(0.78, 0.76, 0.70);
  emission = float3(0.0, 0.0, 0.0);
  material = CORNELL_MATERIAL_DIFFUSE;
  bool hit = false;

  float t;
  float2 p;

  t = (0.0 - ro.y) / rd.y;
  p = ro.xz + rd.xz * t;
  if (t > 0.001 && t < closest && abs(p.x) <= 1.0 && p.y >= 0.0 && p.y <= 2.0) {
    closest = t;
    normal = float3(0.0, 1.0, 0.0);
    albedo = float3(0.78, 0.76, 0.70);
    hit = true;
  }

  t = (2.0 - ro.y) / rd.y;
  p = ro.xz + rd.xz * t;
  if (t > 0.001 && t < closest && abs(p.x) <= 1.0 && p.y >= 0.0 && p.y <= 2.0) {
    closest = t;
    normal = float3(0.0, -1.0, 0.0);
    albedo = float3(0.78, 0.76, 0.70);
    emission = p.x > -0.32 && p.x < 0.32 && p.y > 0.78 && p.y < 1.36
                   ? float3(18.0, 14.0, 9.0)
                   : float3(0.0, 0.0, 0.0);
    hit = true;
  }

  t = (2.0 - ro.z) / rd.z;
  p = ro.xy + rd.xy * t;
  if (t > 0.001 && t < closest && abs(p.x) <= 1.0 && p.y >= 0.0 && p.y <= 2.0) {
    closest = t;
    normal = float3(0.0, 0.0, -1.0);
    albedo = float3(0.78, 0.76, 0.70);
    emission = float3(0.0, 0.0, 0.0);
    hit = true;
  }

  t = (-1.0 - ro.x) / rd.x;
  p = ro.yz + rd.yz * t;
  if (t > 0.001 && t < closest && p.x >= 0.0 && p.x <= 2.0 && p.y >= 0.0 &&
      p.y <= 2.0) {
    closest = t;
    normal = float3(1.0, 0.0, 0.0);
    albedo = float3(0.86, 0.12, 0.08);
    emission = float3(0.0, 0.0, 0.0);
    hit = true;
  }

  t = (1.0 - ro.x) / rd.x;
  p = ro.yz + rd.yz * t;
  if (t > 0.001 && t < closest && p.x >= 0.0 && p.x <= 2.0 && p.y >= 0.0 &&
      p.y <= 2.0) {
    closest = t;
    normal = float3(-1.0, 0.0, 0.0);
    albedo = float3(0.10, 0.48, 0.16);
    emission = float3(0.0, 0.0, 0.0);
    hit = true;
  }

  return hit;
}

bool intersectCornellScene(float3 ro, float3 rd, out float closest,
                           out float3 normal, out float3 albedo,
                           out float3 emission, out uint material) {
  bool hit = intersectCornellShell(ro, rd, closest, normal, albedo, emission,
                                   material);
  float t;
  float3 hn;

  if (intersectBox(ro, rd, float3(-0.47, 0.0, 0.56), float3(-0.05, 0.72, 0.98),
                   t, hn) &&
      t < closest) {
    closest = t;
    normal = hn;
    albedo = float3(0.76, 0.72, 0.64);
    emission = float3(0.0, 0.0, 0.0);
    material = CORNELL_MATERIAL_DIFFUSE;
    hit = true;
  }
  if (intersectBox(ro, rd, float3(0.22, 0.0, 0.82), float3(0.67, 1.18, 1.30), t,
                   hn) &&
      t < closest) {
    closest = t;
    normal = hn;
    albedo = float3(0.72, 0.72, 0.76);
    emission = float3(0.0, 0.0, 0.0);
    material = CORNELL_MATERIAL_DIFFUSE;
    hit = true;
  }
  if (intersectSphere(ro, rd, CORNELL_GLASS_CENTER, CORNELL_GLASS_RADIUS, t,
                      hn) &&
      t < closest) {
    closest = t;
    normal = hn;
    albedo = float3(0.96, 0.985, 1.0);
    emission = float3(0.0, 0.0, 0.0);
    material = CORNELL_MATERIAL_GLASS;
    hit = true;
  }

  return hit;
}

float3 cornellDirectLight(float3 p, float3 n, uint seed) {
  float2 baseJitter = float2(lcg(seed), lcg(seed));
  float3 sum = float3(0.0, 0.0, 0.0);

  for (int sample = 0; sample < 4; ++sample) {
    float2 cell = float2(float(sample & 1), float(sample >> 1));
    float2 xi = (cell + baseJitter) * 0.5;
    float3 lightPos =
        float3(lerp(-0.32, 0.32, xi.x), 1.998, lerp(0.78, 1.36, xi.y));
    float3 toLight = lightPos - p;
    float dist2 = dot(toLight, toLight);
    float dist = sqrt(dist2);
    float3 wi = toLight / dist;
    float cosSurface = saturate(dot(n, wi));
    float cosLight = saturate(wi.y);
    if (cosSurface <= 0.0 || cosLight <= 0.0)
      continue;

    float shadowT;
    float3 shadowN;
    float3 shadowAlbedo;
    float3 shadowEmission;
    uint shadowMaterial;
    bool occluded =
        intersectCornellScene(p + n * 0.004, wi, shadowT, shadowN, shadowAlbedo,
                              shadowEmission, shadowMaterial) &&
        shadowT < dist - 0.01 && dot(shadowEmission, shadowEmission) <= 0.0 &&
        shadowMaterial != CORNELL_MATERIAL_GLASS;
    if (!occluded) {
      float lightArea = 0.64 * 0.58;
      sum += float3(18.0, 14.0, 9.0) *
             (lightArea * cosSurface * cosLight / max(dist2, 0.001));
    }
  }

  return sum * 0.25;
}

float schlickFresnel(float cosTheta, float etaI, float etaT) {
  float r0 = (etaI - etaT) / (etaI + etaT);
  r0 *= r0;
  return r0 + (1.0 - r0) * pow(1.0 - saturate(cosTheta), 5.0);
}

float3 cornellGlassCaustic(float3 p, float3 n) {
  float3 lightCenter = float3(0.0, 1.998, 1.07);
  float3 lightToSphere = CORNELL_GLASS_CENTER - lightCenter;
  float3 sphereToPoint = p - CORNELL_GLASS_CENTER;
  float axial = dot(normalize(lightToSphere), normalize(sphereToPoint));
  float projectedRadius =
      length(sphereToPoint - normalize(lightToSphere) *
                                 dot(sphereToPoint, normalize(lightToSphere)));
  float floorMask =
      smoothstep(0.18, 0.0, abs(p.y)) * saturate(dot(n, float3(0.0, 1.0, 0.0)));
  float focus = exp(-projectedRadius * projectedRadius * 38.0) *
                smoothstep(-0.35, 0.85, axial);
  float distanceFalloff = 1.0 / max(dot(sphereToPoint, sphereToPoint), 0.08);
  return float3(1.0, 0.94, 0.82) * focus * floorMask * distanceFalloff * 1.15;
}

float3 cornellPathTraceSample(float2 uv, float time, uint frameIndex,
                              uint sampleIndex) {
  float3 ro = float3(0.0, 1.0, -3.15);
  float3 rd = normalize(float3(uv.x * 0.85, uv.y * 0.62, 1.65));
  uint seed = uint((uv.x * 8192.0 + 9000.0) * 13.0) ^
              uint((uv.y * 8192.0 + 9000.0) * 131.0) ^
              (frameIndex * 2891336453u) ^ (sampleIndex * 747796405u);
  rd = normalize(rd + float3(lcg(seed) - 0.5, lcg(seed) - 0.5, 0.0) * 0.0025);

  float3 throughput = float3(1.0, 1.0, 1.0);
  float3 radiance = float3(0.0, 0.0, 0.0);
  for (int bounce = 0; bounce < 8; ++bounce) {
    float closest;
    float3 n;
    float3 albedo;
    float3 emission;
    uint material;
    if (!intersectCornellScene(ro, rd, closest, n, albedo, emission, material))
      break;

    float3 p = ro + rd * closest;
    radiance += throughput * emission;
    if (dot(emission, emission) > 0.0)
      break;

    if (material == CORNELL_MATERIAL_GLASS) {
      bool entering = dot(rd, n) < 0.0;
      float3 orientedN = entering ? n : -n;
      float etaI = entering ? 1.0 : CORNELL_GLASS_IOR;
      float etaT = entering ? CORNELL_GLASS_IOR : 1.0;
      float eta = etaI / etaT;
      float cosI = saturate(dot(-rd, orientedN));
      float sin2T = eta * eta * max(0.0, 1.0 - cosI * cosI);
      float fresnel = schlickFresnel(cosI, etaI, etaT);
      bool totalInternalReflection = sin2T >= 1.0;
      bool reflectEvent = totalInternalReflection || lcg(seed) < fresnel;

      if (reflectEvent) {
        rd = normalize(reflect(rd, orientedN));
        ro = p + orientedN * 0.004;
      } else {
        rd = normalize(refract(rd, orientedN, eta));
        ro = p - orientedN * 0.004;
        float travel = 2.0 * CORNELL_GLASS_RADIUS;
        throughput *= exp(-float3(0.018, 0.008, 0.002) * travel);
      }
      throughput *= albedo;
    } else {
      radiance += throughput * albedo *
                  (cornellDirectLight(p, n, seed) + cornellGlassCaustic(p, n)) /
                  PI;

      float2 xi = float2(lcg(seed), lcg(seed));
      ro = p + n * 0.004;
      rd = cosineHemisphere(xi, n);
      throughput *= albedo;
    }
  }
  return radiance;
}

float3 cornellPathTracingScene(float2 uv, float time, float2 resolution,
                               uint frameIndex) {
  float2 texel = 2.0 / max(resolution, float2(1.0, 1.0));
  float2 offsets[9] = { float2(0.0, 0.0),    float2(0.5, 0.0),
                        float2(-0.5, 0.0),   float2(0.0, 0.5),
                        float2(0.0, -0.5),   float2(0.35, 0.35),
                        float2(-0.35, 0.35), float2(0.35, -0.35),
                        float2(-0.35, -0.35) };

  float3 samples[9];
  float lumas[9];
  float meanLuma = 0.0;
  for (int i = 0; i < 9; ++i) {
    samples[i] = cornellPathTraceSample(uv + offsets[i] * texel, time,
                                        frameIndex, uint(i));
    lumas[i] = dot(samples[i], float3(0.2126, 0.7152, 0.0722));
    meanLuma += lumas[i];
  }
  meanLuma /= 9.0;

  float variance = 0.0;
  for (int i = 0; i < 9; ++i) {
    float d = lumas[i] - meanLuma;
    variance += d * d;
  }
  variance /= 9.0;
  float sigma = sqrt(variance + 0.0001);
  float fireflyLimit = meanLuma + sigma * 1.6 + 0.45;

  float3 filtered = float3(0.0, 0.0, 0.0);
  float totalWeight = 0.0;
  float centerLuma = lumas[0];
  for (int i = 0; i < 9; ++i) {
    float clippedLuma = min(lumas[i], fireflyLimit);
    float3 clipped = samples[i] * (clippedLuma / max(lumas[i], 0.0001));
    float spatialWeight = (i == 0) ? 2.2 : ((i < 5) ? 1.0 : 0.7);
    float rangeWeight =
        exp(-abs(lumas[i] - centerLuma) / max(sigma * 1.8, 0.08));
    float weight = spatialWeight * rangeWeight;
    filtered += clipped * weight;
    totalWeight += weight;
  }

  return filtered / max(totalWeight, 0.0001);
}

float3 debugGraphScene(float2 uv, float time) {
  float3 ray = viewRay(uv);
  float3 color = skyboxRadiance(ray, normalize(float3(-0.4, 0.55, 0.5))) * 0.10;
  for (int y = 0; y < 4; ++y) {
    for (int x = 0; x < 5; ++x) {
      float2 cell = float2(float(x), float(y));
      float2 center = (cell / float2(4.0, 3.0)) * 1.45 - float2(0.72, 0.72);
      center.y *= 0.82;
      float node = roundedPanel(uv, center, float2(0.12, 0.055), 0.018);
      float activity = hash12(cell + floor(time * 3.0));
      color += node * lerp(float3(0.08, 0.25, 0.75), float3(1.4, 0.32, 0.10),
                           activity);
    }
  }
  return color;
}

float3 overviewScene(float2 uv, float time) {
  float3 color = skyboxRadiance(viewRay(uv), skySunDirection(time)) * 0.05;
  float2 centers[8] = { float2(-0.72, 0.56),  float2(0.0, 0.56),
                        float2(0.72, 0.56),   float2(-0.72, -0.02),
                        float2(0.0, -0.02),   float2(0.72, -0.02),
                        float2(-0.36, -0.62), float2(0.36, -0.62) };
  for (int i = 0; i < 8; ++i) {
    float2 c = centers[i];
    float2 halfSize = i >= 6 ? float2(0.32, 0.20) : float2(0.27, 0.20);
    float panel = roundedPanel(uv, c, halfSize, 0.035);
    float2 local = (uv - c) / halfSize;
    float3 sceneColor = float3(0.0, 0.0, 0.0);
    if (i == 0)
      sceneColor = proceduralSky(local, time);
    if (i == 1)
      sceneColor = proceduralMaterialsScene(local, time);
    if (i == 2)
      sceneColor = dynamicLightingScene(local, time);
    if (i == 3)
      sceneColor = temporalPostScene(local, time);
    if (i == 4)
      sceneColor = proceduralTextureScene(local, time);
    if (i == 5)
      sceneColor = bloomScene(local, time);
    if (i == 6)
      sceneColor =
          cornellPathTracingScene(local, time, float2(640.0, 360.0), 0u);
    if (i == 7)
      sceneColor = debugGraphScene(local, time);
    color += sceneColor * panel;
  }
  return color;
}

float4 main(VSOut input, uniform FrameGraphConstants graph) : SV_TARGET {
  float2 flippedUv = float2(input.uv.x, 1.0 - input.uv.y);
  float2 uv = flippedUv * 2.0 - 1.0;
  uv.x *= graph.aspect;

  float3 color;
  if (graph.scene == 1) {
    color = proceduralSky(uv, graph.time);
  } else if (graph.scene == 2) {
    color = proceduralMaterialsScene(uv, graph.time);
  } else if (graph.scene == 3) {
    color = dynamicLightingScene(uv, graph.time);
  } else if (graph.scene == 4) {
    color = temporalPostScene(uv, graph.time);
  } else if (graph.scene == 5) {
    color = proceduralTextureScene(uv, graph.time);
  } else if (graph.scene == 6) {
    color = bloomScene(uv, graph.time);
  } else if (graph.scene == 7) {
    color = cornellPathTracingScene(uv, graph.time, graph.resolution,
                                    graph.frameIndex);
  } else if (graph.scene == 8) {
    color = debugGraphScene(uv, graph.time);
  } else {
    color = overviewScene(uv, graph.time);
  }

  float luma = dot(color, float3(0.2126, 0.7152, 0.0722));
  float3 tint = lut(luma * 0.45 + 0.05);
  color *= lerp(float3(1.0, 1.0, 1.0), tint,
                graph.scene == 1 || graph.scene == 3 ? 0.06 : 0.22);

  float mask =
      procedural_mask.SampleLevel(procedural_mask_sampler, flippedUv, 0).a;
  color *= lerp(0.95, 1.06, mask);

  return float4(color, 1.0);
}
