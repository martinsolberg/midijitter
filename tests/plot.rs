use std::path::PathBuf;

use midijitter::plot::render_plots;
use midijitter::{AnalysisOptions, CaptureFile, analyze};

fn load_fixture(name: &str) -> CaptureFile {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(format!("{name}.json"));
    let json = std::fs::read_to_string(path).unwrap();
    CaptureFile::from_json_str(&json).unwrap()
}

fn output_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "midijitter-plot-test-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn assert_valid_png(path: &std::path::Path) {
    let bytes = std::fs::read(path).expect("plot file should exist");
    assert!(
        bytes.len() > 1_000,
        "plot file should not be trivially small"
    );
    assert_eq!(
        &bytes[..8],
        &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
        "plot file should be a PNG"
    );
}

#[test]
fn render_plots_creates_the_three_required_pngs() {
    let capture = load_fixture("perfect-120");
    let analysis = analyze(&capture, AnalysisOptions::default()).unwrap();
    let dir = output_dir("perfect");

    let created = render_plots(&capture, &analysis, &dir).unwrap();

    assert_eq!(created.len(), 3);
    for path in &created {
        assert_valid_png(path);
    }
    assert_valid_png(&dir.join("phase.png"));
    assert_valid_png(&dir.join("period.png"));
    assert_valid_png(&dir.join("phase-histogram.png"));
}

#[test]
fn render_plots_handles_jittered_and_gapped_captures() {
    for fixture in ["gaussian", "missing", "outlier", "drift"] {
        let capture = load_fixture(fixture);
        let analysis = analyze(&capture, AnalysisOptions::default()).unwrap();
        let dir = output_dir(fixture);

        let created = render_plots(&capture, &analysis, &dir).unwrap();

        assert_eq!(
            created.len(),
            3,
            "fixture {fixture} should render all plots"
        );
        for path in &created {
            assert_valid_png(path);
        }
    }
}
