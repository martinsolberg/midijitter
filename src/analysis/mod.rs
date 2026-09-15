mod fit;
mod indexing;
mod jitter;
mod statistics;

use crate::{AppError, CaptureFile, MidiEvent};

pub use jitter::{AnalysisResult, AnalysisRow, ExclusionCounts, PeriodStatistics, PhaseStatistics};

#[derive(Debug, Clone, Copy)]
pub struct AnalysisOptions {
    /// Maximum classification/refit passes; the analysis never exceeds ten.
    pub max_passes: usize,
    /// Allowed fractional error when an interval represents an integer number of ticks.
    pub integer_multiple_tolerance: f64,
}

impl Default for AnalysisOptions {
    fn default() -> Self {
        Self {
            max_passes: 10,
            integer_multiple_tolerance: 0.2,
        }
    }
}

pub fn analyze(
    capture: &CaptureFile,
    options: AnalysisOptions,
) -> Result<AnalysisResult, AppError> {
    let clock_events: Vec<_> = capture
        .events
        .iter()
        .filter(|event| event.event == MidiEvent::Clock)
        .collect();
    if clock_events.len() < 2 {
        return Err(AppError::InvalidCapture(
            "clock analysis requires at least two Clock events".to_owned(),
        ));
    }

    let initial_period = indexing::initial_period(&clock_events)?;
    let pass_limit = options.max_passes.min(10);
    let mut period = initial_period;
    let mut previous_signature = None;

    for _ in 0..pass_limit {
        let indexed = indexing::classify(&clock_events, period, options.integer_multiple_tolerance);
        let signature = indexing::signature(&indexed);
        let fit = fit::least_squares(&indexed)?;

        if previous_signature.as_ref() == Some(&signature) {
            if !indexed
                .iter()
                .skip(1)
                .any(|row| !row.duplicate && !row.anomalous && row.step == 1)
            {
                return Err(AppError::InvalidCapture(
                    "clock analysis requires a normal one-tick interval".to_owned(),
                ));
            }
            return Ok(jitter::build_result(&indexed, fit, capture.events.len()));
        }

        previous_signature = Some(signature);
        period = fit.period_ns;
    }

    Err(AppError::AnalysisDidNotConverge)
}
