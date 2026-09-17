mod fit;
mod indexing;
mod jitter;
mod paired;
pub mod rolling;
mod statistics;

use crate::{AppError, CaptureFile, MidiEvent};

pub use indexing::{EventDisposition, IntervalDisposition};
pub use jitter::{
    AnalysisResult, AnalysisRow, AnomalyDetail, AnomalySummary, ExclusionCounts, PeriodStatistics,
    PhaseStatistics, StartupSummary, is_clean_period, is_clean_phase,
};
pub use paired::{
    LatencyStatistics, PairStatus, PairStatusCounts, PairedAnalysisResult, PairedEvent,
    PairingResult, analyze_paired,
};
pub use rolling::{RollingPoint, rolling_bpm};

#[derive(Debug, Clone, Copy)]
pub struct AnalysisOptions {
    /// Maximum classification/refit passes; the analysis never exceeds ten.
    pub max_passes: usize,
    /// Allowed fractional error when an interval represents an integer number of ticks.
    pub integer_multiple_tolerance: f64,
    /// Events before this offset from capture start are startup-transient:
    /// kept in rows, excluded from fit and statistics. Zero disables.
    pub settle_ns: i128,
    /// Startup cadence confirmation count. None selects the backend default;
    /// Some(0) disables startup filtering.
    pub startup_cadence: Option<usize>,
}

impl Default for AnalysisOptions {
    fn default() -> Self {
        Self {
            max_passes: 10,
            integer_multiple_tolerance: 0.2,
            settle_ns: 0,
            startup_cadence: None,
        }
    }
}

pub fn analyze(
    capture: &CaptureFile,
    options: AnalysisOptions,
) -> Result<AnalysisResult, AppError> {
    if !capture.transitions.is_empty() {
        return Err(AppError::GraphRateTransitionUnsupported);
    }

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
    let startup_cadence = options
        .startup_cadence
        .unwrap_or_else(|| if capture.backend == "pipewire" { 8 } else { 0 });
    let anchor = indexing::find_live_anchor(
        &clock_events,
        initial_period,
        options.integer_multiple_tolerance,
        startup_cadence,
        options.settle_ns,
    )
    .ok_or_else(|| {
        AppError::InvalidCapture(
            "clock analysis requires a complete startup cadence confirmation".to_owned(),
        )
    })?;
    let pass_limit = options.max_passes.min(10);
    let mut period = initial_period;
    let mut previous_signature = None;

    for _ in 0..pass_limit {
        let mut indexed = indexing::classify(
            &clock_events[anchor..],
            period,
            options.integer_multiple_tolerance,
        );
        let mut transient = clock_events[..anchor]
            .iter()
            .map(|event| indexing::IndexedEvent {
                event,
                tick_index: 0,
                step: 0,
                disposition: indexing::EventDisposition::StartupTransient,
                interval_disposition: indexing::IntervalDisposition::Normal,
                missing_before: 0,
            })
            .collect::<Vec<_>>();
        transient.append(&mut indexed);
        let indexed = transient;
        let signature = indexing::signature(&indexed);
        let fit = fit::least_squares(&indexed)?;

        if previous_signature.as_ref() == Some(&signature) {
            if !indexed
                .iter()
                .any(|row| row.disposition == indexing::EventDisposition::Valid && row.step == 1)
            {
                return Err(AppError::InvalidCapture(
                    "clock analysis requires a normal one-tick interval".to_owned(),
                ));
            }
            return jitter::build_result(
                &indexed,
                fit,
                capture.events.len(),
                startup_cadence,
                initial_period,
            );
        }

        previous_signature = Some(signature);
        period = fit.period_ns;
    }

    Err(AppError::AnalysisDidNotConverge)
}
