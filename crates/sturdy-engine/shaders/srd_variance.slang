// SRD variance and moments tracking.
//
// Online luminance moment estimation used by the radiance family to drive
// adaptive spatial reconstruction. We use a Welford-style update so a single
// bright outlier does not poison the running variance.
//
// References (public literature):
//   * Welford, "Note on a method for calculating corrected sums of squares
//     and products," 1962.
//   * Schied et al., "Spatiotemporal Variance-Guided Filtering" (SVGF), 2017
//     — informs the idea of using a running per-pixel variance to widen the
//     reconstruction kernel, not its exact pass split.

import srd_common;

struct SrdMoments {
    float mean;     // running mean of luminance
    float m2;       // running second central moment (sum of squared deltas)
    float count;    // history length in frames (clamped on the caller side)
};

// Online Welford update. `current` is this frame's luminance; the returned
// moments incorporate it.
SrdMoments srd_welford_update(SrdMoments m, float current) {
    SrdMoments next;
    next.count = m.count + 1.0f;
    float delta = current - m.mean;
    next.mean = m.mean + delta / max(next.count, 1.0f);
    float delta2 = current - next.mean;
    next.m2 = m.m2 + delta * delta2;
    return next;
}

// Sample variance from the running moments. Returns 0 for fewer than 2
// observations.
float srd_variance(SrdMoments m) {
    if (m.count < 2.0f) return 0.0f;
    return max(m.m2 / (m.count - 1.0f), 0.0f);
}

float srd_stddev(SrdMoments m) {
    return sqrt(srd_variance(m));
}

// Clamp the count so the running statistics stay responsive to long-running
// scene changes. Without this the variance would shrink without bound and the
// reconstruction kernel would never widen again after a long calm period.
SrdMoments srd_decay_count(SrdMoments m, float max_history_frames) {
    SrdMoments next = m;
    next.count = min(m.count, max(max_history_frames, 1.0f));
    return next;
}

// Pack two-channel moments into a half4 footprint: (mean, second_moment).
// The second moment is stored as raw E[L^2] rather than central moment for
// numerical robustness across history rotation.
float2 srd_pack_moments_e_l2(SrdMoments m) {
    float second = m.mean * m.mean + (m.count >= 2.0f ? m.m2 / m.count : 0.0f);
    return float2(m.mean, second);
}

SrdMoments srd_unpack_moments_e_l2(float2 packed, float count) {
    SrdMoments m;
    m.mean = packed.x;
    m.count = count;
    m.m2 = max(packed.y - packed.x * packed.x, 0.0f) * max(count, 1.0f);
    return m;
}
