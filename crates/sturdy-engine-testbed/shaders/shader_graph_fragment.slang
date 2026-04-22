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
};

float2 rotate2(float2 p, float angle) {
    float s = sin(angle);
    float c = cos(angle);
    return float2(c * p.x - s * p.y, s * p.x + c * p.y);
}

float opSmoothUnion(float a, float b, float k) {
    float h = saturate(0.5 + 0.5 * (b - a) / k);
    return lerp(b, a, h) - k * h * (1.0 - h);
}

float sphere(float3 p, float radius) {
    return length(p) - radius;
}

float torus(float3 p, float2 radius) {
    float2 q = float2(length(p.xz) - radius.x, p.y);
    return length(q) - radius.y;
}

float graphDistance(float3 p, float time) {
    p.xz = rotate2(p.xz, time * 0.42);
    p.xy = rotate2(p.xy, sin(time * 0.31) * 0.55);

    float shell = torus(p, float2(0.78 + 0.08 * sin(time * 1.7), 0.22));

    float3 core_p = p;
    core_p.y += sin(time * 1.3) * 0.12;
    float core = sphere(core_p, 0.42);

    float3 satellite_p = p;
    satellite_p.xz = rotate2(satellite_p.xz, -time * 1.25);
    satellite_p -= float3(0.76, 0.22 * sin(time * 2.1), 0.0);
    float satellite = sphere(satellite_p, 0.18);

    float shape = opSmoothUnion(shell, core, 0.24);
    return opSmoothUnion(shape, satellite, 0.18);
}

float3 estimateNormal(float3 p, float time) {
    float2 e = float2(0.0015, 0.0);
    return normalize(float3(
        graphDistance(p + e.xyy, time) - graphDistance(p - e.xyy, time),
        graphDistance(p + e.yxy, time) - graphDistance(p - e.yxy, time),
        graphDistance(p + e.yyx, time) - graphDistance(p - e.yyx, time)
    ));
}

float3 shadeGraph(float3 p, float3 ray_dir, float time) {
    float3 n = estimateNormal(p, time);
    float3 light_a = normalize(float3(0.45, 0.75, -0.55));
    float3 light_b = normalize(float3(-0.65, 0.20, 0.70));
    float rim = pow(saturate(1.0 + dot(n, ray_dir)), 3.0);
    float bands = 0.5 + 0.5 * sin(9.0 * n.x + 7.0 * n.y + time * 2.2);

    float3 ink = float3(0.045, 0.055, 0.075);
    float3 copper = float3(0.95, 0.43, 0.18);
    float3 teal = float3(0.08, 0.82, 0.78);
    float3 violet = float3(0.45, 0.30, 0.96);
    float3 material = lerp(copper, teal, bands);

    float diffuse = saturate(dot(n, light_a)) * 0.85 + saturate(dot(n, light_b)) * 0.35;
    float specular = pow(saturate(dot(reflect(light_a, n), ray_dir)), 22.0);
    return ink + material * diffuse + violet * rim + float3(1.0, 0.88, 0.62) * specular;
}

float4 main(VSOut input, uniform FrameGraphConstants graph) : SV_TARGET {
    float2 uv = input.uv * 2.0 - 1.0;
    uv.x *= graph.aspect;

    float3 origin = float3(0.0, 0.0, -3.35);
    float3 ray_dir = normalize(float3(uv, 1.65));
    float depth = 0.0;
    float hit = 0.0;

    for (int i = 0; i < 96; ++i) {
        float3 p = origin + ray_dir * depth;
        float dist = graphDistance(p, graph.time);
        if (dist < 0.0015) {
            hit = 1.0;
            break;
        }
        depth += dist * 0.82;
        if (depth > 7.0) {
            break;
        }
    }

    float3 background = float3(0.015, 0.018, 0.026) + 0.17 * float3(uv.y + 0.7, 0.3 + uv.x * 0.2, 0.8);
    float3 color = background;
    if (hit > 0.5) {
        float3 p = origin + ray_dir * depth;
        color = shadeGraph(p, ray_dir, graph.time);
        color *= exp(-0.045 * depth * depth);
    }

    float vignette = smoothstep(1.75, 0.25, length(uv));
    color *= 0.65 + 0.35 * vignette;

    // Color-grade via CPU-generated LUT: sample at luminance to get a tint.
    float luma = dot(color, float3(0.2126, 0.7152, 0.0722));
    float3 tint = color_lut.SampleLevel(color_lut_sampler, float2(saturate(luma * 0.5), 0.5), 0).rgb;
    color = color * tint;

    float mask = procedural_mask.SampleLevel(procedural_mask_sampler, input.uv, 0).a;
    if (hit > 0.5) {
        color *= lerp(0.94, 1.08, mask);
    }

    // Output linear HDR — the Hermite tonemapper handles gamma encoding.
    return float4(color, 1.0);
}
