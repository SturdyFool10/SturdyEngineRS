// Tests extracted from crates/sturdy-engine/src/runtime.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn frame_timing_report_carries_latency_fields_when_available() {
    let summary = RuntimeTimingSummary {
        available: true,
        cpu_frame_time_ms: Some(4.0),
        gpu_wait_time_ms: Some(2.0),
        gpu_frame_time_ms: Some(6.0),
        present_to_display_ms: Some(8.0),
        total_latency_ms: Some(18.0),
        pass_timings: Vec::new(),
        cpu_mean_ms: Some(5.0),
        cpu_p95_ms: Some(7.0),
        cpu_p99_ms: Some(9.0),
        gpu_mean_ms: None,
        gpu_p95_ms: None,
        gpu_p99_ms: None,
        gpu_timeline: None,
    };

    let report = FrameTimingReport::from_summary(&summary).unwrap();

    assert_eq!(report.cpu_ms, 4.0);
    assert_eq!(report.gpu_wait_ms, Some(2.0));
    assert_eq!(report.gpu_ms, Some(6.0));
    assert_eq!(report.present_to_display_ms, Some(8.0));
    assert_eq!(report.total_latency_ms, Some(18.0));
}

#[test]
fn frame_timing_report_leaves_total_latency_absent_without_present_timing() {
    let summary = RuntimeTimingSummary {
        available: true,
        cpu_frame_time_ms: Some(4.0),
        gpu_wait_time_ms: None,
        gpu_frame_time_ms: Some(6.0),
        present_to_display_ms: None,
        total_latency_ms: None,
        pass_timings: Vec::new(),
        cpu_mean_ms: Some(5.0),
        cpu_p95_ms: Some(7.0),
        cpu_p99_ms: Some(9.0),
        gpu_mean_ms: None,
        gpu_p95_ms: None,
        gpu_p99_ms: None,
        gpu_timeline: None,
    };

    let report = FrameTimingReport::from_summary(&summary).unwrap();

    assert_eq!(report.present_to_display_ms, None);
    assert_eq!(report.total_latency_ms, None);
}

// ── BenchmarkSession / BenchmarkReport tests ─────────────────────────────────

fn make_sample(frame_index: u64, cpu_ms: f32, gpu_ms: Option<f32>) -> BenchmarkFrameSample {
    BenchmarkFrameSample {
        frame_index,
        cpu_ms,
        gpu_ms,
        gpu_wait_ms: None,
        pass_timings: Vec::new(),
    }
}

#[test]
fn benchmark_report_empty_session_returns_zero_frame_count() {
    let session = BenchmarkSession::new();
    let report = session.finish();
    assert_eq!(report.frame_count, 0);
    assert!(report.gpu_ms.is_none());
}

#[test]
fn benchmark_report_cpu_stats_are_correct() {
    let mut session = BenchmarkSession::new();
    for i in 0..4 {
        session.record(make_sample(i as u64, (i + 1) as f32 * 2.0, None));
    }
    // cpu_ms: [2.0, 4.0, 6.0, 8.0]
    let report = session.finish();
    assert_eq!(report.frame_count, 4);
    assert!((report.cpu_ms.min - 2.0).abs() < 0.01);
    assert!((report.cpu_ms.max - 8.0).abs() < 0.01);
    assert!((report.cpu_ms.mean - 5.0).abs() < 0.01);
}

#[test]
fn benchmark_report_gpu_stats_absent_when_no_gpu_times() {
    let mut session = BenchmarkSession::new();
    for i in 0..4 {
        session.record(make_sample(i as u64, 1.0, None));
    }
    let report = session.finish();
    assert!(report.gpu_ms.is_none());
}

#[test]
fn benchmark_report_gpu_stats_present_when_gpu_times_available() {
    let mut session = BenchmarkSession::new();
    for i in 0..4 {
        session.record(make_sample(i as u64, 1.0, Some((i + 1) as f32 * 3.0)));
    }
    let report = session.finish();
    assert!(report.gpu_ms.is_some());
    let gpu = report.gpu_ms.unwrap();
    assert!((gpu.min - 3.0).abs() < 0.01);
    assert!((gpu.max - 12.0).abs() < 0.01);
}

#[test]
fn benchmark_report_per_pass_stats_aggregated() {
    let mut session = BenchmarkSession::new();
    for i in 0..4u64 {
        session.record(BenchmarkFrameSample {
            frame_index: i,
            cpu_ms: 1.0,
            gpu_ms: None,
            gpu_wait_ms: None,
            pass_timings: vec![
                BenchmarkPassSample { name: "GBuffer".into(), gpu_ms: (i + 1) as f32 },
                BenchmarkPassSample { name: "Shadow".into(), gpu_ms: 0.5 },
            ],
        });
    }
    let report = session.finish();
    assert!(report.per_pass.contains_key("GBuffer"));
    assert!(report.per_pass.contains_key("Shadow"));
    let gbuffer = &report.per_pass["GBuffer"];
    assert!((gbuffer.min - 1.0).abs() < 0.01);
    assert!((gbuffer.max - 4.0).abs() < 0.01);
}

#[test]
fn benchmark_report_to_json_produces_valid_json() {
    let mut session = BenchmarkSession::new();
    session.record(make_sample(0, 5.0, Some(3.0)));
    let report = session.finish();
    let json = report.to_json().expect("serialization should succeed");
    assert!(json.contains("frame_count"));
    assert!(json.contains("cpu_ms"));
    assert!(json.contains("gpu_ms"));
}

#[test]
fn frame_stats_from_samples_computes_percentiles() {
    let samples: Vec<f32> = (1..=100).map(|i| i as f32).collect();
    let stats = FrameStats::from_samples(&samples);
    assert!((stats.min - 1.0).abs() < 0.01);
    assert!((stats.max - 100.0).abs() < 0.01);
    // p99 of 100 samples → index 98 (0-based) → value 99.0
    assert!((stats.p99 - 99.0).abs() < 1.0);
}

#[test]
fn is_benchmarking_reflects_session_state() {
    let mut session_opt: Option<BenchmarkSession> = None;
    assert!(session_opt.is_none());
    session_opt = Some(BenchmarkSession::new());
    assert!(session_opt.is_some());
    let _ = session_opt.take().map(|s| s.finish());
    assert!(session_opt.is_none());
}
