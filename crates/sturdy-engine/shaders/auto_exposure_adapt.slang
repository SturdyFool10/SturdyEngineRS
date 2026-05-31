// Auto-exposure adapt compute pass.
//
// Consumes the 256-bin log-luminance histogram produced by
// `auto_exposure_histogram.slang`, derives a target EV100 value from the
// requested percentile of the luminance distribution, blends it toward the
// previous frame's stored EV at the configured adaptation speed, clamps to the
// configured EV range, and writes the new EV back into the persistent exposure
// buffer.
//
// Output buffer layout (`exposure_state`):
//   element 0 — current adapted EV100 (this frame's exposure)
//   element 1 — last raw target EV100 (for diagnostics)
//   element 2 — total weighted-sample count from histogram (for diagnostics)
//   element 3 — average luminance derived from histogram (for diagnostics)
//
// The pass clears the histogram bins as it reads them so the next frame's
// histogram pass starts from zero without a separate clear dispatch.

RWStructuredBuffer<uint>  histogram;
RWStructuredBuffer<float> exposure_state;

struct AdaptConstants {
    float min_log_luma;
    float log_luma_range;      // max_log_luma - min_log_luma
    float min_ev;
    float max_ev;
    float target_percentile;
    float adaptation_rate;     // 1.0 - exp(-dt * speed); precomputed on CPU
    float dt_seconds;
    float _pad;
};
[[vk::push_constant]] AdaptConstants push;

groupshared uint   bin_counts[256];
groupshared float  weighted[256];
groupshared uint   total_count;

[numthreads(256, 1, 1)]
void main(uint3 dt_id : SV_DispatchThreadID, uint thread_id : SV_GroupIndex) {
    uint count = histogram[thread_id];
    bin_counts[thread_id] = count;
    // Weighted sum used to derive the average log luminance.  Use the bin
    // centre in normalised log space.
    float bin_centre_normalised = (float(thread_id) - 0.5) / 254.0;
    bin_centre_normalised = saturate(bin_centre_normalised);
    float bin_log_luma = push.min_log_luma + bin_centre_normalised * push.log_luma_range;
    weighted[thread_id] = float(count) * bin_log_luma;
    if (thread_id == 0u) {
        total_count = 0u;
    }
    GroupMemoryBarrierWithGroupSync();

    // Single-threaded reduction.  The histogram is only 256 bins; the cost of
    // sequential work for one thread is negligible compared to launch
    // overhead.  Using one thread keeps the percentile logic readable.
    if (thread_id == 0u) {
        uint total = 0u;
        float weighted_log_sum = 0.0;
        for (uint i = 0u; i < 256u; ++i) {
            total += bin_counts[i];
            weighted_log_sum += weighted[i];
        }
        total_count = total;

        // Exclude bin 0 (essentially-black / no-hit-ray pixels) from the mean.
        // Bin 0 captures luminance below ~min_log_luma and would otherwise
        // drag the mean down to -10, causing massive over-exposure in path-traced
        // scenes where most rays miss geometry (black background).
        uint total_valid = total - bin_counts[0];
        float valid_weighted_sum = weighted_log_sum - weighted[0];

        float target_log_luma;
        if (total_valid == 0u) {
            target_log_luma = push.min_log_luma;
        } else {
            // Use the weighted mean of log luminance across valid (non-black) pixels.
            // This matches Frostbite / Filament eye-adaptation behaviour and is more
            // stable than a hard percentile when scenes contain large flat regions.
            // The `target_percentile` knob applies a small log-space bias so users
            // can still bias toward highlights (>0.5) or shadows (<0.5).
            float mean_log_luma = valid_weighted_sum / float(total_valid);
            float bias = (push.target_percentile - 0.5) * push.log_luma_range;
            target_log_luma = mean_log_luma - bias;
        }

        // EV100 = log2(luma * 100 / 12.5) = log2(luma) + log2(8.0)
        // = log2(luma) + 3.  Camera-style 18% middle-grey calibration.
        float target_ev = target_log_luma + 3.0;
        target_ev = clamp(target_ev, push.min_ev, push.max_ev);

        float previous_ev = exposure_state[0];
        float adapted_ev  = lerp(previous_ev, target_ev, saturate(push.adaptation_rate));

        exposure_state[0] = adapted_ev;
        exposure_state[1] = target_ev;
        exposure_state[2] = float(total);
        // Average linear luminance is exp2(mean_log_luma) when total > 0.
        float avg_luma = (total > 0u) ? exp2(weighted_log_sum / float(total)) : 0.0;
        exposure_state[3] = avg_luma;
    }

    // Clear histogram for the next frame so the histogram pass can start at
    // zero without an extra clear dispatch.
    GroupMemoryBarrierWithGroupSync();
    histogram[thread_id] = 0u;
}
