use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::{
    AppError, CaptureFile, CapturedEvent, CompletionStatus, EventDisposition, MidiEvent,
    PairedCapture,
};

use super::{AnalysisOptions, AnalysisResult, analyze};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PairStatus {
    Valid,
    MissingReference,
    MissingReturned,
    ReferenceAnomalous,
    ReturnedAnomalous,
    BothAnomalous,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PairedEvent {
    pub tick_index: i64,
    pub reference: Option<crate::AnalysisRow>,
    pub returned: Option<crate::AnalysisRow>,
    pub status: PairStatus,
    pub latency_ns: Option<i128>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PairingResult {
    pub rows: Vec<PairedEvent>,
    pub lag_ticks: i64,
    pub anchored: bool,
    pub candidate_lags: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LatencyStatistics {
    pub count: usize,
    pub mean_ns: i128,
    pub median_ns: i128,
    pub p95_ns: i128,
    pub minimum_ns: i128,
    pub maximum_ns: i128,
    pub standard_deviation_ns: i128,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PairedAnalysisResult {
    pub reference: AnalysisResult,
    pub returned: AnalysisResult,
    pub pairing: PairingResult,
    pub latency: LatencyStatistics,
}

pub fn analyze_paired(
    capture: &PairedCapture,
    options: AnalysisOptions,
) -> Result<PairedAnalysisResult, AppError> {
    capture.validate()?;
    if capture.completion.status != CompletionStatus::Complete {
        return Err(AppError::InvalidCapture(
            "paired analysis requires a complete capture".to_owned(),
        ));
    }
    if capture
        .transitions
        .iter()
        .any(|transition| !matches!(transition.kind, crate::GraphTransitionKind::Initial))
    {
        return Err(AppError::GraphRateTransitionUnsupported);
    }

    let reference_capture = single_capture(capture, &capture.reference_events, "reference");
    let returned_capture = single_capture(capture, &capture.returned_events, "returned");
    let reference = analyze(&reference_capture, options)?;
    let returned = analyze(&returned_capture, options)?;
    let (lag_ticks, anchored, candidates) = select_lag(capture, &reference, &returned)?;
    let rows = pair_rows(&reference, &returned, lag_ticks);
    let values: Vec<_> = rows
        .iter()
        .filter_map(|row| {
            (row.status == PairStatus::Valid)
                .then_some(row.latency_ns)
                .flatten()
        })
        .collect();
    if values.is_empty() {
        return Err(AppError::InvalidCapture(
            "paired analysis requires at least one valid pair".to_owned(),
        ));
    }
    Ok(PairedAnalysisResult {
        reference,
        returned,
        pairing: PairingResult {
            rows,
            lag_ticks,
            anchored,
            candidate_lags: candidates,
        },
        latency: summarize_latency(&values),
    })
}

fn single_capture(capture: &PairedCapture, events: &[CapturedEvent], name: &str) -> CaptureFile {
    CaptureFile {
        format_version: 1,
        backend: capture.backend.clone(),
        source: if name == "reference" {
            capture.reference.clone()
        } else {
            capture.returned.clone()
        },
        timestamp_method: capture.timestamp_method.clone(),
        ppqn: capture.ppqn,
        application_version: capture.application_version.clone(),
        environment: capture.environment.clone(),
        transitions: vec![],
        events: events.to_vec(),
    }
}

fn select_lag(
    capture: &PairedCapture,
    reference: &AnalysisResult,
    returned: &AnalysisResult,
) -> Result<(i64, bool, Vec<i64>), AppError> {
    if let Some(lag) = anchored_lag(capture, reference, returned) {
        return Ok((lag, true, vec![lag]));
    }
    let ref_valid: Vec<_> = reference
        .rows
        .iter()
        .filter(|row| row.disposition == EventDisposition::Valid)
        .collect();
    let ret_valid: Vec<_> = returned
        .rows
        .iter()
        .filter(|row| row.disposition == EventDisposition::Valid)
        .collect();
    let mut candidates = BTreeSet::new();
    for left in &ref_valid {
        for right in &ret_valid {
            candidates.insert(right.tick_index - left.tick_index);
        }
    }
    let candidates: Vec<_> = candidates.into_iter().collect();
    let mut scored = Vec::new();
    for lag in &candidates {
        let values: Vec<_> = ref_valid
            .iter()
            .filter_map(|left| {
                ret_valid
                    .iter()
                    .find(|right| right.tick_index == left.tick_index + lag)
                    .map(|right| right.timestamp_ns - left.timestamp_ns)
            })
            .collect();
        if values.len() >= 2 {
            let median = median_i128(&values);
            if median > 0 {
                scored.push((*lag, median, mad(&values, median)));
            }
        }
    }
    if scored.is_empty() {
        return Err(AppError::InvalidCapture(
            "paired streams have no correspondence".to_owned(),
        ));
    }
    let period = reference.fitted_period_ns.max(1.0) as i128;
    let initial_delta =
        ret_valid.first().unwrap().timestamp_ns - ref_valid.first().unwrap().timestamp_ns;
    if initial_delta.abs() >= period {
        return Err(AppError::InvalidCapture(
            "paired stream alignment is ambiguous".to_owned(),
        ));
    }
    scored.sort_by(|left, right| {
        left.2
            .cmp(&right.2)
            .then(left.1.cmp(&right.1))
            .then(left.0.cmp(&right.0))
    });
    if scored.len() > 1 && scored[0].2 == scored[1].2 && scored[0].1 == scored[1].1 {
        return Err(AppError::InvalidCapture(
            "paired stream alignment is ambiguous".to_owned(),
        ));
    }
    Ok((scored[0].0, false, candidates))
}

fn anchored_lag(
    capture: &PairedCapture,
    reference: &AnalysisResult,
    returned: &AnalysisResult,
) -> Option<i64> {
    let mut lags = Vec::new();
    for kind in [MidiEvent::Start, MidiEvent::Continue, MidiEvent::Stop] {
        let ref_event = capture
            .reference_events
            .iter()
            .find(|event| event.event == kind);
        let ret_event = capture
            .returned_events
            .iter()
            .find(|event| event.event == kind);
        if let (Some(left), Some(right)) = (ref_event, ret_event) {
            let left = nearest_valid_tick(reference, left.timestamp_ns)?;
            let right = nearest_valid_tick(returned, right.timestamp_ns)?;
            lags.push(right - left);
        }
    }
    if lags.is_empty() || !lags.windows(2).all(|pair| pair[0] == pair[1]) {
        None
    } else {
        Some(lags[0])
    }
}

fn nearest_valid_tick(result: &AnalysisResult, timestamp_ns: i128) -> Option<i64> {
    result
        .rows
        .iter()
        .filter(|row| row.disposition == EventDisposition::Valid)
        .min_by_key(|row| (row.timestamp_ns - timestamp_ns).abs())
        .map(|row| row.tick_index)
}

fn pair_rows(reference: &AnalysisResult, returned: &AnalysisResult, lag: i64) -> Vec<PairedEvent> {
    let mut refs: BTreeMap<i64, Vec<crate::AnalysisRow>> = BTreeMap::new();
    let mut rets: BTreeMap<i64, Vec<crate::AnalysisRow>> = BTreeMap::new();
    for row in &reference.rows {
        refs.entry(row.tick_index).or_default().push(row.clone());
    }
    for row in &returned.rows {
        rets.entry(row.tick_index).or_default().push(row.clone());
    }
    let mut ticks = BTreeSet::new();
    refs.keys().for_each(|tick| {
        ticks.insert(*tick);
    });
    rets.keys().for_each(|tick| {
        ticks.insert(*tick - lag);
    });
    let mut rows = Vec::new();
    for tick in ticks {
        let reference = refs.get(&tick).and_then(|rows| rows.first()).cloned();
        let returned = rets
            .get(&(tick + lag))
            .and_then(|rows| rows.first())
            .cloned();
        let status = match (reference.as_ref(), returned.as_ref()) {
            (None, _) => PairStatus::MissingReference,
            (_, None) => PairStatus::MissingReturned,
            (Some(left), Some(right)) => match (
                left.disposition == EventDisposition::Valid,
                right.disposition == EventDisposition::Valid,
            ) {
                (true, true) => PairStatus::Valid,
                (false, true) => PairStatus::ReferenceAnomalous,
                (true, false) => PairStatus::ReturnedAnomalous,
                (false, false) => PairStatus::BothAnomalous,
            },
        };
        let latency_ns = match (status, reference.as_ref(), returned.as_ref()) {
            (PairStatus::Valid, Some(reference), Some(returned)) => {
                Some(returned.timestamp_ns - reference.timestamp_ns)
            }
            _ => None,
        };
        rows.push(PairedEvent {
            tick_index: tick,
            reference,
            returned,
            status,
            latency_ns,
        });
        if let Some(extras) = refs.get(&tick).map(|rows| rows.iter().skip(1)) {
            for extra in extras {
                rows.push(PairedEvent {
                    tick_index: tick,
                    reference: Some(extra.clone()),
                    returned: None,
                    status: PairStatus::ReferenceAnomalous,
                    latency_ns: None,
                });
            }
        }
        if let Some(extras) = rets.get(&(tick + lag)).map(|rows| rows.iter().skip(1)) {
            for extra in extras {
                rows.push(PairedEvent {
                    tick_index: tick,
                    reference: None,
                    returned: Some(extra.clone()),
                    status: PairStatus::ReturnedAnomalous,
                    latency_ns: None,
                });
            }
        }
    }
    rows
}

fn median_i128(values: &[i128]) -> i128 {
    let mut values = values.to_vec();
    values.sort_unstable();
    values[values.len() / 2]
}
fn mad(values: &[i128], median: i128) -> i128 {
    let deviations: Vec<_> = values.iter().map(|value| (*value - median).abs()).collect();
    median_i128(&deviations)
}

fn summarize_latency(values: &[i128]) -> LatencyStatistics {
    let median = median_i128(values);
    let mean = values.iter().sum::<i128>() / values.len() as i128;
    let variance = values
        .iter()
        .map(|value| (*value - mean).pow(2) as f64)
        .sum::<f64>()
        / values.len() as f64;
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let percentile = |p: usize| sorted[((sorted.len() - 1) * p + 50) / 100];
    LatencyStatistics {
        count: values.len(),
        mean_ns: mean,
        median_ns: median,
        p95_ns: percentile(95),
        minimum_ns: sorted[0],
        maximum_ns: *sorted.last().unwrap(),
        standard_deviation_ns: variance.sqrt().round() as i128,
    }
}
