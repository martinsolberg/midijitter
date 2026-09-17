use crate::{AppError, CapturedEvent};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventDisposition {
    Valid,
    StartupTransient,
    Duplicate,
    Anomalous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntervalDisposition {
    Normal,
    Missing { count: u32 },
    Anomalous,
}

#[derive(Debug, Clone)]
pub(crate) struct IndexedEvent<'a> {
    pub event: &'a CapturedEvent,
    pub tick_index: i64,
    pub step: i64,
    pub disposition: EventDisposition,
    pub interval_disposition: IntervalDisposition,
    pub missing_before: u32,
}

pub(crate) fn initial_period(events: &[&CapturedEvent]) -> Result<f64, AppError> {
    let mut intervals: Vec<f64> = events
        .windows(2)
        .filter_map(|pair| {
            let interval = pair[1].timestamp_ns - pair[0].timestamp_ns;
            (interval > 0).then_some(interval as f64)
        })
        .collect();
    if intervals.is_empty() {
        return Err(AppError::InvalidCapture(
            "clock analysis requires increasing Clock timestamps".to_owned(),
        ));
    }
    intervals.sort_by(f64::total_cmp);
    Ok(median(&intervals))
}

pub(crate) fn classify<'a>(
    events: &[&'a CapturedEvent],
    period_ns: f64,
    tolerance: f64,
) -> Vec<IndexedEvent<'a>> {
    let tolerance = tolerance.max(0.0);
    let mut rows = Vec::with_capacity(events.len());
    rows.push(IndexedEvent {
        event: events[0],
        tick_index: 0,
        step: 1,
        disposition: EventDisposition::Valid,
        interval_disposition: IntervalDisposition::Normal,
        missing_before: 0,
    });
    let mut previous_event = events[0];
    let mut last_included_event = events[0];
    let mut last_included_tick_index = 0;

    for event in &events[1..] {
        let raw_interval = (event.timestamp_ns - previous_event.timestamp_ns) as f64;
        let duplicate = raw_interval <= 0.0;
        let interval = (event.timestamp_ns - last_included_event.timestamp_ns) as f64;
        let ratio = interval / period_ns;
        let rounded = ratio.round() as i64;
        let is_missing_multiple = rounded >= 2 && (ratio - rounded as f64).abs() <= tolerance;
        let is_normal = rounded == 1 && (ratio - 1.0).abs() <= tolerance;
        let included = !duplicate && (is_missing_multiple || is_normal);
        let anomalous = !duplicate && !included;
        let step = if included { rounded } else { 0 };
        let tick_index = if included {
            last_included_tick_index + step
        } else if anomalous {
            last_included_tick_index + 1
        } else {
            last_included_tick_index
        };
        rows.push(IndexedEvent {
            event,
            tick_index,
            step,
            disposition: if duplicate {
                EventDisposition::Duplicate
            } else if included {
                EventDisposition::Valid
            } else {
                EventDisposition::Anomalous
            },
            interval_disposition: if duplicate {
                IntervalDisposition::Anomalous
            } else if is_missing_multiple {
                IntervalDisposition::Missing {
                    count: (rounded - 1) as u32,
                }
            } else if is_normal {
                IntervalDisposition::Normal
            } else {
                IntervalDisposition::Anomalous
            },
            missing_before: if is_missing_multiple {
                (rounded - 1) as u32
            } else {
                0
            },
        });
        if included {
            last_included_event = event;
            last_included_tick_index = tick_index;
        }
        previous_event = event;
    }
    rows
}

pub(crate) fn signature(
    rows: &[IndexedEvent<'_>],
) -> Vec<(i64, EventDisposition, IntervalDisposition, u32)> {
    rows.iter()
        .map(|row| {
            (
                row.step,
                row.disposition,
                row.interval_disposition,
                row.missing_before,
            )
        })
        .collect()
}

pub(crate) fn find_live_anchor(
    events: &[&CapturedEvent],
    period_ns: f64,
    tolerance: f64,
    startup_cadence: usize,
    settle_ns: i128,
) -> Option<usize> {
    if startup_cadence == 0 {
        return Some(0);
    }
    let start_ns = events.first()?.timestamp_ns;
    let boundary = start_ns.saturating_add(settle_ns.max(0));
    let tolerance = tolerance.max(0.0);
    events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.timestamp_ns >= boundary)
        .find_map(|(index, _)| {
            let end = index.checked_add(startup_cadence)?;
            let candidate = events.get(index..=end)?;
            let plausible = candidate.windows(2).all(|pair| {
                let interval = (pair[1].timestamp_ns - pair[0].timestamp_ns) as f64;
                interval > 0.0 && ((interval - period_ns) / period_ns).abs() <= tolerance
            });
            plausible.then_some(index)
        })
}

fn median(sorted: &[f64]) -> f64 {
    let middle = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    }
}
