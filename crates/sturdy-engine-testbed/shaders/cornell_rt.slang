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

static const float PI = 3.14159265359;
static const uint MATERIAL_DIFFUSE = 0u;
static const uint MATERIAL_LIGHT = 1u;
static const uint MATERIAL_GLASS = 2u;
static const float3 GLASS_CENTER = float3(0.0, 0.32, 0.34);
static const float GLASS_RADIUS = 0.32;
static const float GLASS_IOR = 1.52;

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

float schlickFresnel(float cosTheta, float etaI, float etaT) {
  float r0 = (etaI - etaT) / (etaI + etaT);
  r0 *= r0;
  return r0 + (1.0 - r0) * pow(1.0 - saturate(cosTheta), 5.0);
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
    payload.albedo = float3(0.96, 0.985, 1.0);
    payload.material = MATERIAL_GLASS;
    payload.radiance = float3(0.0);
  }
}

float3 sampleCornellAreaLight(float3 p, float3 n, inout uint seed) {
  float3 sum = float3(0.0);
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
    bool occluded = shadow.hit != 0u && shadow.hitT < dist - 0.01 &&
                    shadow.material != MATERIAL_LIGHT &&
                    shadow.material != MATERIAL_GLASS;
    if (!occluded) {
      float lightArea = 0.64 * 0.58;
      sum += float3(18.0, 14.0, 9.0) *
             (lightArea * cosSurface * cosLight / max(dist2, 0.001));
    }
  }
  return sum * 0.25;
}

float3 tracePathSample(uint2 pixel, uint2 extent, uint sampleIndex) {
  uint seed =
      wangHash(pixel.x * 1973u + pixel.y * 9277u +
               rt_constants.frameIndex * 26699u + sampleIndex * 747796405u);
  float2 jitter = float2(rand(seed), rand(seed)) - 0.5;
  float2 uv =
      ((float2(pixel) + 0.5 + jitter) / float2(max(extent, uint2(1, 1)))) *
          2.0 -
      1.0;
  uv.y = -uv.y;
  uv.x *= rt_constants.aspect;

  float3 ro = float3(0.0, 1.0, -3.15);
  float3 rd = normalize(float3(uv.x * 0.85, uv.y * 0.62, 1.65));
  float3 throughput = float3(1.0);
  float3 radiance = float3(0.0);

  [loop]
  for (int bounce = 0; bounce < 8; ++bounce) {
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

    radiance += throughput * payload.radiance;
    if (payload.material == MATERIAL_LIGHT)
      break;

    if (payload.material == MATERIAL_GLASS) {
      bool entering = dot(rd, payload.normal) < 0.0;
      float3 orientedN = entering ? payload.normal : -payload.normal;
      float etaI = entering ? 1.0 : GLASS_IOR;
      float etaT = entering ? GLASS_IOR : 1.0;
      float eta = etaI / etaT;
      float cosI = saturate(dot(-rd, orientedN));
      float sin2T = eta * eta * max(0.0, 1.0 - cosI * cosI);
      float fresnel = schlickFresnel(cosI, etaI, etaT);
      bool totalInternalReflection = sin2T >= 1.0;
      bool reflectEvent = totalInternalReflection || rand(seed) < fresnel;
      if (reflectEvent) {
        rd = normalize(reflect(rd, orientedN));
        ro = p + orientedN * 0.004;
      } else {
        rd = normalize(refract(rd, orientedN, eta));
        ro = p - orientedN * 0.004;
        float travel = 2.0 * GLASS_RADIUS;
        throughput *= exp(-float3(0.018, 0.008, 0.002) * travel);
      }
      throughput *= payload.albedo;
    } else {
      radiance +=
          throughput * payload.albedo * sampleCornellAreaLight(p, n, seed) / PI;

      float2 xi = float2(rand(seed), rand(seed));
      rd = cosineHemisphere(xi, n);
      ro = p + n * 0.004;
      throughput *= payload.albedo;
    }

    if (bounce >= 3) {
      float keep =
          min(max(max(throughput.x, throughput.y), throughput.z), 0.95);
      if (rand(seed) > keep)
        break;
      throughput /= max(keep, 0.001);
    }
  }

  return radiance;
}

[shader("raygeneration")]
void raygen() {
  uint2 pixel = DispatchRaysIndex().xy;
  uint2 extent = DispatchRaysDimensions().xy;

  static const uint SAMPLES_PER_PIXEL = 4u;
  float3 radiance = float3(0.0);
  [unroll]
  for (uint sampleIndex = 0u; sampleIndex < SAMPLES_PER_PIXEL; ++sampleIndex) {
    radiance += tracePathSample(pixel, extent, sampleIndex);
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
