use crate::{AppError, CapturedEvent};

#[derive(Debug, Clone)]
pub(crate) struct IndexedEvent<'a> {
    pub event: &'a CapturedEvent,
    pub tick_index: i64,
    pub step: i64,
    pub duplicate: bool,
    pub anomalous: bool,
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
        duplicate: false,
        anomalous: false,
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
        let step = if included { rounded } else { 0 };
        let tick_index = if included {
            last_included_tick_index + step
        } else {
            last_included_tick_index
        };
        rows.push(IndexedEvent {
            event,
            tick_index,
            step,
            duplicate,
            anomalous: !duplicate && !included,
        });
        if included {
            last_included_event = event;
            last_included_tick_index = tick_index;
        }
        previous_event = event;
    }
    rows
}

pub(crate) fn signature(rows: &[IndexedEvent<'_>]) -> Vec<(i64, bool, bool)> {
    rows.iter()
        .map(|row| (row.step, row.duplicate, row.anomalous))
        .collect()
}

fn median(sorted: &[f64]) -> f64 {
    let middle = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    }
}
