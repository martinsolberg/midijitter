use super::{fit::Fit, indexing::IndexedEvent, statistics};

#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisResult {
    pub measured_bpm: f64,
    pub phase: PhaseStatistics,
    pub period: PeriodStatistics,
    pub exclusions: ExclusionCounts,
    pub rows: Vec<AnalysisRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisRow {
    pub sequence: u64,
    pub timestamp_ns: i128,
    pub tick_index: i64,
    pub step: i64,
    pub duplicate: bool,
    pub anomalous: bool,
    pub phase_error_ns: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExclusionCounts {
    pub non_clock_events: usize,
    pub duplicate_events: usize,
    pub anomalous_events: usize,
    pub inferred_missing_ticks: usize,
    pub excluded_from_regression: usize,
    pub excluded_from_phase_statistics: usize,
    pub excluded_from_period_statistics: usize,
}

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, PartialEq)]
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
) -> AnalysisResult {
    let rows: Vec<_> = indexed
        .iter()
        .map(|row| AnalysisRow {
            sequence: row.event.sequence,
            timestamp_ns: row.event.timestamp_ns,
            tick_index: row.tick_index,
            step: row.step,
            duplicate: row.duplicate,
            anomalous: row.anomalous,
            phase_error_ns: row.event.timestamp_ns as f64
                - (fit.intercept_ns + fit.period_ns * row.tick_index as f64),
        })
        .collect();
    let phase_errors: Vec<_> = rows
        .iter()
        .filter(|row| !row.duplicate && !row.anomalous)
        .map(|row| row.phase_error_ns)
        .collect();
    let intervals: Vec<_> = rows
        .windows(2)
        .filter(|pair| is_normal_one_tick_interval(pair))
        .map(|pair| (pair[1].timestamp_ns - pair[0].timestamp_ns) as f64)
        .collect();
    let period_errors: Vec<_> = intervals
        .iter()
        .map(|interval| interval - fit.period_ns)
        .collect();
    let phase = statistics::summarize(&phase_errors);
    let interval = statistics::summarize(&intervals);
    let period_error = statistics::summarize(&period_errors);
    let duplicate_events = rows.iter().filter(|row| row.duplicate).count();
    let anomalous_events = rows.iter().filter(|row| row.anomalous).count();
    let excluded_period = rows
        .windows(2)
        .filter(|pair| !is_normal_one_tick_interval(pair))
        .count();

    AnalysisResult {
        measured_bpm: 60_000_000_000.0 / (fit.period_ns * 24.0),
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
            inferred_missing_ticks: rows
                .iter()
                .filter(|row| row.step > 1)
                .map(|row| (row.step - 1) as usize)
                .sum(),
            excluded_from_regression: duplicate_events + anomalous_events,
            excluded_from_phase_statistics: duplicate_events + anomalous_events,
            excluded_from_period_statistics: excluded_period,
        },
        rows,
    }
}

fn is_normal_one_tick_interval(pair: &[AnalysisRow]) -> bool {
    let [previous, current] = pair else {
        return false;
    };

    !previous.duplicate
        && !previous.anomalous
        && !current.duplicate
        && !current.anomalous
        && current.tick_index == previous.tick_index + 1
}
