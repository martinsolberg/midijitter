use serde::Serialize;

use super::{
    fit::Fit,
    indexing::{EventDisposition, IndexedEvent, IntervalDisposition},
    statistics,
};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnalysisResult {
    pub measured_bpm: f64,
    pub fitted_period_ns: f64,
    pub intercept_ns: f64,
    pub phase: PhaseStatistics,
    pub period: PeriodStatistics,
    pub exclusions: ExclusionCounts,
    pub startup: StartupSummary,
    pub anomalies: AnomalySummary,
    pub rows: Vec<AnalysisRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnalysisRow {
    pub sequence: u64,
    pub timestamp_ns: i128,
    pub tick_index: i64,
    pub step: i64,
    pub disposition: EventDisposition,
    pub interval_disposition: IntervalDisposition,
    pub missing_before: u32,
    #[serde(skip)]
    pub duplicate: bool,
    #[serde(skip)]
    pub anomalous: bool,
    #[serde(skip)]
    pub transient: bool,
    pub phase_error_ns: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StartupSummary {
    pub transient_events: usize,
    pub zero_or_negative_backlog: usize,
    pub live_anchor_sequence: Option<u64>,
    pub live_anchor_timestamp_ns: Option<i128>,
    pub cadence_confirmation: usize,
    pub period_ns: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnomalyDetail {
    pub sequence: u64,
    pub timestamp_ns: i128,
    pub value_ns: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnomalySummary {
    pub count: usize,
    pub worst_phase: Option<AnomalyDetail>,
    pub worst_interval: Option<AnomalyDetail>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExclusionCounts {
    pub non_clock_events: usize,
    pub duplicate_events: usize,
    pub anomalous_events: usize,
    pub transient_events: usize,
    pub inferred_missing_ticks: usize,
    pub excluded_from_regression: usize,
    pub excluded_from_phase_statistics: usize,
    pub excluded_from_period_statistics: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PhaseStatistics {
    pub mean_ns: f64,
    pub standard_deviation_ns: f64,
    pub rms_ns: f64,
    pub mean_absolute_ns: f64,
    pub median_absolute_ns: f64,
    pub p95_absolute_ns: f64,
    pub p99_absolute_ns: f64,
    pub minimum_ns: f64,
    pub maximum_ns: f64,
    pub peak_to_peak_ns: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PeriodStatistics {
    pub mean_interval_ns: f64,
    pub standard_deviation_ns: f64,
    pub rms_error_ns: f64,
    pub minimum_interval_ns: f64,
    pub maximum_interval_ns: f64,
    pub p95_absolute_error_ns: f64,
    pub p99_absolute_error_ns: f64,
}

pub(crate) fn build_result(
    indexed: &[IndexedEvent<'_>],
    fit: Fit,
    event_count: usize,
    startup_cadence: usize,
    startup_period_ns: f64,
) -> Result<AnalysisResult, crate::AppError> {
    let rows: Vec<_> = indexed
        .iter()
        .map(|row| AnalysisRow {
            sequence: row.event.sequence,
            timestamp_ns: row.event.timestamp_ns,
            tick_index: row.tick_index,
            step: row.step,
            disposition: row.disposition,
            interval_disposition: row.interval_disposition,
            missing_before: row.missing_before,
            duplicate: row.disposition == EventDisposition::Duplicate,
            anomalous: row.disposition == EventDisposition::Anomalous,
            transient: row.disposition == EventDisposition::StartupTransient,
            phase_error_ns: row.event.timestamp_ns as f64
                - (fit.intercept_ns + fit.period_ns * row.tick_index as f64),
        })
        .collect();
    let phase_errors: Vec<_> = rows
        .iter()
        .filter(|row| is_clean_phase(row))
        .map(|row| row.phase_error_ns)
        .collect();
    let intervals: Vec<_> = rows
        .windows(2)
        .filter(|pair| is_clean_period(&pair[0], &pair[1]))
        .map(|pair| (pair[1].timestamp_ns - pair[0].timestamp_ns) as f64)
        .collect();
    let period_errors: Vec<_> = intervals
        .iter()
        .map(|interval| interval - fit.period_ns)
        .collect();
    if intervals.is_empty() {
        return Err(crate::AppError::InvalidCapture(
            "clock analysis requires a normal one-tick interval".to_owned(),
        ));
    }
    let phase = statistics::summarize(&phase_errors);
    let interval = statistics::summarize(&intervals);
    let period_error = statistics::summarize(&period_errors);
    let duplicate_events = rows
        .iter()
        .filter(|row| row.disposition == EventDisposition::Duplicate)
        .count();
    let anomalous_events = rows
        .iter()
        .filter(|row| row.disposition == EventDisposition::Anomalous)
        .count();
    let transient_events = rows
        .iter()
        .filter(|row| row.disposition == EventDisposition::StartupTransient)
        .count();
    let excluded_period = rows
        .windows(2)
        .filter(|pair| !is_clean_period(&pair[0], &pair[1]))
        .count();
    let zero_or_negative_backlog = rows
        .windows(2)
        .filter(|pair| {
            pair[1].disposition == EventDisposition::StartupTransient
                && pair[1].timestamp_ns <= pair[0].timestamp_ns
        })
        .count();
    let anomaly_rows = rows
        .iter()
        .filter(|row| row.disposition == EventDisposition::Anomalous);
    let worst_phase = anomaly_rows
        .clone()
        .max_by(|left, right| left.phase_error_ns.abs().total_cmp(&right.phase_error_ns.abs()))
        .map(|row| AnomalyDetail {
            sequence: row.sequence,
            timestamp_ns: row.timestamp_ns,
            value_ns: row.phase_error_ns,
        });
    let worst_interval = rows
        .windows(2)
        .filter(|pair| pair[1].disposition == EventDisposition::Anomalous)
        .map(|pair| {
            let interval = (pair[1].timestamp_ns - pair[0].timestamp_ns) as f64;
            AnomalyDetail {
                sequence: pair[1].sequence,
                timestamp_ns: pair[1].timestamp_ns,
                value_ns: interval,
            }
        })
        .max_by(|left, right| left.value_ns.abs().total_cmp(&right.value_ns.abs()));

    Ok(AnalysisResult {
        measured_bpm: 60_000_000_000.0 / (fit.period_ns * 24.0),
        fitted_period_ns: fit.period_ns,
        intercept_ns: fit.intercept_ns,
        phase: PhaseStatistics {
            mean_ns: phase.mean,
            standard_deviation_ns: phase.standard_deviation,
            rms_ns: phase.rms,
            mean_absolute_ns: phase.mean_absolute,
            median_absolute_ns: phase.median_absolute,
            p95_absolute_ns: phase.p95_absolute,
            p99_absolute_ns: phase.p99_absolute,
            minimum_ns: phase.minimum,
            maximum_ns: phase.maximum,
            peak_to_peak_ns: phase.maximum - phase.minimum,
        },
        period: PeriodStatistics {
            mean_interval_ns: interval.mean,
            standard_deviation_ns: interval.standard_deviation,
            rms_error_ns: period_error.rms,
            minimum_interval_ns: interval.minimum,
            maximum_interval_ns: interval.maximum,
            p95_absolute_error_ns: period_error.p95_absolute,
            p99_absolute_error_ns: period_error.p99_absolute,
        },
        exclusions: ExclusionCounts {
            non_clock_events: event_count - rows.len(),
            duplicate_events,
            anomalous_events,
            transient_events,
            inferred_missing_ticks: rows
                .iter()
                .map(|row| row.missing_before as usize)
                .sum(),
            excluded_from_regression: duplicate_events + anomalous_events + transient_events,
            excluded_from_phase_statistics: duplicate_events + anomalous_events + transient_events,
            excluded_from_period_statistics: excluded_period,
        },
        startup: StartupSummary {
            transient_events,
            zero_or_negative_backlog,
            live_anchor_sequence: rows
                .iter()
                .find(|row| row.disposition != EventDisposition::StartupTransient)
                .map(|row| row.sequence),
            live_anchor_timestamp_ns: rows
                .iter()
                .find(|row| row.disposition != EventDisposition::StartupTransient)
                .map(|row| row.timestamp_ns),
            cadence_confirmation: startup_cadence,
            period_ns: startup_period_ns,
        },
        anomalies: AnomalySummary {
            count: anomalous_events,
            worst_phase,
            worst_interval,
        },
        rows,
    })
}

pub fn is_clean_phase(row: &AnalysisRow) -> bool {
    row.disposition == EventDisposition::Valid
}

pub fn is_clean_period(previous: &AnalysisRow, current: &AnalysisRow) -> bool {
    is_clean_phase(previous)
        && is_clean_phase(current)
        && current.tick_index == previous.tick_index + 1
        && current.interval_disposition == IntervalDisposition::Normal
}
