use crate::{AnalysisResult, CaptureFile};

use super::{duration_s, timing_summary, transport_counts, warnings};

fn ms(nanoseconds: f64) -> String {
    format!("{:.3} ms", nanoseconds / 1_000_000.0)
}

fn maybe_ms(value: f64, present: bool) -> String {
    if present { ms(value) } else { "n/a".to_owned() }
}

/// Renders the human-readable jitter analysis report.
pub fn format_report(capture: &CaptureFile, analysis: &AnalysisResult) -> String {
    let timing = timing_summary(capture);
    let (starts, continues, stops) = transport_counts(capture);
    let clock_events = analysis.rows.len();
    let mut report = String::from("MIDI Clock Jitter Analysis\n==========================\n");

    for warning in warnings(capture, clock_events) {
        report.push_str(&format!("\n{warning}\n"));
    }

    report.push_str("\nInput\n");
    report.push_str(&format!("  Backend                {}\n", capture.backend));
    report.push_str(&format!(
        "  Source                 {}\n",
        capture.source.display_name
    ));
    report.push_str(&format!(
        "  Timestamp source       {}\n",
        capture.timestamp_method
    ));
    report.push_str(&format!(
        "  Sample rate            {}\n",
        timing
            .rate_hz
            .map(|rate| format!("{rate:.2} Hz"))
            .unwrap_or_else(|| "unknown".to_owned())
    ));
    report.push_str(&format!(
        "  Quantum                {}\n",
        timing
            .quantum
            .map(|quantum| quantum.to_string())
            .unwrap_or_else(|| "unknown".to_owned())
    ));
    report.push_str(&format!("  Clock events           {clock_events}\n"));
    report.push_str(&format!(
        "  Duration               {:.2} s\n",
        duration_s(capture)
    ));
    report.push_str(&format!(
        "  Missing clocks         {}\n",
        analysis.exclusions.inferred_missing_ticks
    ));
    report.push_str(&format!(
        "  Anomalous clocks       {}\n",
        analysis.exclusions.duplicate_events + analysis.exclusions.anomalous_events
    ));
    report.push_str(&format!(
        "  Rate changes           {}\n",
        timing.rate_changes
    ));
    report.push_str(&format!(
        "  Quantum changes        {}\n",
        timing.quantum_changes
    ));
    report.push_str(&format!(
        "  Start / Continue / Stop  {starts} / {continues} / {stops}\n"
    ));

    report.push_str("\nClock\n");
    report.push_str(&format!("  PPQN                   {}\n", capture.ppqn));
    report.push_str(&format!(
        "  Measured BPM           {:.4}\n",
        analysis.measured_bpm
    ));
    report.push_str(&format!(
        "  Fitted period          {}\n",
        ms(analysis.fitted_period_ns)
    ));

    let phase = &analysis.phase;
    let has_phase = clock_events > 0;
    report.push_str("\nPhase jitter\n");
    report.push_str(&format!(
        "  RMS                    {}\n",
        maybe_ms(phase.rms_ns, has_phase)
    ));
    report.push_str(&format!(
        "  Standard deviation     {}\n",
        maybe_ms(phase.standard_deviation_ns, has_phase)
    ));
    report.push_str(&format!(
        "  Mean absolute          {}\n",
        maybe_ms(phase.mean_absolute_ns, has_phase)
    ));
    report.push_str(&format!(
        "  Median absolute        {}\n",
        maybe_ms(phase.median_absolute_ns, has_phase)
    ));
    report.push_str(&format!(
        "  P95 absolute           {}\n",
        maybe_ms(phase.p95_absolute_ns, has_phase)
    ));
    report.push_str(&format!(
        "  P99 absolute           {}\n",
        maybe_ms(phase.p99_absolute_ns, has_phase)
    ));
    report.push_str(&format!(
        "  Earliest               {}\n",
        maybe_ms(phase.minimum_ns, has_phase)
    ));
    report.push_str(&format!(
        "  Latest                 {}\n",
        maybe_ms(phase.maximum_ns, has_phase)
    ));
    report.push_str(&format!(
        "  Peak-to-peak           {}\n",
        maybe_ms(phase.peak_to_peak_ns, has_phase)
    ));

    let period = &analysis.period;
    let has_period = has_phase && clock_events > 1;
    report.push_str("\nPeriod jitter\n");
    report.push_str(&format!(
        "  Mean interval          {}\n",
        maybe_ms(period.mean_interval_ns, has_period)
    ));
    report.push_str(&format!(
        "  Standard deviation     {}\n",
        maybe_ms(period.standard_deviation_ns, has_period)
    ));
    report.push_str(&format!(
        "  RMS error              {}\n",
        maybe_ms(period.rms_error_ns, has_period)
    ));
    report.push_str(&format!(
        "  P95 absolute           {}\n",
        maybe_ms(period.p95_absolute_error_ns, has_period)
    ));
    report.push_str(&format!(
        "  P99 absolute           {}\n",
        maybe_ms(period.p99_absolute_error_ns, has_period)
    ));
    report.push_str(&format!(
        "  Minimum interval       {}\n",
        maybe_ms(period.minimum_interval_ns, has_period)
    ));
    report.push_str(&format!(
        "  Maximum interval       {}\n",
        maybe_ms(period.maximum_interval_ns, has_period)
    ));

    report
}
