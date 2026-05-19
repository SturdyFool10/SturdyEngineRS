// Tests extracted from crates/sturdy-engine/src/runtime.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn frame_timing_report_carries_latency_fields_when_available() {
    let summary = RuntimeTimingSummary {
        available: true,
        cpu_frame_time_ms: Some(4.0),
        gpu_frame_time_ms: Some(6.0),
        present_to_display_ms: Some(8.0),
        total_latency_ms: Some(18.0),
        pass_timings: Vec::new(),
        cpu_mean_ms: Some(5.0),
        cpu_p95_ms: Some(7.0),
        cpu_p99_ms: Some(9.0),
    };

    let report = FrameTimingReport::from_summary(&summary).unwrap();

    assert_eq!(report.cpu_ms, 4.0);
    assert_eq!(report.gpu_ms, Some(6.0));
    assert_eq!(report.present_to_display_ms, Some(8.0));
    assert_eq!(report.total_latency_ms, Some(18.0));
}

#[test]
fn frame_timing_report_leaves_total_latency_absent_without_present_timing() {
    let summary = RuntimeTimingSummary {
        available: true,
        cpu_frame_time_ms: Some(4.0),
        gpu_frame_time_ms: Some(6.0),
        present_to_display_ms: None,
        total_latency_ms: None,
        pass_timings: Vec::new(),
        cpu_mean_ms: Some(5.0),
        cpu_p95_ms: Some(7.0),
        cpu_p99_ms: Some(9.0),
    };

    let report = FrameTimingReport::from_summary(&summary).unwrap();

    assert_eq!(report.present_to_display_ms, None);
    assert_eq!(report.total_latency_ms, None);
}
