use serde_json::json;

use crate::{AnalysisResult, AppError, CaptureFile, PairedAnalysisResult, PairedCapture};

use super::{duration_s, timing_summary, transport_counts, warnings};

/// Renders the machine-readable JSON analysis report.
pub fn format_json_report(
    capture: &CaptureFile,
    analysis: &AnalysisResult,
) -> Result<String, AppError> {
    let timing = timing_summary(capture);
    let (starts, continues, stops) = transport_counts(capture);
    let report = json!({
        "backend": capture.backend,
        "source": capture.source,
        "timestamp_method": capture.timestamp_method,
        "ppqn": capture.ppqn,
        "application_version": capture.application_version,
        "pipewire_version": capture.environment.pipewire_version,
        "sample_rate_hz": timing.rate_hz,
        "quantum": timing.quantum,
        "clock_events": analysis.rows.len(),
        "duration_s": duration_s(capture),
        "missing_clocks": analysis.exclusions.inferred_missing_ticks,
        "duplicate_clocks": analysis.exclusions.duplicate_events,
        "anomalous_clocks": analysis.exclusions.anomalous_events,
        "transient_clocks": analysis.exclusions.transient_events,
        "rate_changes": timing.rate_changes,
        "quantum_changes": timing.quantum_changes,
        "starts": starts,
        "continues": continues,
        "stops": stops,
        "measured_bpm": analysis.measured_bpm,
        "fitted_period_ns": analysis.fitted_period_ns,
        "intercept_ns": analysis.intercept_ns,
        "phase": analysis.phase,
        "period": analysis.period,
        "startup": analysis.startup,
        "anomalies": analysis.anomalies,
        "exclusions": analysis.exclusions,
        "rows": analysis.rows,
        "warnings": warnings(capture, analysis.rows.len()),
    });
    Ok(serde_json::to_string_pretty(&report)?)
}

pub fn format_paired_json_report(
    capture: &PairedCapture,
    analysis: &PairedAnalysisResult,
) -> Result<String, AppError> {
    Ok(serde_json::to_string_pretty(&serde_json::json!({
        "reference": capture.reference,
        "returned": capture.returned,
        "pairing": analysis.pairing,
        "latency": analysis.latency,
        "reference_analysis": analysis.reference,
        "returned_analysis": analysis.returned,
    }))?)
}
