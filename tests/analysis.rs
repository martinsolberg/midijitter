use std::path::Path;

use midijitter::{AnalysisOptions, AppError, CaptureFile, analyze};

fn load_fixture(name: &str) -> CaptureFile {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    let json = std::fs::read_to_string(path).unwrap();
    CaptureFile::from_json_str(&json).unwrap()
}

fn capture_with_clock_timestamps(timestamps_ns: &[i128]) -> CaptureFile {
    let events: Vec<_> = timestamps_ns
        .iter()
        .enumerate()
        .map(|(index, timestamp_ns)| {
            serde_json::json!({
                "sequence": index + 1,
                "timestamp_ns": timestamp_ns,
                "event": "clock",
                "timestamp_metadata": {
                    "pipewire": {
                        "cycle_position": 0,
                        "event_offset": 0,
                        "event_position": 0,
                        "rate_num": 1,
                        "rate_denom": 48_000,
                        "quantum": 256
                    }
                }
            })
        })
        .collect();
    let capture = serde_json::json!({
        "format_version": 1,
        "backend": "fixture",
        "source": { "identity": "fixture", "display_name": "Fixture" },
        "timestamp_method": "fixture",
        "ppqn": 24,
        "application_version": "test",
        "environment": { "operating_system": "test", "pipewire_version": null },
        "transitions": [],
        "events": events
    });

    CaptureFile::from_json_str(&capture.to_string()).unwrap()
}

#[test]
fn perfect_119_bpm_is_not_reported_as_jitter() {
    let result = analyze(
        &load_fixture("perfect-119.json"),
        AnalysisOptions::default(),
    )
    .unwrap();

    assert!((result.measured_bpm - 119.0).abs() < 0.001);
    assert!(result.phase.rms_ns < 1_000.0);
    assert!(result.period.rms_error_ns < 1_000.0);
}

#[test]
fn perfect_120_bpm_has_no_material_phase_or_period_error() {
    let result = analyze(
        &load_fixture("perfect-120.json"),
        AnalysisOptions::default(),
    )
    .unwrap();

    assert!((result.measured_bpm - 120.0).abs() < 0.001);
    assert!(result.phase.rms_ns < 1_000.0);
    assert!(result.period.rms_error_ns < 1_000.0);
    assert_eq!(result.exclusions.anomalous_events, 0);
}

#[test]
fn fixed_gaussian_jitter_reports_phase_metrics() {
    let result = analyze(&load_fixture("gaussian.json"), AnalysisOptions::default()).unwrap();

    assert!(result.phase.standard_deviation_ns > 100_000.0);
    assert!(result.phase.p99_absolute_ns >= result.phase.p95_absolute_ns);
    assert!(result.phase.peak_to_peak_ns > 500_000.0);
}

#[test]
fn alternating_phase_jitter_is_not_lost_in_period_measurement() {
    let result = analyze(
        &load_fixture("alternating.json"),
        AnalysisOptions::default(),
    )
    .unwrap();

    assert!(result.phase.rms_ns > 900_000.0);
    assert!(result.period.rms_error_ns > 1_500_000.0);
}

#[test]
fn slow_drift_is_fitted_as_a_changed_period() {
    let result = analyze(&load_fixture("drift.json"), AnalysisOptions::default()).unwrap();

    assert!(result.measured_bpm < 120.0);
    assert!(result.phase.rms_ns < 1_000.0);
}

#[test]
fn missing_ticks_are_inferred_and_excluded_from_one_tick_period_statistics() {
    let result = analyze(&load_fixture("missing.json"), AnalysisOptions::default()).unwrap();

    assert_eq!(result.exclusions.inferred_missing_ticks, 1);
    assert_eq!(result.exclusions.anomalous_events, 0);
    assert!(result.period.rms_error_ns < 1_000.0);
}

#[test]
fn duplicate_ticks_remain_in_rows_but_are_excluded_from_statistics() {
    let result = analyze(&load_fixture("duplicate.json"), AnalysisOptions::default()).unwrap();

    assert_eq!(result.exclusions.duplicate_events, 1);
    assert_eq!(result.rows.len(), 13);
    assert!(result.period.rms_error_ns < 1_000.0);
}

#[test]
fn out_of_order_duplicate_excludes_both_adjacent_periods_but_remains_a_row() {
    let result = analyze(
        &load_fixture("out-of-order-duplicate.json"),
        AnalysisOptions::default(),
    )
    .unwrap();

    assert_eq!(result.rows.len(), 7);
    assert_eq!(result.exclusions.duplicate_events, 1);
    assert_eq!(result.exclusions.excluded_from_period_statistics, 2);
    assert_eq!(result.period.minimum_interval_ns, 20_833_333.0);
    assert_eq!(result.period.maximum_interval_ns, 20_833_333.0);
    assert!(result.period.rms_error_ns < 1.0);
}

#[test]
fn outliers_remain_in_rows_but_are_excluded_from_fit_and_statistics() {
    let result = analyze(&load_fixture("outlier.json"), AnalysisOptions::default()).unwrap();

    assert!(result.exclusions.anomalous_events >= 1);
    assert_eq!(result.rows.len(), 12);
    assert!(result.phase.rms_ns < 1_000.0);
}

#[test]
fn inserted_outlier_does_not_shift_following_valid_clock_tick_indices() {
    let capture = capture_with_clock_timestamps(&[
        0,
        20_833_333,
        41_666_666,
        62_499_999,
        83_333_332,
        93_333_332,
        104_166_665,
        124_999_998,
        145_833_331,
        166_666_664,
    ]);

    let result = analyze(&capture, AnalysisOptions::default()).unwrap();

    assert!((result.measured_bpm - 120.0).abs() < 0.001);
    assert!(result.phase.rms_ns < 1_000.0);
    assert_eq!(result.exclusions.anomalous_events, 1);
    assert_eq!(result.exclusions.excluded_from_regression, 1);
    assert_eq!(result.exclusions.excluded_from_phase_statistics, 1);
    assert_eq!(result.exclusions.excluded_from_period_statistics, 2);
}

#[test]
fn duplicate_only_intervals_are_rejected_without_panicking() {
    // Rows: valid tick 0, duplicate at the same instant, valid tick 1. The
    // convergence guard passes (a clean step-1 row exists) but no adjacent
    // pair forms a normal one-tick interval.
    let capture = capture_with_clock_timestamps(&[0, 0, 20_833_333]);

    assert!(matches!(
        analyze(&capture, AnalysisOptions::default()),
        Err(AppError::InvalidCapture(message))
            if message == "clock analysis requires a normal one-tick interval"
    ));
}

#[test]
fn analysis_stops_after_the_configured_iteration_cap() {
    let capture = load_fixture("outlier.json");
    let options = AnalysisOptions {
        max_passes: 0,
        ..AnalysisOptions::default()
    };

    assert!(matches!(
        analyze(&capture, options),
        Err(AppError::AnalysisDidNotConverge)
    ));
}

#[test]
fn settle_window_marks_leading_burst_transient_but_keeps_rows() {
    // 5 zero-interval events (link-startup flush signature) then 120 perfect
    // 120 BPM clocks starting 1 ms after capture start.
    let mut timestamps = vec![0, 0, 0, 0, 0];
    let mut time = 1_000_000i128;
    for _ in 0..120 {
        timestamps.push(time);
        time += 20_833_333;
    }
    let capture = capture_with_clock_timestamps(&timestamps);
    let analysis = analyze(
        &capture,
        AnalysisOptions {
            settle_ns: 500_000_000,
            ..Default::default()
        },
    )
    .unwrap();
    let transient: Vec<_> = analysis.rows.iter().filter(|row| row.transient).collect();
    assert!(!transient.is_empty(), "leading burst must be marked");
    assert_eq!(analysis.rows.len(), timestamps.len(), "rows are preserved");
    assert!(
        analysis.rows.iter().filter(|row| !row.transient).count() >= 90,
        "live traffic stays usable"
    );
    let latest_live = analysis
        .rows
        .iter()
        .filter(|row| !row.transient)
        .map(|row| row.phase_error_ns)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        latest_live < 1_000_000.0,
        "startup excursion must not leak into live Latest, got {latest_live}"
    );
}

#[test]
fn settle_zero_disables_transient_marking() {
    let mut timestamps = vec![0, 0, 0];
    let mut time = 1_000_000i128;
    for _ in 0..60 {
        timestamps.push(time);
        time += 20_833_333;
    }
    let capture = capture_with_clock_timestamps(&timestamps);
    let analysis = analyze(&capture, AnalysisOptions::default()).unwrap();
    assert!(
        analysis.rows.iter().all(|row| !row.transient),
        "default options must mark nothing transient"
    );
    assert_eq!(analysis.exclusions.transient_events, 0);
}
