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

float sdBox(float2 p, float2 b) {
  float2 d = abs(p) - b;
  return length(max(d, 0.0)) + min(max(d.x, d.y), 0.0);
}

float roundedPanel(float2 uv, float2 center, float2 halfSize, float radius) {
  float d = sdBox(uv - center, halfSize - radius) - radius;
  return 1.0 - smoothstep(0.0, 0.02, d);
}

float2 panelMotion(float2 uv, float2 center, float2 halfSize, float2 direction,
                   float speed, float time) {
  float panel = roundedPanel(uv, center, halfSize, 0.035);
  float2 local = (uv - center) / halfSize;
  float swirl =
      sin(atan2(local.y, local.x) * 4.0 + length(local) * 8.0 + time * speed);
  float2 tangent = normalize(float2(-local.y, local.x) + float2(0.0001, 0.0));
  return panel * (direction * speed * 0.0015 + tangent * swirl * 0.0025);
}

float4 main(VSOut input, uniform FrameGraphConstants graph) : SV_TARGET {
  float2 uv = float2(input.uv.x, 1.0 - input.uv.y) * 2.0 - 1.0;
  uv.x *= graph.aspect;

  float2 motion = float2(0.0, 0.0);

  if (graph.scene == 1) {
    float cloudBand = smoothstep(0.25, 0.85, uv.y + 0.6);
    return float4(float2(0.0012, 0.0) * cloudBand, 0.0, 1.0);
  }

  if (graph.scene == 3) {
    float2 lightingMotion =
        float2(sin(graph.time * 0.7), cos(graph.time * 0.9)) * 0.004;
    return float4(lightingMotion * smoothstep(1.4, 0.1, length(uv)), 0.0, 1.0);
  }

  if (graph.scene == 6) {
    float2 radial = normalize(uv + float2(0.0001, 0.0));
    return float4(radial * 0.003, 0.0, 1.0);
  }

  if (graph.scene == 7) {
    return float4(float2(0.0007, 0.0003) * sin(graph.time * 0.5), 0.0, 1.0);
  }

  if (graph.scene == 4) {
    float2 local = uv;
    float2 tangent = normalize(float2(-local.y, local.x) + float2(0.0001, 0.0));
    float pulse = sin(graph.time * 2.0 + length(local) * 12.0);
    return float4(tangent * pulse * 0.006, 0.0, 1.0);
  }

  // Overview/default: strong motion in the hero panel, smaller directional
  // motion in each system tile. This makes TAA/motion-debug views visibly
  // useful.
  float hero = roundedPanel(uv, float2(0.0, 0.03), float2(0.92, 0.66), 0.035);
  float2 heroLocal = (uv - float2(0.0, 0.03)) / float2(0.92, 0.66);
  float orbital = sin(graph.time * 1.25 + length(heroLocal) * 5.0);
  float2 tangent =
      normalize(float2(-heroLocal.y, heroLocal.x) + float2(0.0001, 0.0));
  motion +=
      hero * (tangent * orbital * 0.005 +
              float2(cos(graph.time * 0.7), sin(graph.time * 0.9)) * 0.0015);

  motion += panelMotion(uv, float2(-1.28, 0.58), float2(0.34, 0.28),
                        float2(1.0, 0.0), 1.1, graph.time);
  motion += panelMotion(uv, float2(-1.28, -0.08), float2(0.34, 0.28),
                        float2(0.0, -1.0), 0.8, graph.time);
  motion += panelMotion(uv, float2(-1.28, -0.72), float2(0.34, 0.22),
                        float2(0.6, 0.4), 1.3, graph.time);
  motion += panelMotion(uv, float2(1.28, 0.58), float2(0.34, 0.28),
                        float2(-0.8, 0.3), 1.6, graph.time);
  motion += panelMotion(uv, float2(1.28, -0.08), float2(0.34, 0.28),
                        float2(0.4, 0.8), 0.9, graph.time);
  motion += panelMotion(uv, float2(1.28, -0.72), float2(0.34, 0.22),
                        float2(-1.0, 0.0), 1.1, graph.time + 2.0);

  return float4(motion, 0.0, 1.0);
}
