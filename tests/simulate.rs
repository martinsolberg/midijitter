use std::process::Command;

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_midijitter"))
}

fn temp_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "midijitter-simulate-test-{}-{name}",
        std::process::id()
    ))
}

fn simulate(args: &[&str]) -> std::process::Output {
    binary()
        .arg("simulate")
        .args(args)
        .output()
        .expect("binary should run")
}

#[test]
fn identical_invocations_produce_identical_files() {
    let first = temp_path("a.json");
    let second = temp_path("b.json");

    for path in [&first, &second] {
        let output = simulate(&[
            "--bpm",
            "120",
            "--duration",
            "5",
            "--output",
            &path.to_string_lossy(),
        ]);
        assert!(output.status.success());
    }

    assert_eq!(
        std::fs::read(&first).unwrap(),
        std::fs::read(&second).unwrap()
    );
}

#[test]
fn perfect_simulation_analyzes_with_zero_jitter() {
    let capture = temp_path("perfect.json");
    let output = simulate(&[
        "--bpm",
        "120",
        "--duration",
        "5",
        "--output",
        &capture.to_string_lossy(),
    ]);
    assert!(output.status.success());

    let output = binary()
        .args(["analyze", &capture.to_string_lossy(), "--json"])
        .output()
        .expect("binary should run");
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!((report["measured_bpm"].as_f64().unwrap() - 120.0).abs() < 0.001);
    assert!(report["phase"]["rms_ns"].as_f64().unwrap() < 1_000.0);
}

#[test]
fn gaussian_jitter_is_recovered() {
    let capture = temp_path("jitter.json");
    let output = simulate(&[
        "--bpm",
        "120",
        "--duration",
        "30",
        "--jitter-std",
        "1ms",
        "--output",
        &capture.to_string_lossy(),
    ]);
    assert!(output.status.success());

    let output = binary()
        .args(["analyze", &capture.to_string_lossy(), "--json"])
        .output()
        .expect("binary should run");
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let rms_ms = report["phase"]["rms_ns"].as_f64().unwrap() / 1_000_000.0;
    assert!(
        (0.7..1.3).contains(&rms_ms),
        "expected phase RMS near 1 ms, got {rms_ms}"
    );
}

#[test]
fn missing_and_duplicate_rates_are_detected() {
    let capture = temp_path("anomalies.json");
    let output = simulate(&[
        "--bpm",
        "120",
        "--duration",
        "30",
        "--missing-rate",
        "0.05",
        "--duplicate-rate",
        "0.05",
        "--output",
        &capture.to_string_lossy(),
    ]);
    assert!(output.status.success());

    let output = binary()
        .args(["analyze", &capture.to_string_lossy(), "--json"])
        .output()
        .expect("binary should run");
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report["missing_clocks"].as_u64().unwrap() > 0);
    assert!(
        report["exclusions"]["duplicate_events"].as_u64().unwrap()
            + report["exclusions"]["anomalous_events"].as_u64().unwrap()
            > 0
    );
}

#[test]
fn invalid_simulation_configs_are_rejected() {
    let output = simulate(&[
        "--bpm",
        "0",
        "--duration",
        "5",
        "--output",
        &temp_path("bad.json").to_string_lossy(),
    ]);
    assert!(!output.status.success());

    let output = simulate(&[
        "--bpm",
        "120",
        "--duration",
        "5",
        "--jitter-std",
        "1",
        "--output",
        &temp_path("bad.json").to_string_lossy(),
    ]);
    assert!(!output.status.success());
}
