use crate::{AnalysisResult, AppError};

use super::jitter::AnalysisRow;

/// One rolling-tempo sample: the mean tempo over the window ending at a tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RollingPoint {
    pub tick_index: i64,
    pub time_s: f64,
    pub rolling_bpm: f64,
}

/// Rolling BPM over a sliding tick window.
///
/// Each point estimates tempo from the slope across the included (usable)
/// rows in the window ending at that tick, so inferred missing ticks do not
/// bias the estimate the way a plain interval mean would.
pub fn rolling_bpm(
    analysis: &AnalysisResult,
    window_ticks: usize,
) -> Result<Vec<RollingPoint>, AppError> {
    if window_ticks < 1 {
        return Err(AppError::InvalidCapture(
            "rolling window must cover at least one tick".to_owned(),
        ));
    }
    let usable: Vec<&AnalysisRow> = analysis
        .rows
        .iter()
        .filter(|row| !row.duplicate && !row.anomalous)
        .collect();
    if usable.len() < window_ticks + 1 {
        return Err(AppError::InvalidCapture(
            "capture is shorter than the rolling window".to_owned(),
        ));
    }

    let mut points = Vec::new();
    for end in 0..usable.len() {
        let start_tick = usable[end].tick_index - window_ticks as i64;
        let start = usable[..=end]
            .iter()
            .rposition(|row| row.tick_index <= start_tick)
            .unwrap_or(0);
        let (first, last) = (usable[start], usable[end]);
        if last.tick_index <= first.tick_index {
            continue;
        }
        let span_ns = (last.timestamp_ns - first.timestamp_ns) as f64;
        let span_ticks = (last.tick_index - first.tick_index) as f64;
        if span_ns <= 0.0 {
            continue;
        }
        points.push(RollingPoint {
            tick_index: last.tick_index,
            time_s: last.timestamp_ns as f64 / 1_000_000_000.0,
            rolling_bpm: 60_000_000_000.0 / ((span_ns / span_ticks) * 24.0),
        });
    }
    if points.is_empty() {
        return Err(AppError::InvalidCapture(
            "no rolling tempo could be estimated".to_owned(),
        ));
    }
    Ok(points)
}

#[cfg(test)]
mod tests {
    use crate::simulate::{SimulateConfig, generate};
    use crate::{AnalysisOptions, analyze};

    use super::rolling_bpm;

    fn analyzed(config: SimulateConfig) -> crate::AnalysisResult {
        let capture = generate(config).unwrap();
        analyze(&capture, AnalysisOptions::default()).unwrap()
    }

    fn config() -> SimulateConfig {
        SimulateConfig {
            bpm: 120.0,
            duration_s: 60,
            jitter_std_ns: 0.0,
            periodic_jitter_ns: 0.0,
            periodic_hz: 1.0,
            missing_rate: 0.0,
            duplicate_rate: 0.0,
            drift_per_second: 0.0,
            seed: 7,
        }
    }

    #[test]
    fn perfect_tempo_is_flat() {
        let analysis = analyzed(config());
        let points = rolling_bpm(&analysis, 48).unwrap();
        assert!(points.len() > 100);
        for point in &points {
            assert!(
                (point.rolling_bpm - 120.0).abs() < 0.01,
                "unexpected rolling BPM {}",
                point.rolling_bpm
            );
        }
    }

    #[test]
    fn drift_shows_as_a_tempo_trend() {
        let mut drifted = config();
        // Period grows 200 ppm per second, so tempo falls over time while
        // staying well inside the tick classifier's tolerance.
        drifted.drift_per_second = 0.000_2;
        let analysis = analyzed(drifted);
        let points = rolling_bpm(&analysis, 48).unwrap();
        let first = points.first().unwrap().rolling_bpm;
        let last = points.last().unwrap().rolling_bpm;
        assert!(
            last < first - 0.5,
            "expected falling tempo, got first={first} last={last}"
        );
    }

    #[test]
    fn short_captures_are_rejected() {
        let analysis = analyzed(config());
        assert!(rolling_bpm(&analysis, 1_000_000).is_err());
        assert!(rolling_bpm(&analysis, 0).is_err());
    }
}
