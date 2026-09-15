pub mod compare;
pub mod csv;
pub mod json;
pub mod text;

use crate::{CaptureFile, MidiEvent, TimestampMetadata};

/// Clock-event count below which percentile statistics are unreliable.
pub const SHORT_CAPTURE_CLOCK_THRESHOLD: usize = 240;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimingSummary {
    pub rate_hz: Option<f64>,
    pub quantum: Option<u32>,
    pub rate_changes: usize,
    pub quantum_changes: usize,
}

/// Derives the graph rate, quantum, and transition counts from a capture.
///
/// The initial timing comes from the first PipeWire event; every stored
/// transition is classified by comparing it against the previous timing.
pub fn timing_summary(capture: &CaptureFile) -> TimingSummary {
    let initial = capture
        .events
        .first()
        .and_then(|event| match &event.timestamp_metadata {
            TimestampMetadata::PipeWire(timestamp) => {
                Some((timestamp.rate_num, timestamp.rate_denom, timestamp.quantum))
            }
            // NOTE: a future timestamp variant must extend this match.
            TimestampMetadata::Alsa(_) => None,
        });

    let mut rate_changes = 0;
    let mut quantum_changes = 0;
    let mut previous = initial;
    for transition in &capture.transitions {
        if let Some((rate_num, rate_denom, quantum)) = previous {
            if (transition.rate_num, transition.rate_denom) != (rate_num, rate_denom) {
                rate_changes += 1;
            }
            if transition.quantum != quantum {
                quantum_changes += 1;
            }
        }
        previous = Some((
            transition.rate_num,
            transition.rate_denom,
            transition.quantum,
        ));
    }

    TimingSummary {
        rate_hz: initial
            .map(|(rate_num, rate_denom, _)| f64::from(rate_denom) / f64::from(rate_num)),
        quantum: initial.map(|(_, _, quantum)| quantum),
        rate_changes,
        quantum_changes,
    }
}

/// Counts transport events; clock events are analyzed separately.
pub fn transport_counts(capture: &CaptureFile) -> (usize, usize, usize) {
    let mut start = 0;
    let mut cont = 0;
    let mut stop = 0;
    for event in &capture.events {
        match event.event {
            MidiEvent::Start => start += 1,
            MidiEvent::Continue => cont += 1,
            MidiEvent::Stop => stop += 1,
            MidiEvent::Clock | MidiEvent::ActiveSensing => {}
        }
    }
    (start, cont, stop)
}

/// Capture duration in seconds, from the first to the last stored event.
pub fn duration_s(capture: &CaptureFile) -> f64 {
    let (Some(first), Some(last)) = (capture.events.first(), capture.events.last()) else {
        return 0.0;
    };
    (last.timestamp_ns - first.timestamp_ns) as f64 / 1_000_000_000.0
}

/// Human-readable warnings for conditions that affect interpretation.
pub fn warnings(capture: &CaptureFile, clock_events: usize) -> Vec<String> {
    let mut warnings = Vec::new();
    if !capture.transitions.is_empty() {
        warnings.push(format!(
            "Warning: graph timing changed during capture ({} transition{} recorded). \
             Results mix graph configurations.",
            capture.transitions.len(),
            if capture.transitions.len() == 1 {
                ""
            } else {
                "s"
            },
        ));
    }
    if clock_events < SHORT_CAPTURE_CLOCK_THRESHOLD {
        warnings.push(format!(
            "Warning: only {clock_events} MIDI Clock events were captured. \
             Use a longer capture for meaningful percentile statistics."
        ));
    }
    warnings
}
