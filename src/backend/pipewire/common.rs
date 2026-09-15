use super::timing::normalize_pipewire_event_timestamps;
use crate::backend::pipewire::timing::pipewire_event_position;
use crate::backend::{CaptureRequest, CaptureTermination};
use crate::capture::MidiParser;
use crate::{
    AppError, CaptureFile, CapturedEvent, EnvironmentMetadata, GraphTransition, PipeWireTimestamp,
    SourceMetadata, TimestampMetadata,
};
use pipewire as pw;
use pw::spa;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

const MAX_MIDI_BYTES_PER_SECOND: u64 = 3_125;
const EVENT_SAFETY_MARGIN: u64 = 128;
const TRANSITION_CAPACITY: usize = 16;
pub(super) const STOP_NONE: u8 = 0;
pub(super) const STOP_COMPLETE: u8 = 1;
pub(super) const STOP_OVERFLOW: u8 = 2;
pub(super) const STOP_UNSUPPORTED_FORMAT: u8 = 3;
pub(super) const STOP_SOURCE_GONE: u8 = 4;
pub(super) const STOP_NEGOTIATION_FAILURE: u8 = 5;
pub(super) const STOP_TIMESTAMP_ERROR: u8 = 6;
pub(super) const STOP_STREAM_ERROR: u8 = 7;

/// Capture data owned exclusively by the realtime callback side while the
/// graph runs. Never shared across threads: the main-loop thread only touches
/// [`SharedControl`]. Reclaimed by the main thread after callbacks stop.
pub(super) struct CaptureData {
    parser: MidiParser,
    events: Vec<CapturedEvent>,
    clock_events: u64,
    transitions: Vec<GraphTransition>,
    last_timing: Option<(u32, u32, u32)>,
    first_cycle_position: Option<i64>,
    termination: CaptureTermination,
}

/// Thread-safe control block shared between the realtime process callback and
/// the main-loop thread. Atomics only during the run; the error slot is
/// written on failure paths and read after callbacks have stopped, so the
/// mutex is never contended while capturing.
pub(super) struct SharedControl {
    stop: AtomicU8,
    event_count: AtomicU64,
    streamed: AtomicBool,
    error: Mutex<Option<String>>,
}

impl CaptureData {
    pub(super) fn new(termination: CaptureTermination, event_capacity: usize) -> Self {
        Self {
            parser: MidiParser::default(),
            events: Vec::with_capacity(event_capacity),
            clock_events: 0,
            transitions: Vec::with_capacity(TRANSITION_CAPACITY),
            last_timing: None,
            first_cycle_position: None,
            termination,
        }
    }

    pub(super) fn observe_timing(
        &mut self,
        control: &SharedControl,
        rate_num: u32,
        rate_denom: u32,
        quantum: u32,
    ) {
        if rate_num == 0 || rate_denom == 0 || quantum == 0 {
            control.request_stop(STOP_NEGOTIATION_FAILURE);
            return;
        }
        let timing = (rate_num, rate_denom, quantum);
        if self.last_timing.is_some_and(|previous| previous != timing) {
            if self.transitions.len() == self.transitions.capacity() {
                control.request_stop(STOP_OVERFLOW);
                return;
            }
            self.transitions.push(GraphTransition {
                event_sequence: self.events.len() as u64,
                rate_num,
                rate_denom,
                quantum,
            });
        }
        self.last_timing = Some(timing);
    }

    pub(super) fn note_cycle_position(&mut self, cycle_position: i64) {
        self.first_cycle_position.get_or_insert(cycle_position);
    }

    pub(super) fn termination_reached(&self, cycle_position: i64) -> bool {
        match self.termination {
            CaptureTermination::Ticks(target) => self.clock_events >= target,
            CaptureTermination::DurationSeconds(seconds) => {
                self.first_cycle_position.is_some_and(|first| {
                    let Some((rate_num, rate_denom, _)) = self.last_timing else {
                        return false;
                    };
                    let elapsed = i128::from(cycle_position) - i128::from(first);
                    elapsed >= i128::from(seconds) * i128::from(rate_denom) / i128::from(rate_num)
                })
            }
        }
    }
}

impl SharedControl {
    pub(super) fn new() -> Self {
        Self {
            stop: AtomicU8::new(STOP_NONE),
            event_count: AtomicU64::new(0),
            streamed: AtomicBool::new(false),
            error: Mutex::new(None),
        }
    }

    pub(super) fn request_stop(&self, reason: u8) {
        let _ = self
            .stop
            .compare_exchange(STOP_NONE, reason, Ordering::Release, Ordering::Relaxed);
    }

    pub(super) fn stop_reason(&self) -> u8 {
        self.stop.load(Ordering::Acquire)
    }

    pub(super) fn mark_streamed(&self) {
        self.streamed.store(true, Ordering::Release);
    }

    pub(super) fn is_streamed(&self) -> bool {
        self.streamed.load(Ordering::Acquire)
    }

    pub(super) fn set_event_count(&self, count: usize) {
        self.event_count.store(count as u64, Ordering::Release);
    }

    pub(super) fn event_count(&self) -> u64 {
        self.event_count.load(Ordering::Acquire)
    }

    pub(super) fn set_error(&self, detail: String) {
        *self.error.lock().expect("control error slot") = Some(detail);
    }

    pub(super) fn take_error(&self) -> Option<String> {
        self.error.lock().expect("control error slot").take()
    }
}

/// Whether the manual-connect "link active" message should print now: once,
/// and only after the link actually streams.
pub(super) fn link_announcement_needed(
    manual_connect: bool,
    streamed: bool,
    announced: bool,
) -> bool {
    manual_connect && streamed && !announced
}

pub(super) fn event_capacity(termination: &CaptureTermination) -> Result<usize, AppError> {
    let capacity = match termination {
        CaptureTermination::DurationSeconds(seconds) => seconds
            .checked_mul(MAX_MIDI_BYTES_PER_SECOND)
            .and_then(|events| events.checked_add(EVENT_SAFETY_MARGIN)),
        CaptureTermination::Ticks(ticks) => ticks.checked_add(EVENT_SAFETY_MARGIN),
    }
    .ok_or_else(|| AppError::InvalidCapture("capture termination is too large".to_owned()))?;
    usize::try_from(capacity).map_err(|_| {
        AppError::InvalidCapture("capture capacity does not fit this platform".to_owned())
    })
}

/// Settles an interrupted-or-stopped capture: maps the stop reason to the
/// spec-listed error or assembles the versioned capture file.
pub(super) fn finish_capture(
    data: &mut CaptureData,
    control: &SharedControl,
    interrupted: bool,
    request: &CaptureRequest,
) -> Result<CaptureFile, AppError> {
    // An interrupt ends the bounded capture early; valid captured events are
    // retained and reported as a partial capture.
    if control.stop_reason() == STOP_NONE && interrupted {
        if data.events.is_empty() {
            return Err(AppError::NoClockEvents);
        }
        control.request_stop(STOP_COMPLETE);
    }
    match control.stop_reason() {
        STOP_COMPLETE => {}
        STOP_OVERFLOW => return Err(AppError::CaptureOverflow),
        STOP_UNSUPPORTED_FORMAT => return Err(AppError::PipeWireUnsupportedControlFormat),
        STOP_SOURCE_GONE => return Err(AppError::PipeWireSourceDisappeared),
        STOP_NEGOTIATION_FAILURE => {
            return Err(AppError::PipeWireNegotiationFailed {
                detail: "PipeWire reported an invalid graph rate or quantum".to_owned(),
            });
        }
        STOP_TIMESTAMP_ERROR => return Err(AppError::TimestampArithmeticOverflow),
        STOP_STREAM_ERROR => {
            let detail = control
                .take_error()
                .unwrap_or_else(|| "PipeWire capture entered the error state".to_owned());
            return Err(AppError::PipeWireNegotiationFailed { detail });
        }
        _ => {
            return Err(AppError::PipeWireNegotiationFailed {
                detail: "PipeWire capture stopped before a bounded termination condition"
                    .to_owned(),
            });
        }
    }
    if data.events.is_empty() {
        return Err(AppError::NoClockEvents);
    }
    normalize_pipewire_event_timestamps(&mut data.events)?;
    Ok(CaptureFile {
        format_version: crate::capture::CURRENT_FORMAT_VERSION,
        backend: "pipewire".to_owned(),
        source: SourceMetadata {
            identity: request.source.stable_identity(),
            display_name: request.source.display_name.clone(),
        },
        timestamp_method: "PipeWire graph position + event offset".to_owned(),
        ppqn: 24,
        application_version: env!("CARGO_PKG_VERSION").to_owned(),
        environment: EnvironmentMetadata {
            operating_system: std::env::consts::OS.to_owned(),
            pipewire_version: None,
        },
        transitions: std::mem::take(&mut data.transitions),
        events: std::mem::take(&mut data.events),
    })
}

/// Casts a raw data buffer to a SPA control sequence if its header looks valid.
///
/// PipeWire can deliver MIDI either as `SPA_META_Control` metadata or as a
/// `SPA_TYPE_Sequence` pod inside the buffer's data memory. This handles the
/// latter case for manually-linked sinks.
pub(super) fn spa_sequence_from_bytes(bytes: &[u8]) -> Option<&spa::sys::spa_pod_sequence> {
    if bytes.len() < std::mem::size_of::<spa::sys::spa_pod>() {
        return None;
    }
    // SAFETY: the pointer is valid for the lifetime of the borrowed slice and
    // we only dereference the fixed-size pod header here.
    let pod = unsafe { &*bytes.as_ptr().cast::<spa::sys::spa_pod>() };
    if pod.type_ != spa::sys::SPA_TYPE_Sequence {
        return None;
    }
    let needed = std::mem::size_of::<spa::sys::spa_pod>() + pod.size as usize;
    if bytes.len() < needed {
        return None;
    }
    Some(unsafe { &*bytes.as_ptr().cast::<spa::sys::spa_pod_sequence>() })
}

/// Reads the SPA sequence in-place; PipeWire owns this memory until the buffer is returned.
pub(super) unsafe fn record_spa_sequence(
    sequence: &spa::sys::spa_pod_sequence,
    cycle_position: i64,
    rate_num: u32,
    rate_denom: u32,
    quantum: u32,
    data: &mut CaptureData,
    control: &SharedControl,
) -> Result<(), ()> {
    // SAFETY: `sequence` points to a live PipeWire buffer owned by the stream
    // until the buffer is returned, and all pointer arithmetic below stays
    // within the sequence pod bounds checked against `pod.size`.
    unsafe {
        let sequence_start = sequence as *const _ as *const u8;
        let pod_size = sequence.pod.size as usize;
        if pod_size < std::mem::size_of::<spa::sys::spa_pod_sequence_body>() {
            return Err(());
        }
        let sequence_end = sequence_start.add(std::mem::size_of::<spa::sys::spa_pod>() + pod_size);
        let mut cursor = sequence_start.add(std::mem::size_of::<spa::sys::spa_pod_sequence>());
        while cursor < sequence_end {
            if sequence_end.offset_from(cursor)
                < std::mem::size_of::<spa::sys::spa_pod_control>() as isize
            {
                return Err(());
            }
            let event = cursor.cast::<spa::sys::spa_pod_control>();
            let value_size = (*event).value.size as usize;
            let payload_start = cursor.add(std::mem::size_of::<spa::sys::spa_pod_control>());
            let payload_end = payload_start.add(value_size);
            if payload_end > sequence_end {
                return Err(());
            }
            if (*event).type_ != spa::sys::SPA_CONTROL_Midi {
                cursor = payload_end.add((8 - (value_size % 8)) % 8);
                continue;
            }
            let bytes = std::slice::from_raw_parts(payload_start, value_size);
            let before = data.events.len();
            if let Err(error) = record_control_bytes(
                &mut data.parser,
                &mut data.events,
                cycle_position,
                (*event).offset,
                rate_num,
                rate_denom,
                quantum,
                bytes,
            ) {
                match error {
                    AppError::CaptureOverflow => control.request_stop(STOP_OVERFLOW),
                    _ => control.request_stop(STOP_TIMESTAMP_ERROR),
                }
                return Ok(());
            }
            data.clock_events += data.events[before..]
                .iter()
                .filter(|event| matches!(event.event, crate::MidiEvent::Clock))
                .count() as u64;
            cursor = payload_end.add((8 - (value_size % 8)) % 8);
        }
        Ok(())
    }
}

/// Shared per-cycle transport tail: stamps the first position, tracks graph
/// timing, records the sequence, and checks termination. Used verbatim by
/// both the stream and filter transports so their semantics cannot diverge.
pub(super) fn record_cycle(
    data: &mut CaptureData,
    control: &SharedControl,
    sequence: &spa::sys::spa_pod_sequence,
    cycle_position: i64,
    rate_num: u32,
    rate_denom: u32,
    quantum: u32,
) {
    data.note_cycle_position(cycle_position);
    data.observe_timing(control, rate_num, rate_denom, quantum);
    if control.stop_reason() != STOP_NONE {
        return;
    }
    if unsafe {
        record_spa_sequence(
            sequence,
            cycle_position,
            rate_num,
            rate_denom,
            quantum,
            data,
            control,
        )
    }
    .is_err()
    {
        control.request_stop(STOP_UNSUPPORTED_FORMAT);
        return;
    }
    if data.termination_reached(cycle_position) {
        control.request_stop(STOP_COMPLETE);
    }
    control.set_event_count(data.events.len());
}

pub(super) fn pipewire_unavailable(error: pw::Error) -> AppError {
    AppError::PipeWireUnavailable {
        detail: error.to_string(),
    }
}

/// Appends recognized realtime MIDI bytes from one SPA control event.
///
/// `events` must be preallocated by the caller. The capacity check ensures this
/// function never asks `Vec` to grow while it is called from PipeWire's process
/// callback.
#[allow(clippy::too_many_arguments)]
pub(super) fn record_control_bytes(
    parser: &mut MidiParser,
    events: &mut Vec<CapturedEvent>,
    cycle_position: i64,
    event_offset: u32,
    rate_num: u32,
    rate_denom: u32,
    quantum: u32,
    bytes: &[u8],
) -> Result<(), AppError> {
    let event_position = pipewire_event_position(cycle_position, event_offset)?;

    for &byte in bytes {
        let Some(event) = parser.push(byte) else {
            continue;
        };
        if events.len() == events.capacity() {
            return Err(AppError::CaptureOverflow);
        }

        events.push(CapturedEvent {
            sequence: events.len() as u64,
            timestamp_ns: 0,
            event,
            timestamp_metadata: TimestampMetadata::PipeWire(PipeWireTimestamp {
                cycle_position,
                event_offset,
                event_position,
                rate_num,
                rate_denom,
                quantum,
            }),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::backend::pipewire::common::{
        STOP_COMPLETE, STOP_NONE, SharedControl, link_announcement_needed, record_control_bytes,
    };
    use crate::capture::MidiParser;
    use crate::{CapturedEvent, TimestampMetadata};

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn shared_control_is_send_and_sync() {
        assert_send_sync::<SharedControl>();
    }

    #[test]
    fn shared_control_stop_roundtrip() {
        let control = SharedControl::new();
        assert_eq!(control.stop_reason(), STOP_NONE);
        control.request_stop(STOP_COMPLETE);
        assert_eq!(control.stop_reason(), STOP_COMPLETE);
        // First reason wins.
        control.request_stop(STOP_NONE);
        assert_eq!(control.stop_reason(), STOP_COMPLETE);
    }

    #[test]
    fn shared_control_tracks_streamed_and_event_count() {
        let control = SharedControl::new();
        assert!(!control.is_streamed());
        control.mark_streamed();
        assert!(control.is_streamed());
        assert_eq!(control.event_count(), 0);
        control.set_event_count(65);
        assert_eq!(control.event_count(), 65);
    }

    #[test]
    fn shared_control_survives_concurrent_access() {
        use std::sync::{Arc, Barrier};

        let control = Arc::new(SharedControl::new());
        let barrier = Arc::new(Barrier::new(9));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let control = Arc::clone(&control);
            let barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..10_000 {
                    control.request_stop(STOP_COMPLETE);
                    let _ = control.stop_reason();
                    control.mark_streamed();
                    let _ = control.is_streamed();
                    control.set_event_count(1);
                    let _ = control.event_count();
                }
            }));
        }
        barrier.wait();
        for handle in handles {
            handle.join().expect("worker must not panic");
        }
        assert_eq!(control.stop_reason(), STOP_COMPLETE);
        assert!(control.is_streamed());
    }

    #[test]
    fn link_announcement_fires_once_on_manual_streamed() {
        assert!(link_announcement_needed(true, true, false));
        assert!(!link_announcement_needed(true, true, true));
        assert!(!link_announcement_needed(true, false, false));
        assert!(!link_announcement_needed(false, true, false));
    }

    #[test]
    fn records_each_realtime_byte_at_its_graph_cycle_offset() {
        let mut parser = MidiParser::default();
        let mut events: Vec<CapturedEvent> = Vec::with_capacity(2);

        record_control_bytes(
            &mut parser,
            &mut events,
            4_096,
            17,
            1,
            48_000,
            256,
            &[0xf8, 0xfa],
        )
        .unwrap();

        assert_eq!(events.len(), 2);
        for (sequence, event) in events.iter().enumerate() {
            assert_eq!(event.sequence, sequence as u64);
            assert_eq!(event.timestamp_ns, 0);
            let TimestampMetadata::PipeWire(timestamp) = &event.timestamp_metadata else {
                panic!("PipeWire metadata must be preserved");
            };
            assert_eq!(timestamp.cycle_position, 4_096);
            assert_eq!(timestamp.event_offset, 17);
            assert_eq!(timestamp.event_position, 4_113);
            assert_eq!(timestamp.rate_num, 1);
            assert_eq!(timestamp.rate_denom, 48_000);
            assert_eq!(timestamp.quantum, 256);
        }
    }

    #[test]
    fn events_in_one_cycle_keep_distinct_offsets_and_positions() {
        let mut parser = MidiParser::default();
        let mut events: Vec<CapturedEvent> = Vec::with_capacity(2);

        record_control_bytes(
            &mut parser,
            &mut events,
            48_000,
            317,
            1,
            48_000,
            1024,
            &[0xf8],
        )
        .unwrap();
        record_control_bytes(
            &mut parser,
            &mut events,
            48_000,
            851,
            1,
            48_000,
            1024,
            &[0xf8],
        )
        .unwrap();

        assert_eq!(events.len(), 2);
        let TimestampMetadata::PipeWire(first) = &events[0].timestamp_metadata else {
            panic!("PipeWire metadata must be preserved");
        };
        let TimestampMetadata::PipeWire(second) = &events[1].timestamp_metadata else {
            panic!("PipeWire metadata must be preserved");
        };
        assert_eq!((first.event_offset, first.event_position), (317, 48_317));
        assert_eq!((second.event_offset, second.event_position), (851, 48_851));
        assert_ne!(first.event_position, second.event_position);
    }

    #[test]
    fn timestamp_overflow_is_not_reported_as_capacity_overflow() {
        let mut parser = MidiParser::default();
        let mut events: Vec<CapturedEvent> = Vec::with_capacity(1);

        let result = record_control_bytes(
            &mut parser,
            &mut events,
            i64::MAX,
            u32::MAX,
            1,
            48_000,
            1024,
            &[0xf8],
        );

        assert!(matches!(
            result,
            Err(crate::AppError::TimestampArithmeticOverflow)
        ));
    }
}
