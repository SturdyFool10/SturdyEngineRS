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
  // PsychoV-11 parameters
  float psychoPeakValue;
  float psychoHighlights;
  float psychoShadows;
  float psychoContrastHigh;
  float psychoContrastLow;
  float psychoPurity;
  float psychoBleaching;
  float psychoHueRestore;
  float psychoAdaptContrast;
  float psychoConeExp;
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
// PsychoV-11  (by clshortfuse/RenoDX)
//
// Perceptual tonemapper built on the Stockman-Sharp LMS cone model:
//   1. BT.709 → BT.2020 → Stockman-Sharp LMS
//   2. Gamut compress in MacLeod-Boynton chromaticity (add white toward D65)
//   3. Luminance-space highlights / shadows / per-side contrast adjustments
//   4. Optional purity (saturation) scaling in MB chromaticity
//   5. Optional adaptation-contrast (Michaelis-Menten-like per-cone gain)
//   6. Optional photopigment bleaching model
//   7. Neutwo per-channel curve (or Naka-Rushton when coneExp != 1)
//   8. Post-curve hue restoration in MB space
//   9. Gamut compress again → BT.2020 → BT.709
//
// Reference: https://github.com/clshortfuse/renodx
// ────────────────────────────────────────────────────────────────────────────

// Color-space matrices (row-major for Slang mul(M, v) semantics).
static const float3x3 P11_BT709_TO_XYZ = float3x3(
    0.4123907993f,  0.3575843394f,  0.1804807884f,
    0.2126390059f,  0.7151686788f,  0.0721923154f,
    0.0193308187f,  0.1191947798f,  0.9505321522f);

static const float3x3 P11_XYZ_TO_BT709 = float3x3(
     3.2409699419f, -1.5373831776f, -0.4986107603f,
    -0.9692436363f,  1.8759675015f,  0.0415550574f,
     0.0556300797f, -0.2039769589f,  1.0569715142f);

static const float3x3 P11_BT2020_TO_XYZ = float3x3(
    0.6369580483f, 0.1446169036f, 0.1688809752f,
    0.2627002120f, 0.6779980715f, 0.0593017165f,
    0.0000000000f, 0.0280726930f, 1.0609850577f);

static const float3x3 P11_XYZ_TO_BT2020 = float3x3(
     1.7166511880f, -0.3556707838f, -0.2533662814f,
    -0.6666843518f,  1.6164812366f,  0.0157685458f,
     0.0176398574f, -0.0427706133f,  0.9421031212f);

// Stockman-Sharp LMS from XYZ (CIE 170-2 physiological cone fundamentals).
static const float3x3 P11_XYZ_TO_SS_LMS = float3x3(
     0.2670502842655792f,   0.8471990148492798f,  -0.03470416612462053f,
    -0.38706882411220156f,  1.165429935890458f,    0.10302286696614202f,
     0.026727793989083093f,-0.02729131667566509f,  0.5333267257603284f);

// CIE 170-2 / MacLeod-Boynton luminance weights (Lw + Mw = physiological Y).
static const float3 P11_MB_WEIGHTS = float3(0.68990272f, 0.34832189f, 0.0371597f);
static const float2 P11_D65_XY     = float2(0.31272f, 0.32903f);

// ── Internal helpers ─────────────────────────────────────────────────────────

float3x3 p11_invert3x3(float3x3 m) {
    float a = m[0][0], b = m[0][1], c = m[0][2];
    float d = m[1][0], e = m[1][1], f = m[1][2];
    float g = m[2][0], h = m[2][1], i = m[2][2];
    float A =  e*i - f*h,  B = -(d*i - f*g), C =  d*h - e*g;
    float D = -(b*i - c*h), E =  a*i - c*g,  F = -(a*h - b*g);
    float G =  b*f - c*e,  H = -(a*f - c*d), I =  a*e - b*d;
    float det = a*A + b*B + c*C;
    float inv = (det == 0.f) ? 0.f : (1.f / det);
    return float3x3(A, D, G, B, E, H, C, F, I) * inv;
}

float p11_safe_div(float a, float b, float fallback) {
    return (b == 0.f) ? fallback : (a / b);
}

float3 p11_sign_pow(float3 x, float3 e) {
    return float3(
        (x.x < 0.f ? -1.f : 1.f) * pow(abs(x.x), e.x),
        (x.y < 0.f ? -1.f : 1.f) * pow(abs(x.y), e.y),
        (x.z < 0.f ? -1.f : 1.f) * pow(abs(x.z), e.z));
}

float3 p11_xyz_from_xyY(float3 xyY) {
    float3 xyz;
    xyz.xz = float2(xyY.x, 1.f - xyY.x - xyY.y) / xyY.y * xyY.z;
    xyz.y  = xyY.z;
    return xyz;
}

// ── Color-space conversions ───────────────────────────────────────────────────

float3 p11_bt2020_from_bt709(float3 c) {
    return mul(P11_XYZ_TO_BT2020, mul(P11_BT709_TO_XYZ, c));
}
float3 p11_bt709_from_bt2020(float3 c) {
    return mul(P11_XYZ_TO_BT709, mul(P11_BT2020_TO_XYZ, c));
}
float3 p11_lms_from_bt2020(float3 c) {
    return mul(P11_XYZ_TO_SS_LMS, mul(P11_BT2020_TO_XYZ, c));
}
float3 p11_bt2020_from_lms(float3 lms) {
    return mul(P11_XYZ_TO_BT2020, mul(p11_invert3x3(P11_XYZ_TO_SS_LMS), lms));
}

// ── MacLeod-Boynton chromaticity ─────────────────────────────────────────────

// Physiological luminance from LMS (CIE 170-2 weights, no S contribution).
float p11_stockman_lum(float3 lms) {
    return P11_MB_WEIGHTS.x * lms.x + P11_MB_WEIGHTS.y * lms.y;
}

// MB2 = (l, s, y_mb) where l = Lw/(Lw+Mw), s = Sw/(Lw+Mw), y_mb = Lw+Mw.
float3 p11_mb2_from_lms(float3 lms) {
    float wl = P11_MB_WEIGHTS.x * lms.x;
    float wm = P11_MB_WEIGHTS.y * lms.y;
    float y  = wl + wm;
    if (y <= 1e-12f) return float3(0.f, 0.f, 0.f);
    return float3(wl / y, P11_MB_WEIGHTS.z * lms.z / y, y);
}

float3 p11_lms_from_mb2(float3 mb2) {
    float y = max(mb2.z, 0.f);
    return float3(
        p11_safe_div(mb2.x * y,       P11_MB_WEIGHTS.x, 0.f),
        p11_safe_div((1.f - mb2.x)*y, P11_MB_WEIGHTS.y, 0.f),
        p11_safe_div(mb2.y * y,       P11_MB_WEIGHTS.z, 0.f));
}

// MB chromaticity (l, s) only — no achromatic component.
float2 p11_mb_from_lms(float3 lms) {
    float y = P11_MB_WEIGHTS.x * lms.x + P11_MB_WEIGHTS.y * lms.y;
    if (y <= 0.f) return float2(0.f, 0.f);
    return float2(
        p11_safe_div(P11_MB_WEIGHTS.x * lms.x, y, 0.f),
        p11_safe_div(P11_MB_WEIGHTS.z * lms.z, y, 0.f));
}

float2 p11_mb_from_bt2020(float3 rgb) {
    return p11_mb_from_lms(p11_lms_from_bt2020(rgb));
}

// D65 white chromaticity in MB space.
float2 p11_white_d65_mb() {
    float3 xyz = p11_xyz_from_xyY(float3(P11_D65_XY, 1.f));
    float3 lms = mul(P11_XYZ_TO_SS_LMS, xyz);
    return p11_mb2_from_lms(lms).xy;
}

// ── Gamut compression ─────────────────────────────────────────────────────────
// Adds enough D65-white LMS to push the chromaticity back inside the BT.2020
// triangle, preserving achromatic luminance.

float p11_cross2(float2 a, float2 b) { return a.x * b.y - a.y * b.x; }

bool p11_ray_segment_hit(float2 origin, float2 dir, float2 a, float2 b, out float t) {
    t = 0.f;
    float2 e    = b - a;
    float denom = p11_cross2(dir, e);
    if (abs(denom) <= 1e-20f) return false;
    float2 ao = a - origin;
    float  tt = p11_cross2(ao, e) / denom;
    float  u  = p11_cross2(ao, dir) / denom;
    if (tt < 0.f || u < 0.f || u > 1.f) return false;
    t = tt;
    return true;
}

float p11_max_t_bt2020_mb(float2 origin, float2 dir, out bool hit) {
    hit = false;
    if (dot(dir, dir) <= 1e-14f) return 0.f;
    float2 r = p11_mb_from_bt2020(float3(1.f, 0.f, 0.f));
    float2 g = p11_mb_from_bt2020(float3(0.f, 1.f, 0.f));
    float2 b = p11_mb_from_bt2020(float3(0.f, 0.f, 1.f));
    float t_best = 1e30f, t;
    bool any = false;
    if (p11_ray_segment_hit(origin, dir, r, g, t)) { t_best = min(t_best, t); any = true; }
    if (p11_ray_segment_hit(origin, dir, g, b, t)) { t_best = min(t_best, t); any = true; }
    if (p11_ray_segment_hit(origin, dir, b, r, t)) { t_best = min(t_best, t); any = true; }
    hit = any;
    return any ? max(t_best, 0.f) : 0.f;
}

float3 p11_gamut_compress(float3 lms) {
    float  y_mb  = P11_MB_WEIGHTS.x * abs(lms.x) + P11_MB_WEIGHTS.y * abs(lms.y);
    float2 white = p11_white_d65_mb();
    float2 mb0   = p11_mb_from_lms(lms);
    float2 dir   = mb0 - white;
    if (dot(dir, dir) < 1e-14f) return lms;
    bool hit;
    float t_max = p11_max_t_bt2020_mb(white, dir, hit);
    if (!hit) return lms;
    float wr    = max(p11_safe_div(1.f - t_max, t_max, 0.f), 0.f);
    float3 wlms = p11_lms_from_mb2(float3(white, 1.f));
    return lms + wlms * (y_mb * wr);
}

// ── Grading helpers ───────────────────────────────────────────────────────────

float p11_contrast_safe(float x, float c, float mg) {
    float r = x / mg;
    return ((r < 0.f ? -1.f : 1.f) * pow(abs(r), c)) * mg;
}

float p11_highlights(float x, float h, float mg) {
    if (h > 1.f) return max(x, lerp(x, mg * pow(x / mg, h), x));
    if (h < 1.f) return min(x, x / (1.f + mg * pow(x / mg, 2.f - h) - x));
    return x;
}

float p11_shadows(float x, float s, float mg) {
    if (s > 1.f) return max(x, x * (1.f + (x * mg / pow(x / mg, s))));
    if (s < 1.f) return clamp(x * (1.f - (x * mg / pow(x / mg, 2.f - s))), 0.f, x);
    return x;
}

// ── Tonemapping curves ────────────────────────────────────────────────────────

float p11_neutwo(float x, float peak) {
    return (peak * x) * rsqrt(mad(x, x, peak * peak));
}

float p11_neutwo_clipped(float x, float peak, float clip) {
    float cc = clip * clip, pp = peak * peak, xx = x * x;
    float num    = clip * peak * x;
    float den_sq = mad(xx, cc - pp, cc * pp);
    return num * rsqrt(den_sq);
}

float3 p11_neutwo_ch(float3 c, float3 peak) {
    return float3(p11_neutwo(c.r, peak.r), p11_neutwo(c.g, peak.g), p11_neutwo(c.b, peak.b));
}
float3 p11_neutwo_ch_clipped(float3 c, float3 peak, float3 clip) {
    return float3(
        p11_neutwo_clipped(c.r, peak.r, clip.r),
        p11_neutwo_clipped(c.g, peak.g, clip.g),
        p11_neutwo_clipped(c.b, peak.b, clip.b));
}

float3 p11_naka_rushton(float3 x, float3 peak, float3 gray, float n_exp) {
    float3 n   = n_exp * peak / (peak - gray);
    float3 xn  = p11_sign_pow(x, n);
    float3 num = peak * xn;
    float3 den = mad(pow(gray, n - 1.f), peak - gray, xn);
    return num / den;
}

// ── Hue / purity helpers ─────────────────────────────────────────────────────

float3 p11_restore_hue(float3 lms_src, float3 lms_tgt, float amount) {
    float restore = saturate(amount);
    if (restore <= 0.f) return lms_tgt;
    float3 mb_src = p11_mb2_from_lms(lms_src);
    float3 mb_tgt = p11_mb2_from_lms(lms_tgt);
    float2 white  = p11_white_d65_mb();
    float2 src_off = mb_src.xy - white;
    float2 tgt_off = mb_tgt.xy - white;
    float s2 = dot(src_off, src_off), t2 = dot(tgt_off, tgt_off);
    if (s2 <= 1e-6f || t2 <= 1e-6f) return lms_tgt;
    float2 src_dir = src_off * rsqrt(s2);
    float2 tgt_dir = tgt_off * rsqrt(t2);
    float2 blend   = lerp(tgt_dir, src_dir, restore);
    float  bl2     = dot(blend, blend);
    blend = (bl2 > 1e-6f) ? blend * rsqrt(bl2) : tgt_dir;
    float3 mb_out  = float3(white + blend * sqrt(t2), mb_tgt.z);
    return p11_lms_from_mb2(mb_out);
}

float3 p11_scale_purity(float3 lms, float scale) {
    if (abs(scale - 1.f) <= 1e-6f) return lms;
    float3 mb    = p11_mb2_from_lms(lms);
    float2 white = p11_white_d65_mb();
    float2 scaled = white + (mb.xy - white) * max(scale, 0.f);
    return p11_lms_from_mb2(float3(scaled, mb.z));
}

// ── Top-level PsychoV-11 entry point ─────────────────────────────────────────

float3 psychoV11(float3 bt709, uniform TonemapConstants p) {
    const float kEps        = 1e-6f;
    const float kHalfBleach = 20000.f; // half-bleach trolands ~10^4.3

    // Midgray reference in LMS (0.18 diffuse white in BT.2020 space).
    const float3 lms_midgray = p11_lms_from_bt2020(float3(0.18f, 0.18f, 0.18f));
    const float  lum_midgray = p11_stockman_lum(lms_midgray);

    // Stage 1: BT.709 → LMS + initial gamut compression.
    float3 lms = p11_lms_from_bt2020(p11_bt2020_from_bt709(bt709));
    lms = p11_gamut_compress(lms);

    // Stage 2: luminance-domain color grading.
    float lum   = p11_stockman_lum(lms);
    float lum_t = lum;
    if (p.psychoHighlights   != 1.f) lum_t = p11_highlights(lum_t, p.psychoHighlights,   lum_midgray);
    if (p.psychoShadows      != 1.f) lum_t = p11_shadows(lum_t,    p.psychoShadows,       lum_midgray);
    if (lum < lum_midgray) {
        if (p.psychoContrastLow  != 1.f) lum_t = p11_contrast_safe(lum_t, p.psychoContrastLow,  lum_midgray);
    } else {
        if (p.psychoContrastHigh != 1.f) lum_t = p11_contrast_safe(lum_t, p.psychoContrastHigh, lum_midgray);
    }

    float lum_scale = p11_safe_div(lum_t, lum, 1.f);
    float clip_pt   = 100.f * lum_scale;
    float3 lms_unit = lms * lum_scale;

    // Stage 3: purity (saturation) scaling in MacLeod-Boynton space.
    if (p.psychoPurity != 1.f) lms_unit = p11_scale_purity(lms_unit, p.psychoPurity);

    // Stage 4: adaptation contrast (Michaelis-Menten-like per-cone gain).
    float3 lms_pre_adapt = lms_unit;
    if (p.psychoAdaptContrast != 1.f) {
        float3 sigma  = max(lms_midgray, kEps.xxx);
        float  exp_ac = max(p.psychoAdaptContrast, kEps);
        float3 ax     = abs(lms_unit);
        float3 ax_n   = pow(ax, exp_ac);
        float3 s_n    = pow(sigma, exp_ac);
        float3 r_tgt  = ax_n / max(ax_n + s_n, kEps.xxx);
        float3 r_base = ax   / max(ax   + sigma, kEps.xxx);
        float3 gain   = r_tgt / max(r_base, kEps.xxx);
        float3 sgn = float3(
            lms_unit.x < 0.f ? -1.f : 1.f,
            lms_unit.y < 0.f ? -1.f : 1.f,
            lms_unit.z < 0.f ? -1.f : 1.f);
        lms_unit = sgn * (ax * gain);
        if (p.psychoHueRestore > 0.f)
            lms_unit = p11_restore_hue(lms_pre_adapt, lms_unit, p.psychoHueRestore);
    }

    // Stage 5: photopigment bleaching (Rushton-Henry availability model).
    if (p.psychoBleaching != 0.f) {
        float  lum_ad  = max(p11_stockman_lum(max(lms_unit, 0.f)), 0.18f);
        float3 lms_ad  = p11_lms_from_bt2020(float3(lum_ad, lum_ad, lum_ad));
        float3 td      = max(lms_ad, 0.f) * 203.f * 4.f;
        float3 avail   = lerp(1.f.xxx, 1.f.xxx / (1.f.xxx + td / kHalfBleach), p.psychoBleaching);
        float3 delta   = (lms_unit - lms_ad) * avail;
        lms_unit = lms_ad + delta;
    }

    // Stage 6: per-channel tone curve (Neutwo by default, Naka-Rushton if coneExp != 1).
    float3 lms_peak = p11_lms_from_bt2020(float3(p.psychoPeakValue, p.psychoPeakValue, p.psychoPeakValue));
    float3 lms_clip = p11_lms_from_bt2020(float3(clip_pt, clip_pt, clip_pt));
    float3 lms_toned;
    if (p.psychoConeExp != 1.f) {
        lms_toned = p11_naka_rushton(lms_unit, lms_peak, lms_midgray, p.psychoConeExp);
    } else if (clip_pt > p.psychoPeakValue) {
        lms_toned = p11_neutwo_ch_clipped(lms_unit, lms_peak, lms_clip);
    } else {
        lms_toned = p11_neutwo_ch(lms_unit, lms_peak);
    }

    // Stage 7: post-curve hue restoration.
    if (p.psychoHueRestore > 0.f)
        lms_toned = p11_restore_hue(lms_unit, lms_toned, p.psychoHueRestore);

    // Stage 8: final gamut compress + convert back to BT.709.
    lms_toned = p11_gamut_compress(lms_toned);
    return saturate(p11_bt709_from_bt2020(p11_bt2020_from_lms(lms_toned)));
}

// ────────────────────────────────────────────────────────────────────────────
// PsychoV-17  (by clshortfuse/RenoDX)
//
// Key advances over v11:
//   - CIE 170-2 human gamut halfspace boundary for purity signals / hue
//     sensitivity instead of BT.2020 triangle.
//   - All MB operations use weighted LMS (LMS * MB_WEIGHTS) so the
//     achromatic y term is Yf (≈ physiological luminance).
//   - Naka-Rushton with separate anchor_in (adaptive state) and anchor_out
//     (background state) — models observer adaptation more faithfully.
//   - Adaptive-relative gamut compression: display primaries are expressed
//     relative to the adapted state before compression.
//   - Hue restoration weights by purity-signal loss and adaptive hue
//     sensitivity (distant hues restored less aggressively).
//   - Purity scaling operates around the adapted neutral (unit LMS), not D65.
//
// Adapted state is fixed at 0.18 diffuse-white reference (no scene stats).
// Reference: https://github.com/clshortfuse/renodx
// ────────────────────────────────────────────────────────────────────────────

// CIE 170-2 human gamut boundary as 7 halfspace constraints (from RenoDX).
// The halfspaces are expressed relative to the D65 anchor in MB space.
static const int    P17_CIE_EDGE_COUNT = 7;
static const float2 P17_CIE_NORMALS[7] = {
    float2(-0.043889f, -0.006807f),
    float2(-0.007821f, -0.008564f),
    float2(-0.000604f, -0.007942f),
    float2( 0.000000f, -0.080835f),
    float2( 0.953597f,  0.307020f),
    float2(-0.060969f,  0.019752f),
    float2(-0.106895f,  0.004035f),
};
static const float P17_CIE_NUMER[7] = {
    0.0065035249f, 0.00104900495f, 0.000207697044f, 0.00165556648f,
    0.252472349f,  0.0241967351f,  0.0199621232f,
};

// MB (l, s) of unit equal LMS (1,1,1) in weighted space — the adapted neutral.
// Computed as WeighLMS(1,1,1) → MB → (l,s).
float2 p17_unit_lms_mb() {
    float y = P11_MB_WEIGHTS.x + P11_MB_WEIGHTS.y;
    return float2(P11_MB_WEIGHTS.x / y, P11_MB_WEIGHTS.z / y);
}

// Finds t at which a ray from `origin` in `direction` exits the CIE 1702
// human gamut (halfspace method). Uses D65 as the gamut-interior anchor.
float p17_ray_exit_cie1702(float2 origin, float2 direction) {
    const float kMax = 1e20f;
    if (dot(direction, direction) <= 1e-14f) return kMax;
    float2 d65 = p11_white_d65_mb();
    float2 w2o = d65 - origin; // vector from D65 to origin
    float t_best = kMax;
    bool  hit    = false;
    [unroll]
    for (int i = 0; i < P17_CIE_EDGE_COUNT; ++i) {
        float denom = dot(P17_CIE_NORMALS[i], direction);
        float numer = P17_CIE_NUMER[i] + dot(P17_CIE_NORMALS[i], w2o);
        float t     = (denom > 1e-8f) ? (numer / denom) : kMax;
        t_best = min(t_best, t);
        hit    = hit || (denom > 1e-8f);
    }
    return hit ? max(t_best, 0.f) : kMax;
}

// ── Weighted LMS (LMS * MB_WEIGHTS) ─────────────────────────────────────────

float3 p17_weigh_lms(float3 lms)    { return lms * P11_MB_WEIGHTS; }
float3 p17_unweigh_lms(float3 wlms) { return wlms / P11_MB_WEIGHTS; }

// MB (l, s, y_mb) from weighted LMS. y_mb = Lw + Mw.
float3 p17_mb_from_wlms(float3 wlms) {
    float y = wlms.x + wlms.y;
    if (y <= 1e-12f) return float3(0.f, 0.f, 0.f);
    return float3(wlms.x / y, wlms.z / y, y);
}
// Reconstruct weighted LMS from MB (l, s) and achromatic scale y_mb.
float3 p17_wlms_from_mb(float2 ls, float y_mb) {
    return float3(ls.x * y_mb, (1.f - ls.x) * y_mb, ls.y * y_mb);
}
float3 p17_wlms_from_mb(float3 mb) { return p17_wlms_from_mb(mb.xy, mb.z); }

// ── Gamut helpers ────────────────────────────────────────────────────────────

// Clamp weighted LMS to lie within the CIE 1702 human gamut boundary.
float3 p17_clamp_to_cie1702(float3 wlms) {
    float3 wc  = max(wlms, 0.f);
    float3 mb  = p17_mb_from_wlms(wc);
    float  y   = mb.z;
    if (!(y > 1e-20f)) return float3(wc.x, wc.y, 0.f);
    float2 d65 = p11_white_d65_mb();
    float2 dir = mb.xy - d65;
    if (dot(dir, dir) <= 1e-14f) return wc;
    float t_clip  = p17_ray_exit_cie1702(d65, dir);
    float2 ls_out = d65 + dir * min(1.f, t_clip);
    return p17_wlms_from_mb(ls_out, y);
}

// Soft-compression activation: ramps up as t_peak exceeds 1 (outside gamut).
float p17_soft_activation(float t_peak, float knee = 0.08f) {
    float outside = 1.f - saturate(t_peak);
    return outside / max(outside + knee, 1e-6f);
}
// Neutwo peak-clip scalar for soft gamut compression.
float p17_neutwo_pc(float x, float peak, float clip) {
    float cc = clip*clip, pp = peak*peak, xx = x*x;
    return (clip * peak * x) * rsqrt(max(mad(xx, cc-pp, cc*pp), 1e-20f));
}

// Adaptive-relative weighted LMS gamut compression.
// `wlms_rel`   — input in adaptive-relative weighted LMS space
// `adapt_lms`  — absolute adaptive state in LMS
// `rgb_to_lms_w` — matrix: column j = weighted LMS of j-th display primary
// `strength`   — blend from hard-clip (0) to Neutwo soft curve (1)
float3 p17_gamut_compress(float3 wlms_rel, float3 adapt_lms,
                           float3x3 rgb_to_lms_w, float strength)
{
    // Adaptive-relative MB for each display primary.
    // WeighLMS(primary) / adapt_lms (unweighted) matches RenoDX's
    // RGBToAdaptiveWeightedLMSPrimaryColumn semantics.
    float3 adapt_safe = max(adapt_lms, 1e-6f);
    float2 mb_r = p17_mb_from_wlms(float3(rgb_to_lms_w[0][0], rgb_to_lms_w[1][0], rgb_to_lms_w[2][0]) / adapt_safe).xy;
    float2 mb_g = p17_mb_from_wlms(float3(rgb_to_lms_w[0][1], rgb_to_lms_w[1][1], rgb_to_lms_w[2][1]) / adapt_safe).xy;
    float2 mb_b = p17_mb_from_wlms(float3(rgb_to_lms_w[0][2], rgb_to_lms_w[1][2], rgb_to_lms_w[2][2]) / adapt_safe).xy;

    // Clamp to CIE 1702 first, then work in MB.
    float3 wc  = p17_clamp_to_cie1702(max(wlms_rel, 0.f));
    float3 mb  = p17_mb_from_wlms(wc);
    float  y   = mb.z;
    if (!(y > 1e-20f)) return float3(wc.x, wc.y, 0.f);

    float2 d65 = p11_white_d65_mb();
    float2 dir = mb.xy - d65;
    if (dot(dir, dir) <= 1e-14f) return wc;

    // t at adaptive RGB triangle boundary.
    float t_peak = 1e30f;
    bool  any    = false;
    float t_hit;
    if (p11_ray_segment_hit(d65, dir, mb_r, mb_g, t_hit)) { t_peak = min(t_peak, t_hit); any = true; }
    if (p11_ray_segment_hit(d65, dir, mb_g, mb_b, t_hit)) { t_peak = min(t_peak, t_hit); any = true; }
    if (p11_ray_segment_hit(d65, dir, mb_b, mb_r, t_hit)) { t_peak = min(t_peak, t_hit); any = true; }

    float t_clip  = p17_ray_exit_cie1702(d65, dir);
    if (!any) t_peak = t_clip;
    else      t_peak = max(t_peak, 0.f);

    float t_hard  = saturate(t_peak);
    float t_soft  = saturate(p17_neutwo_pc(1.f, min(t_peak, t_clip), t_clip));
    float soft_mx = saturate(strength) * p17_soft_activation(t_peak);
    float t_final = lerp(t_hard, t_soft, soft_mx);

    float2 ls_out = d65 + t_final * dir;
    return p17_wlms_from_mb(ls_out, y);
}

// ── Tone curve ────────────────────────────────────────────────────────────────

// Naka-Rushton with separate input-anchor (adaptive state) and
// output-anchor (background state): f(anchor_in) = anchor_out.
float3 p17_naka_rushton(float3 x, float3 peak, float3 anchor_in, float3 anchor_out, float c) {
    float3 pmo = peak - anchor_out;
    float3 n   = c * peak / pmo;
    float3 con = anchor_out * pow(abs(x) / max(anchor_in, 1e-6f), n);
    float3 sat = peak * con / max(con + pmo, 1e-6f);
    return float3(
        x.x < 0.f ? -sat.x : sat.x,
        x.y < 0.f ? -sat.y : sat.y,
        x.z < 0.f ? -sat.z : sat.z);
}

// ── Hue restoration (v17 — uses CIE 1702 purity signals) ─────────────────────

float3 p17_restore_hue(float3 wlms_src, float3 wlms_tgt, float amount, float2 mb_anchor) {
    if (amount <= 0.f) return wlms_tgt;
    float3 mb_s = p17_mb_from_wlms(wlms_src);
    float3 mb_t = p17_mb_from_wlms(wlms_tgt);
    float2 src_off = mb_s.xy - mb_anchor;
    float2 tgt_off = mb_t.xy - mb_anchor;
    float  s2 = dot(src_off, src_off), t2 = dot(tgt_off, tgt_off);
    if (s2 <= 1e-7f || t2 <= 1e-7f) return wlms_tgt;

    // CIE 1702 purity signals — how far each point is from the gamut boundary.
    float t_src = p17_ray_exit_cie1702(mb_anchor, src_off);
    float t_tgt = p17_ray_exit_cie1702(mb_anchor, tgt_off);
    float src_p = saturate(p11_safe_div(1.f, t_src, 0.f));
    float tgt_p = saturate(p11_safe_div(1.f, t_tgt, 0.f));
    float loss  = saturate(p11_safe_div(tgt_p, max(src_p, 1e-6f), 0.f));

    // Adaptive hue sensitivity: long-ray (distant) hues are less aggressively restored.
    const float kMean = 0.20139844f, kMax = 1.02634534f, kMin = 0.35f;
    float sens = lerp(1.f, kMin, saturate((t_tgt - kMean) / max(kMax - kMean, 1e-6f)));

    float restore = saturate(amount * sens * loss);
    if (restore <= 0.f) return wlms_tgt;

    float2 src_dir = src_off * rsqrt(s2);
    float2 tgt_dir = tgt_off * rsqrt(t2);
    float2 blend   = lerp(tgt_dir, src_dir, restore);
    float  bl2     = dot(blend, blend);
    blend = (bl2 > 1e-7f) ? blend * rsqrt(bl2) : tgt_dir;

    float3 mb_out = float3(mb_anchor + blend * sqrt(t2), mb_t.z);
    return p17_wlms_from_mb(mb_out);
}

// ── Top-level PsychoV-17 entry point ─────────────────────────────────────────

float3 psychoV17(float3 bt709, uniform TonemapConstants p) {
    const float kEps    = 1e-6f;
    const float kHalfBt = 20000.f; // half-bleach trolands

    // Fixed adaptive state at diffuse white reference (0.18).
    // A full implementation would use rolling scene statistics.
    float3 adapt_lms = p11_lms_from_bt2020(float3(0.18f, 0.18f, 0.18f));
    float3 bg_lms    = adapt_lms; // background == adaptive state for static scenes

    // Build BT.2020→weighted-LMS matrix for gamut compression.
    // Column j = WeighLMS(LMS_of_BT2020_primary_j).
    float3 r_lms = p11_lms_from_bt2020(float3(1.f, 0.f, 0.f));
    float3 g_lms = p11_lms_from_bt2020(float3(0.f, 1.f, 0.f));
    float3 b_lms = p11_lms_from_bt2020(float3(0.f, 0.f, 1.f));
    float3x3 bt2020_to_wlms = float3x3(
        r_lms.x * P11_MB_WEIGHTS.x, g_lms.x * P11_MB_WEIGHTS.x, b_lms.x * P11_MB_WEIGHTS.x,
        r_lms.y * P11_MB_WEIGHTS.y, g_lms.y * P11_MB_WEIGHTS.y, b_lms.y * P11_MB_WEIGHTS.y,
        r_lms.z * P11_MB_WEIGHTS.z, g_lms.z * P11_MB_WEIGHTS.z, b_lms.z * P11_MB_WEIGHTS.z);

    // Stage 1: BT.709 → LMS.
    float3 lms_in   = p11_lms_from_bt2020(p11_bt2020_from_bt709(bt709));
    float3 lms_peak = p11_lms_from_bt2020(float3(p.psychoPeakValue, p.psychoPeakValue, p.psychoPeakValue));
    float  yf_mid   = p11_stockman_lum(adapt_lms);
    float  yf_in    = p11_stockman_lum(lms_in);

    // Stage 2: Luminance-domain grading (single contrast in v17).
    float yf_t = yf_in;
    if (p.psychoHighlights   != 1.f) yf_t = p11_highlights(yf_t, p.psychoHighlights, yf_mid);
    if (p.psychoShadows      != 1.f) yf_t = p11_shadows(yf_t, p.psychoShadows, yf_mid);
    if (p.psychoContrastHigh != 1.f) yf_t = p11_contrast_safe(yf_t, p.psychoContrastHigh, yf_mid);

    float  yf_scale   = p11_safe_div(yf_t, yf_in, 1.f);
    float3 lms_graded = lms_in * yf_scale;

    // Stage 3: Adaptive-relative purity scaling in MB space.
    if (p.psychoPurity != 1.f) {
        float3 lms_rel = lms_graded / max(adapt_lms, kEps.xxx);
        float3 wlms_r  = p17_weigh_lms(lms_rel);
        float3 mb_r    = p17_mb_from_wlms(wlms_r);
        float2 neutral = p17_unit_lms_mb();
        float2 scaled  = lerp(neutral, mb_r.xy, p.psychoPurity);
        lms_graded = p17_unweigh_lms(p17_wlms_from_mb(scaled, mb_r.z)) * adapt_lms;
    }

    float3 lms_cones = lms_graded;

    // Stage 4: Photopigment bleaching (Rushton-Henry availability model).
    if (p.psychoBleaching != 0.f) {
        float3 td    = max(adapt_lms, 0.f) * 203.f * 4.f; // trolands from adaptive state
        float3 avail = lerp(1.f.xxx, 1.f.xxx / (1.f.xxx + td / kHalfBt), p.psychoBleaching);
        float  y     = lms_cones.x + lms_cones.y;
        float  white_y = adapt_lms.x + adapt_lms.y;
        float3 white_at_y = adapt_lms * (y / max(white_y, kEps));
        lms_cones = white_at_y + (lms_cones - white_at_y) * avail;
    }

    // Stage 5: Naka-Rushton with adaptive anchors.
    float3 display = p17_naka_rushton(lms_cones, lms_peak, adapt_lms, bg_lms, p.psychoConeExp);

    // Adaptive-relative weighted LMS: WeighLMS(x) / adapt_lms (unweighted).
    // Dividing by unweighted adapt_lms preserves the MB weight ratios so that
    // p17_mb_from_wlms correctly computes adaptive-relative chromaticity.
    float3 adapt_safe      = max(adapt_lms, kEps.xxx);
    float3 disp_wrel       = p17_weigh_lms(display) / adapt_safe;
    float3 cones_wrel      = p17_weigh_lms(lms_cones) / adapt_safe;
    float2 mb_neutral      = p17_unit_lms_mb();

    // Stage 6: Hue restoration (CIE 1702 purity signals, v17 approach).
    if (p.psychoHueRestore > 0.f)
        disp_wrel = p17_restore_hue(cones_wrel, disp_wrel, p.psychoHueRestore, mb_neutral);

    // Stage 7: Adaptive-relative gamut compression against the display hull.
    disp_wrel = p17_gamut_compress(disp_wrel, adapt_lms, bt2020_to_wlms, 1.f);

    // Stage 8: Back to absolute LMS → BT.2020 → BT.709.
    // Inverse of WeighLMS(x)/adapt_lms: multiply by adapt_lms then unweigh.
    float3 final_lms = p17_unweigh_lms(disp_wrel * adapt_safe);
    return saturate(p11_bt709_from_bt2020(p11_bt2020_from_lms(final_lms)));
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
  else    ev = p.exposure;                            // ACES, PBR Neutral, AgX, PsychoV-11

  float3 x = max(hdr * ev, float3(0.0, 0.0, 0.0));
  float3 color;
  if      (p.tonemapOp == 1) color = reinhard(x, p);
  else if (p.tonemapOp == 2) color = hermite(x, p);
  else if (p.tonemapOp == 3) color = linearMap(x, p);
  else if (p.tonemapOp == 4) color = pbrNeutral(x);
  else if (p.tonemapOp == 5) color = agx(x);
  else if (p.tonemapOp == 6) color = psychoV11(x, p);
  else if (p.tonemapOp == 7) color = psychoV17(x, p);
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
