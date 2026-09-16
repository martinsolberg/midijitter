use std::collections::HashMap;
use std::path::Path;

use crate::{
    AnalysisResult, AppError, CaptureFile, CapturedEvent, EventDisposition, IntervalDisposition,
    TimestampMetadata, is_clean_period,
};

pub const CSV_HEADERS: [&str; 14] = [
    "event",
    "tick_index",
    "time_s",
    "interval_ms",
    "ideal_time_s",
    "phase_error_ms",
    "period_error_ms",
    "backend",
    "cycle_position",
    "event_offset",
    "event_position",
    "disposition",
    "interval_disposition",
    "missing_before",
];

fn seconds(timestamp_ns: i128) -> String {
    format!("{:.9}", timestamp_ns as f64 / 1_000_000_000.0)
}

fn milliseconds(value_ns: f64) -> String {
    format!("{:.6}", value_ns / 1_000_000.0)
}

/// Writes one row per analyzed clock event.
///
/// Backend-specific columns are left blank when the event was not captured
/// through PipeWire graph timing.
pub fn write_csv_report(
    capture: &CaptureFile,
    analysis: &AnalysisResult,
    path: &Path,
) -> Result<(), AppError> {
    let events: HashMap<u64, &CapturedEvent> = capture
        .events
        .iter()
        .map(|event| (event.sequence, event))
        .collect();

    let mut writer = csv::Writer::from_path(path)?;
    writer.write_record(CSV_HEADERS)?;

    let mut previous: Option<&crate::AnalysisRow> = None;
    for row in &analysis.rows {
        let event = events.get(&row.sequence);
        // NOTE: a future timestamp variant must extend this match with blank
        // backend-specific columns.
        let (backend, cycle_position, event_offset, event_position) = match event {
            Some(event) => match &event.timestamp_metadata {
                TimestampMetadata::PipeWire(timestamp) => (
                    "pipewire".to_owned(),
                    timestamp.cycle_position.to_string(),
                    timestamp.event_offset.to_string(),
                    timestamp.event_position.to_string(),
                ),
                TimestampMetadata::Alsa(_) => (
                    "alsa-raw".to_owned(),
                    String::new(),
                    String::new(),
                    String::new(),
                ),
            },
            None => (String::new(), String::new(), String::new(), String::new()),
        };

        let interval_ms = previous
            .map(|previous| milliseconds((row.timestamp_ns - previous.timestamp_ns) as f64))
            .unwrap_or_default();
        let period_error_ms = match previous {
            Some(previous) if is_clean_period(previous, row) => milliseconds(
                (row.timestamp_ns - previous.timestamp_ns) as f64 - analysis.fitted_period_ns,
            ),
            _ => String::new(),
        };
        let ideal_time_s = seconds(
            (analysis.intercept_ns + analysis.fitted_period_ns * row.tick_index as f64) as i128,
        );

        writer.write_record([
            "clock",
            &row.tick_index.to_string(),
            &seconds(row.timestamp_ns),
            &interval_ms,
            &ideal_time_s,
            &milliseconds(row.phase_error_ns),
            &period_error_ms,
            &backend,
            &cycle_position,
            &event_offset,
            &event_position,
            &event_disposition(row.disposition),
            &interval_disposition(row.interval_disposition),
            &row.missing_before.to_string(),
        ])?;

        previous = Some(row);
    }

    writer.flush().map_err(|error| AppError::FileWrite {
        path: path.display().to_string(),
        message: error.to_string(),
    })?;
    Ok(())
}

fn event_disposition(disposition: EventDisposition) -> String {
    format!("{disposition:?}")
}

fn interval_disposition(disposition: IntervalDisposition) -> String {
    match disposition {
        IntervalDisposition::Normal => "Normal".to_owned(),
        IntervalDisposition::Missing { count } => format!("Missing({count})"),
        IntervalDisposition::Anomalous => "Anomalous".to_owned(),
    }
}
