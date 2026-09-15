use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::analysis::{AnalysisOptions, AnalysisResult, analyze};
use crate::{AppError, CaptureFile};

use super::{duration_s, timing_summary};

pub const INDEPENDENT_RUNS_NOTE: &str = "Results from independent runs cannot isolate a backend's \
    contribution exactly because the source itself may vary between runs.";

struct ComparedCapture {
    name: String,
    capture: CaptureFile,
    analysis: AnalysisResult,
}

fn ms(nanoseconds: f64) -> String {
    format!("{:.3} ms", nanoseconds / 1_000_000.0)
}

/// Compares jitter analyses side by side.
///
/// Each capture is re-analyzed from its stored events; independent runs are
/// never treated as synchronized measurements.
pub fn format_comparison(paths: &[PathBuf]) -> Result<String, AppError> {
    if paths.len() < 2 {
        return Err(AppError::InvalidCapture(
            "compare requires at least two captures".to_owned(),
        ));
    }
    let mut compared = Vec::with_capacity(paths.len());
    for path in paths {
        let capture = CaptureFile::read_from_file(path)?;
        let analysis = analyze(&capture, AnalysisOptions::default())?;
        compared.push(ComparedCapture {
            name: display_name(path),
            capture,
            analysis,
        });
    }

    let width = compared
        .iter()
        .map(|entry| entry.name.len())
        .max()
        .unwrap_or(0)
        .max(16)
        + 2;
    let mut output = String::new();
    output.push_str(&format!("{:width$}", "", width = width));
    for entry in &compared {
        output.push_str(&format!("{:width$}", entry.name, width = width));
    }
    output.push('\n');
    output.push('\n');

    let rows: Vec<(&str, Vec<String>)> = vec![
        (
            "Backend",
            compared
                .iter()
                .map(|entry| entry.capture.backend.clone())
                .collect(),
        ),
        (
            "Phase RMS",
            compared
                .iter()
                .map(|entry| ms(entry.analysis.phase.rms_ns))
                .collect(),
        ),
        (
            "Phase P99",
            compared
                .iter()
                .map(|entry| ms(entry.analysis.phase.p99_absolute_ns))
                .collect(),
        ),
        (
            "Period stddev",
            compared
                .iter()
                .map(|entry| ms(entry.analysis.period.standard_deviation_ns))
                .collect(),
        ),
        (
            "Peak-to-peak",
            compared
                .iter()
                .map(|entry| ms(entry.analysis.phase.peak_to_peak_ns))
                .collect(),
        ),
        (
            "Missing ticks",
            compared
                .iter()
                .map(|entry| entry.analysis.exclusions.inferred_missing_ticks.to_string())
                .collect(),
        ),
        (
            "Duration",
            compared
                .iter()
                .map(|entry| format!("{:.2} s", duration_s(&entry.capture)))
                .collect(),
        ),
    ];
    for (label, values) in rows {
        output.push_str(&format!("{label:<16}"));
        for value in values {
            output.push_str(&format!("{value:width$}", width = width));
        }
        output.push('\n');
    }

    let warnings = environment_warnings(&compared);
    if !warnings.is_empty() {
        output.push('\n');
        for warning in warnings {
            output.push_str(&format!("Warning: {warning}\n"));
        }
    }
    output.push_str(&format!("\nNote: {INDEPENDENT_RUNS_NOTE}\n"));
    Ok(output)
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Warns when captures differ in variables that affect interpretation.
fn environment_warnings(compared: &[ComparedCapture]) -> Vec<String> {
    let mut warnings = Vec::new();
    let mut check = |label: &str, values: BTreeSet<String>| {
        if values.len() > 1 {
            warnings.push(format!(
                "captures differ in {label}: {}",
                values.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }
    };

    check(
        "backend",
        compared
            .iter()
            .map(|entry| entry.capture.backend.clone())
            .collect(),
    );
    check(
        "receiving interface",
        compared
            .iter()
            .map(|entry| entry.capture.source.identity.clone())
            .collect(),
    );
    check(
        "sample rate",
        compared
            .iter()
            .map(|entry| {
                timing_summary(&entry.capture)
                    .rate_hz
                    .map(|rate| format!("{rate:.2} Hz"))
                    .unwrap_or_else(|| "unknown".to_owned())
            })
            .collect(),
    );
    check(
        "PipeWire version",
        compared
            .iter()
            .map(|entry| {
                entry
                    .capture
                    .environment
                    .pipewire_version
                    .clone()
                    .unwrap_or_else(|| "unknown".to_owned())
            })
            .collect(),
    );
    check(
        "timestamp method",
        compared
            .iter()
            .map(|entry| entry.capture.timestamp_method.clone())
            .collect(),
    );
    warnings
}
