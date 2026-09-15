use std::process::Command;

use midijitter::AppError;
use midijitter::backend::{MidiSource, select_source};

#[test]
fn devices_reports_unavailable_pipewire_with_a_nonzero_exit_status() {
    let output = Command::new(env!("CARGO_BIN_EXE_midijitter"))
        .args(["devices", "--backend", "pipewire"])
        .env("PIPEWIRE_REMOTE", "midijitter-test-unavailable-remote")
        .output()
        .expect("midijitter binary should run");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("PipeWire daemon unavailable"));
}

#[test]
fn duplicate_display_name_requires_a_stable_source_identity() {
    let sources = vec![
        source("USB MIDI", "usb-midi-a", "out", Some("101"), 10, 20),
        source("USB MIDI", "usb-midi-b", "out", Some("102"), 11, 21),
    ];

    let error = select_source(&sources, "USB MIDI").expect_err("duplicate name must be rejected");

    assert!(matches!(error, AppError::AmbiguousSource { .. }));
    assert_eq!(
        error.to_string(),
        "source selector \"USB MIDI\" is ambiguous; use one of: usb-midi-a/out#101, usb-midi-b/out#102"
    );
}

#[test]
fn source_selection_errors_have_documented_exit_codes() {
    let source = source("USB MIDI", "usb-midi", "out", Some("101"), 10, 20);

    let no_source = select_source(&[], "USB MIDI").expect_err("empty list must reject selection");
    let not_found = select_source(std::slice::from_ref(&source), "missing")
        .expect_err("unknown source must be rejected");
    let ambiguous = select_source(&[source.clone(), source], "USB MIDI")
        .expect_err("duplicate displayed name must be rejected");
    let permission = AppError::PipeWirePermissionDenied {
        detail: "access denied".to_owned(),
    };

    assert!(matches!(no_source, AppError::NoSource));
    assert_eq!(no_source.exit_code(), 5);
    assert!(matches!(not_found, AppError::SourceNotFound { .. }));
    assert_eq!(not_found.exit_code(), 5);
    assert!(matches!(ambiguous, AppError::AmbiguousSource { .. }));
    assert_eq!(ambiguous.exit_code(), 5);
    assert_eq!(permission.exit_code(), 4);
}

fn source(
    display_name: &str,
    node_name: &str,
    port_name: &str,
    object_serial: Option<&str>,
    node_id: u32,
    port_id: u32,
) -> MidiSource {
    MidiSource {
        display_name: display_name.to_owned(),
        node_name: node_name.to_owned(),
        port_name: port_name.to_owned(),
        object_serial: object_serial.map(str::to_owned),
        node_id,
        port_id,
        alsa_device: None,
    }
}

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_midijitter"))
}

fn temp_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("midijitter-cli-test-{}-{name}", std::process::id()))
}

fn write_temp_file(name: &str, contents: &str) -> std::path::PathBuf {
    let path = temp_path(name);
    std::fs::write(&path, contents).expect("test fixture should be writable");
    path
}

fn fixture_capture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture should exist")
}

#[test]
fn analyze_reports_full_text_metrics_for_a_valid_capture() {
    let capture = temp_path("perfect-120.json");
    std::fs::write(&capture, fixture_capture("perfect-120")).unwrap();

    let output = binary()
        .args(["analyze", &capture.to_string_lossy()])
        .output()
        .expect("midijitter binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "MIDI Clock Jitter Analysis",
        "Measured BPM",
        "Fitted period",
        "Phase jitter",
        "RMS",
        "Standard deviation",
        "Mean absolute",
        "Median absolute",
        "P95 absolute",
        "P99 absolute",
        "Peak-to-peak",
        "Period jitter",
        "Mean interval",
        "Minimum interval",
        "Maximum interval",
        "Missing clocks",
        "Duplicate clocks",
        "Excluded from fit",
        "Sample rate",
        "Quantum",
        "PipeWire version",
        "Warning: only 12 MIDI Clock events were captured",
    ] {
        assert!(stdout.contains(expected), "report is missing {expected:?}");
    }
}

#[test]
fn analyze_json_output_is_machine_readable() {
    let capture = temp_path("perfect-120.json");
    std::fs::write(&capture, fixture_capture("perfect-120")).unwrap();

    let output = binary()
        .args(["analyze", &capture.to_string_lossy(), "--json"])
        .output()
        .expect("midijitter binary should run");

    assert!(output.status.success());
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    let bpm = report["measured_bpm"]
        .as_f64()
        .expect("BPM should be a number");
    assert!((bpm - 120.0).abs() < 0.01, "unexpected BPM {bpm}");
    assert_eq!(report["clock_events"], 12);
}

#[test]
fn analyze_csv_output_uses_the_specified_columns() {
    let capture = temp_path("perfect-120.json");
    std::fs::write(&capture, fixture_capture("perfect-120")).unwrap();
    let csv_path = temp_path("perfect-120.csv");

    let output = binary()
        .args([
            "analyze",
            &capture.to_string_lossy(),
            "--csv",
            &csv_path.to_string_lossy(),
        ])
        .output()
        .expect("midijitter binary should run");

    assert!(output.status.success());
    let csv = std::fs::read_to_string(&csv_path).expect("CSV should be written");
    let mut lines = csv.lines();
    assert_eq!(
        lines.next().expect("CSV should have a header"),
        "event,tick_index,time_s,interval_ms,ideal_time_s,phase_error_ms,\
         period_error_ms,backend,cycle_position,event_offset,event_position"
    );
    let rows: Vec<_> = lines.collect();
    assert_eq!(rows.len(), 12);
    let first: Vec<_> = rows[0].split(',').collect();
    assert_eq!(first[0], "clock");
    assert_eq!(first[7], "pipewire");
    assert!(!first[8].is_empty(), "cycle_position should be present");
    // The first row has no predecessor, so interval columns stay blank.
    assert!(first[3].is_empty());
    assert!(first[6].is_empty());
    let second: Vec<_> = rows[1].split(',').collect();
    assert!(!second[3].is_empty(), "interval_ms should be present");
    assert!(!second[6].is_empty(), "period_error_ms should be present");
}

#[test]
fn analyze_rejects_malformed_json() {
    let capture = write_temp_file("malformed.json", "{ not json");

    let output = binary()
        .args(["analyze", &capture.to_string_lossy()])
        .output()
        .expect("midijitter binary should run");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("invalid capture JSON"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn analyze_rejects_unknown_format_versions() {
    let capture = write_temp_file(
        "future.json",
        r#"{"format_version":99,"backend":"pipewire","source":{"identity":"x","display_name":"x"},"timestamp_method":"x","ppqn":24,"application_version":"x","environment":{"operating_system":"x","pipewire_version":null},"transitions":[],"events":[]}"#,
    );

    let output = binary()
        .args(["analyze", &capture.to_string_lossy()])
        .output()
        .expect("midijitter binary should run");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unsupported capture format version 99"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn analyze_rejects_captures_without_usable_clocks() {
    let empty = write_temp_file(
        "empty.json",
        r#"{"format_version":1,"backend":"pipewire","source":{"identity":"x","display_name":"x"},"timestamp_method":"x","ppqn":24,"application_version":"x","environment":{"operating_system":"x","pipewire_version":null},"transitions":[],"events":[]}"#,
    );
    let output = binary()
        .args(["analyze", &empty.to_string_lossy()])
        .output()
        .expect("midijitter binary should run");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("at least two Clock events"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn analyze_rejects_a_single_clock_event() {
    let capture = write_temp_file(
        "single.json",
        r#"{"format_version":1,"backend":"pipewire","source":{"identity":"x","display_name":"x"},"timestamp_method":"x","ppqn":24,"application_version":"x","environment":{"operating_system":"x","pipewire_version":null},"transitions":[],"events":[{"sequence":0,"timestamp_ns":0,"event":"clock","timestamp_metadata":{"pipewire":{"cycle_position":0,"event_offset":0,"event_position":0,"rate_num":1,"rate_denom":48000,"quantum":1024}}}]}"#,
    );

    let output = binary()
        .args(["analyze", &capture.to_string_lossy()])
        .output()
        .expect("midijitter binary should run");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("at least two Clock events"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn analyze_rejects_graph_rate_transitions_with_a_warning() {
    let capture = temp_path("transition.json");
    std::fs::write(&capture, fixture_capture("transition")).unwrap();

    let output = binary()
        .args(["analyze", &capture.to_string_lossy()])
        .output()
        .expect("midijitter binary should run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Warning"),
        "expected a transition warning, got: {stderr}"
    );
    assert!(
        stderr.contains("graph-rate transition"),
        "expected a transition diagnostic, got: {stderr}"
    );
}

#[test]
fn analyze_rejects_missing_capture_files() {
    let missing = temp_path("does-not-exist.json");

    let output = binary()
        .args(["analyze", &missing.to_string_lossy()])
        .output()
        .expect("midijitter binary should run");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cannot read capture file"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn plot_command_creates_the_three_required_pngs() {
    let capture = temp_path("perfect-120.json");
    std::fs::write(&capture, fixture_capture("perfect-120")).unwrap();
    let dir = temp_path("plots");

    let output = binary()
        .args([
            "plot",
            &capture.to_string_lossy(),
            "--output-dir",
            &dir.to_string_lossy(),
        ])
        .output()
        .expect("midijitter binary should run");

    assert!(output.status.success());
    for file in ["phase.png", "period.png", "phase-histogram.png"] {
        let path = dir.join(file);
        let bytes = std::fs::read(&path).expect("plot file should exist");
        assert!(bytes.len() > 1_000, "{file} should not be trivially small");
        assert_eq!(
            &bytes[..8],
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
        );
    }
}

#[test]
fn rolling_command_tracks_a_tempo_trend() {
    let capture = temp_path("rolling-drift.json");
    let simulated = binary()
        .args([
            "simulate",
            "--bpm",
            "120",
            "--duration",
            "60",
            "--drift",
            "200ppm/s",
            "--output",
            &capture.to_string_lossy(),
        ])
        .output()
        .expect("midijitter binary should run");
    assert!(simulated.status.success());

    let output = binary()
        .args(["rolling", &capture.to_string_lossy(), "--window", "5"])
        .output()
        .expect("midijitter binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    assert_eq!(lines.next().unwrap(), "tick_index,time_s,rolling_bpm");
    let bpms: Vec<f64> = lines
        .map(|line| line.split(',').nth(2).unwrap().parse().unwrap())
        .collect();
    assert!(bpms.len() > 100);
    assert!(
        bpms[0] - bpms[bpms.len() - 1] > 0.5,
        "expected a falling tempo trend"
    );
}

#[test]
fn rolling_command_rejects_oversized_windows() {
    let capture = temp_path("rolling-short.json");
    std::fs::write(&capture, fixture_capture("perfect-120")).unwrap();

    let output = binary()
        .args(["rolling", &capture.to_string_lossy(), "--window", "60"])
        .output()
        .expect("midijitter binary should run");

    assert!(!output.status.success());
}

#[test]
fn plot_rejects_graph_rate_transitions_with_a_warning() {
    let capture = temp_path("transition.json");
    std::fs::write(&capture, fixture_capture("transition")).unwrap();

    let output = binary()
        .args([
            "plot",
            &capture.to_string_lossy(),
            "--output-dir",
            &temp_path("transition-plots").to_string_lossy(),
        ])
        .output()
        .expect("midijitter binary should run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Warning") && stderr.contains("graph-rate transition"),
        "expected a transition warning, got: {stderr}"
    );
}

#[test]
fn record_requires_exactly_one_termination_condition() {
    let output = binary()
        .args(["record", "--source", "x", "--output", "out.json"])
        .output()
        .expect("midijitter binary should run");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("exactly one of --duration or --ticks"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let output = binary()
        .args([
            "record",
            "--source",
            "x",
            "--duration",
            "10",
            "--ticks",
            "10",
            "--output",
            "out.json",
        ])
        .output()
        .expect("midijitter binary should run");
    assert_eq!(output.status.code(), Some(2));
}

fn alsa_capture_json() -> String {
    let mut value: serde_json::Value =
        serde_json::from_str(&fixture_capture("perfect-120")).expect("fixture should parse");
    value["backend"] = serde_json::Value::String("alsa-raw".to_owned());
    value["timestamp_method"] =
        serde_json::Value::String("ALSA timestamped RawMIDI (CLOCK_MONOTONIC_RAW)".to_owned());
    value["source"] = serde_json::json!({"identity": "hw:1,0,0", "display_name": "Test MIDI"});
    for event in value["events"]
        .as_array_mut()
        .expect("events should be an array")
    {
        let relative_ns = event["timestamp_ns"]
            .as_i64()
            .expect("timestamp should fit");
        event["timestamp_metadata"] = serde_json::json!({
            "alsa": {
                "absolute_ns": relative_ns + 9_000_000_000,
                "clock": "monotonic-raw",
                "timestamped_read": true,
            }
        });
    }
    serde_json::to_string(&value).expect("capture should serialize")
}

#[test]
fn analyze_accepts_alsa_captures_with_the_backend_label() {
    let capture = temp_path("alsa-120.json");
    std::fs::write(&capture, alsa_capture_json()).unwrap();
    let csv_path = temp_path("alsa-120.csv");

    let output = binary()
        .args([
            "analyze",
            &capture.to_string_lossy(),
            "--csv",
            &csv_path.to_string_lossy(),
        ])
        .output()
        .expect("midijitter binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("alsa-raw"),
        "report should name the backend"
    );

    let csv = std::fs::read_to_string(&csv_path).expect("CSV should be written");
    let mut lines = csv.lines();
    assert!(lines.next().unwrap().starts_with("event,tick_index"));
    let rows: Vec<_> = lines.collect();
    assert_eq!(rows.len(), 12);
    let fields: Vec<_> = rows[1].split(',').collect();
    assert_eq!(fields[7], "alsa-raw");
    assert!(fields[8].is_empty(), "ALSA rows leave graph columns blank");
    assert!(fields[9].is_empty());
    assert!(fields[10].is_empty());
}

#[test]
fn devices_lists_alsa_rawmidi_sources_without_crashing() {
    let output = binary()
        .args(["devices", "--backend", "alsa-raw"])
        .output()
        .expect("midijitter binary should run");

    assert!(output.status.success());
}

#[test]
fn record_rejects_unknown_alsa_devices() {
    let output = binary()
        .args([
            "record",
            "--backend",
            "alsa-raw",
            "--source",
            "hw:99,99,99",
            "--ticks",
            "10",
            "--output",
            &temp_path("alsa-missing.json").to_string_lossy(),
        ])
        .output()
        .expect("midijitter binary should run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no MIDI source is available")
            || stderr.contains("was not found")
            || stderr.contains("ALSA device unavailable"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn record_rejects_userspace_fallback_for_pipewire() {
    let output = binary()
        .args([
            "record",
            "--source",
            "x",
            "--ticks",
            "10",
            "--output",
            "out.json",
            "--allow-userspace-timestamps",
        ])
        .output()
        .expect("midijitter binary should run");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("only valid with --backend alsa-raw"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn record_reports_unavailable_pipewire() {
    let output = binary()
        .args([
            "record",
            "--source",
            "x",
            "--ticks",
            "10",
            "--output",
            &temp_path("unavailable.json").to_string_lossy(),
        ])
        .env("PIPEWIRE_REMOTE", "midijitter-test-unavailable-remote")
        .output()
        .expect("midijitter binary should run");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("PipeWire daemon unavailable"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
