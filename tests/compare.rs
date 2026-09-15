use std::process::Command;

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_midijitter"))
}

fn fixture_path(name: &str) -> String {
    format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"))
}

fn temp_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "midijitter-compare-test-{}-{name}",
        std::process::id()
    ))
}

#[test]
fn compare_renders_a_side_by_side_table() {
    let output = binary()
        .args([
            "compare",
            &fixture_path("perfect-120"),
            &fixture_path("gaussian"),
        ])
        .output()
        .expect("midijitter binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "perfect-120.json",
        "gaussian.json",
        "Backend",
        "Phase RMS",
        "Phase P99",
        "Period stddev",
        "Peak-to-peak",
        "Missing ticks",
        "cannot isolate",
    ] {
        assert!(
            stdout.contains(expected),
            "comparison is missing {expected:?}"
        );
    }
}

#[test]
fn compare_warns_when_captures_differ_in_backend() {
    let relabeled = temp_path("alsa-relabeled.json");
    let contents = std::fs::read_to_string(fixture_path("perfect-120")).unwrap();
    std::fs::write(
        &relabeled,
        contents.replacen("\"backend\":\"fixture\"", "\"backend\":\"alsa-raw\"", 1),
    )
    .unwrap();

    let output = binary()
        .args([
            "compare",
            &fixture_path("perfect-120"),
            &relabeled.to_string_lossy(),
        ])
        .output()
        .expect("midijitter binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Warning") && stdout.contains("backend"),
        "expected a backend-difference warning, got: {stdout}"
    );
}

#[test]
fn compare_warns_for_every_environmental_dimension() {
    let base = std::fs::read_to_string(fixture_path("perfect-120")).unwrap();
    let cases = [
        (
            "receiving interface",
            "\"identity\":\"fixture\"",
            "\"identity\":\"hw:9,9,9\"",
        ),
        (
            "sample rate",
            "\"rate_denom\":48000",
            "\"rate_denom\":44100",
        ),
        ("quantum", "\"quantum\":256", "\"quantum\":512"),
        (
            "PipeWire version",
            "\"pipewire_version\":null",
            "\"pipewire_version\":\"1.0.0\"",
        ),
        (
            "timestamp method",
            "\"timestamp_method\":\"fixture\"",
            "\"timestamp_method\":\"other-method\"",
        ),
    ];
    for (label, from, to) in cases {
        assert!(
            base.contains(from),
            "fixture should contain editable {from:?} for {label}"
        );
        let variant = temp_path(&format!("variant-{}.json", label.replace([' ', '/'], "-")));
        std::fs::write(&variant, base.replacen(from, to, 1)).unwrap();

        let output = binary()
            .args([
                "compare",
                &fixture_path("perfect-120"),
                &variant.to_string_lossy(),
            ])
            .output()
            .expect("midijitter binary should run");

        assert!(
            output.status.success(),
            "variant for {label} should analyze"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Warning") && stdout.contains(label),
            "expected a {label}-difference warning, got: {stdout}"
        );
    }
}

#[test]
fn compare_requires_at_least_two_captures() {
    let output = binary()
        .args(["compare", &fixture_path("perfect-120")])
        .output()
        .expect("midijitter binary should run");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("at least two captures"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
