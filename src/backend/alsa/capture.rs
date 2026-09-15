use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use alsa::Direction;
use alsa::rawmidi::{Clock, Rawmidi, ReadMode};

use crate::backend::{CaptureRequest, CaptureTermination};
use crate::capture::MidiParser;
use crate::{
    AlsaTimestamp, AppError, CaptureFile, CapturedEvent, EnvironmentMetadata, SourceMetadata,
    TimestampMetadata,
};

use super::timing::{alsa_absolute_ns, normalize_alsa_event_timestamps};
use super::{map_capture_error, map_open_error};

const MAX_MIDI_BYTES_PER_SECOND: u64 = 3_125;
const EVENT_SAFETY_MARGIN: u64 = 128;
const POLL_INTERVAL: Duration = Duration::from_millis(1);
const READ_BUFFER_BYTES: usize = 1_024;
const NANOSECONDS_PER_SECOND: i128 = 1_000_000_000;

pub const CLOCK_MONOTONIC_RAW: &str = "monotonic-raw";
pub const CLOCK_USERSPACE_MONOTONIC_RAW: &str = "userspace-monotonic-raw";

pub const TIMESTAMP_METHOD_KERNEL: &str = "ALSA timestamped RawMIDI (CLOCK_MONOTONIC_RAW)";
pub const TIMESTAMP_METHOD_USERSPACE: &str =
    "userspace timestamps after read (CLOCK_MONOTONIC_RAW)";

/// Which clock produced the absolute ALSA timestamps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AlsaClock {
    MonotonicRaw,
    Userspace,
}

impl AlsaClock {
    fn label(self) -> &'static str {
        match self {
            Self::MonotonicRaw => CLOCK_MONOTONIC_RAW,
            Self::Userspace => CLOCK_USERSPACE_MONOTONIC_RAW,
        }
    }
}

pub(super) fn record(request: CaptureRequest) -> Result<CaptureFile, AppError> {
    let device = request.source.alsa_device.clone().ok_or_else(|| {
        AppError::InvalidCapture("selected source is not an ALSA RawMIDI device".to_owned())
    })?;
    let event_capacity = event_capacity(&request.termination)?;

    let mut handle = Rawmidi::new(&device, Direction::Capture, true)
        .map_err(|error| map_open_error(&error, &device))?;
    let clock = configure_clock(&mut handle, &device, request.allow_userspace_timestamps)?;

    let interrupted = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        signal_hook::flag::register(signal, Arc::clone(&interrupted))
            .map_err(|error| AppError::SignalHandler(error.to_string()))?;
    }

    let duration_ns = match request.termination {
        CaptureTermination::DurationSeconds(seconds) => i128::from(seconds)
            .checked_mul(NANOSECONDS_PER_SECOND)
            .ok_or_else(|| {
                AppError::InvalidCapture("capture termination is too large".to_owned())
            })?,
        CaptureTermination::Ticks(_) => 0,
    };
    // Wall-clock backstop so a silent source cannot hang a
    // duration-bounded capture forever; data timestamps decide the content.
    let deadline = match request.termination {
        CaptureTermination::DurationSeconds(seconds) => {
            Some(Instant::now() + Duration::from_secs(seconds))
        }
        CaptureTermination::Ticks(_) => None,
    };

    let mut parser = MidiParser::default();
    let mut events: Vec<CapturedEvent> = Vec::with_capacity(event_capacity);
    let mut clock_events: u64 = 0;
    let mut first_absolute_ns: Option<i128> = None;
    let mut last_absolute_ns: i128 = 0;
    let mut buffer = [0_u8; READ_BUFFER_BYTES];

    loop {
        if interrupted.load(Ordering::Acquire) {
            break;
        }
        if let CaptureTermination::Ticks(target) = request.termination
            && clock_events >= target
        {
            break;
        }
        if let Some(first) = first_absolute_ns
            && duration_ns > 0
            && last_absolute_ns - first >= duration_ns
        {
            break;
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            break;
        }

        let (absolute_ns, length) = match read_chunk(&handle, &mut buffer, clock) {
            Ok(chunk) => chunk,
            Err(ReadOutcome::Retry) => {
                std::thread::sleep(POLL_INTERVAL);
                continue;
            }
            Err(ReadOutcome::Interrupted) => {
                if interrupted.load(Ordering::Acquire) {
                    break;
                }
                continue;
            }
            Err(ReadOutcome::Failed(error)) => {
                return Err(map_capture_error(&error, &device));
            }
        };
        if length == 0 {
            std::thread::sleep(POLL_INTERVAL);
            continue;
        }

        let before = events.len();
        if record_alsa_bytes(
            &mut parser,
            &mut events,
            &buffer[..length],
            absolute_ns,
            clock,
            clock == AlsaClock::MonotonicRaw,
        )
        .is_err()
        {
            return Err(AppError::CaptureOverflow);
        }
        clock_events += events[before..]
            .iter()
            .filter(|event| matches!(event.event, crate::MidiEvent::Clock))
            .count() as u64;
        if first_absolute_ns.is_none() && !events.is_empty() {
            first_absolute_ns = Some(absolute_ns);
        }
        last_absolute_ns = absolute_ns;
    }

    if events.is_empty() {
        return Err(AppError::NoClockEvents);
    }
    normalize_alsa_event_timestamps(&mut events)?;
    Ok(CaptureFile {
        format_version: crate::capture::CURRENT_FORMAT_VERSION,
        backend: "alsa-raw".to_owned(),
        source: SourceMetadata {
            identity: request.source.stable_identity(),
            display_name: request.source.display_name,
        },
        timestamp_method: match clock {
            AlsaClock::MonotonicRaw => TIMESTAMP_METHOD_KERNEL.to_owned(),
            AlsaClock::Userspace => TIMESTAMP_METHOD_USERSPACE.to_owned(),
        },
        ppqn: 24,
        application_version: env!("CARGO_PKG_VERSION").to_owned(),
        environment: EnvironmentMetadata {
            operating_system: std::env::consts::OS.to_owned(),
            pipewire_version: None,
        },
        transitions: Vec::new(),
        events,
    })
}

enum ReadOutcome {
    Retry,
    Interrupted,
    Failed(alsa::Error),
}

fn read_chunk(
    handle: &Rawmidi,
    buffer: &mut [u8],
    clock: AlsaClock,
) -> Result<(i128, usize), ReadOutcome> {
    match clock {
        AlsaClock::MonotonicRaw => match handle.tread(buffer) {
            Ok((timestamp, length)) => {
                let absolute_ns =
                    alsa_absolute_ns(timestamp.tv_sec, timestamp.tv_nsec).map_err(|_| {
                        ReadOutcome::Failed(alsa::Error::new("snd_rawmidi_tread", -libc::EINVAL))
                    })?;
                Ok((absolute_ns, length))
            }
            Err(error) => Err(classify_read_error(&error)),
        },
        AlsaClock::Userspace => match handle.read(buffer) {
            Ok(length) => {
                let absolute_ns = monotonic_raw_now().map_err(|_| {
                    ReadOutcome::Failed(alsa::Error::new("clock_gettime", -libc::ENOTSUP))
                })?;
                Ok((absolute_ns, length))
            }
            Err(error) => Err(classify_read_error(&error)),
        },
    }
}

fn classify_read_error(error: &alsa::Error) -> ReadOutcome {
    match error.errno() {
        errno if errno == -libc::EAGAIN => ReadOutcome::Retry,
        errno if errno == -libc::EINTR => ReadOutcome::Interrupted,
        _ => ReadOutcome::Failed(alsa::Error::new(error.func(), error.errno())),
    }
}

/// Uses only `CLOCK_MONOTONIC_RAW`; never wall-clock time.
fn monotonic_raw_now() -> Result<i128, AppError> {
    let mut timestamp = std::mem::MaybeUninit::<libc::timespec>::uninit();
    // SAFETY: `timestamp` is a valid writable `timespec`; the kernel fills it
    // on success, and the return value is checked before use.
    let result = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC_RAW, timestamp.as_mut_ptr()) };
    if result != 0 {
        return Err(AppError::AlsaUnavailable {
            detail: "clock_gettime(CLOCK_MONOTONIC_RAW) failed".to_owned(),
        });
    }
    let timestamp = unsafe { timestamp.assume_init() };
    alsa_absolute_ns(timestamp.tv_sec, timestamp.tv_nsec)
}

/// Selects kernel timestamping, or the explicit userspace fallback.
fn configure_clock(
    handle: &mut Rawmidi,
    device: &str,
    allow_userspace: bool,
) -> Result<AlsaClock, AppError> {
    let mut params =
        alsa::rawmidi::Params::new().map_err(|error| map_open_error(&error, device))?;
    let configured = params
        .set_read_mode(handle, ReadMode::Timestamp)
        .and_then(|()| params.set_clock_type(handle, Clock::MonotonicRaw))
        .and_then(|()| handle.params(&params));
    match configured {
        Ok(()) => Ok(AlsaClock::MonotonicRaw),
        Err(error) if is_unsupported(&error) => {
            if allow_userspace {
                Ok(AlsaClock::Userspace)
            } else {
                Err(AppError::AlsaTimestampUnsupported)
            }
        }
        Err(error) => Err(map_open_error(&error, device)),
    }
}

fn is_unsupported(error: &alsa::Error) -> bool {
    matches!(
        error.errno(),
        errno if errno == -libc::EINVAL || errno == -libc::ENOTTY
    )
}

fn event_capacity(termination: &CaptureTermination) -> Result<usize, AppError> {
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

/// Appends recognized realtime MIDI bytes sharing one ALSA timestamp.
///
/// `events` must be preallocated by the caller; the capacity check keeps the
/// bounded-capture guarantee.
pub(super) fn record_alsa_bytes(
    parser: &mut MidiParser,
    events: &mut Vec<CapturedEvent>,
    bytes: &[u8],
    absolute_ns: i128,
    clock: AlsaClock,
    timestamped_read: bool,
) -> Result<(), AppError> {
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
            timestamp_metadata: TimestampMetadata::Alsa(AlsaTimestamp {
                absolute_ns,
                clock: clock.label().to_owned(),
                timestamped_read,
            }),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::capture::MidiParser;
    use crate::{CapturedEvent, TimestampMetadata};

    use super::{AlsaClock, record_alsa_bytes};

    #[test]
    fn chunks_keep_their_kernel_timestamps() {
        let mut parser = MidiParser::default();
        let mut events: Vec<CapturedEvent> = Vec::with_capacity(4);

        record_alsa_bytes(
            &mut parser,
            &mut events,
            &[0xf8],
            5_000_000_000,
            AlsaClock::MonotonicRaw,
            true,
        )
        .unwrap();
        record_alsa_bytes(
            &mut parser,
            &mut events,
            &[0xf8, 0xfa],
            5_020_833_333,
            AlsaClock::MonotonicRaw,
            true,
        )
        .unwrap();

        assert_eq!(events.len(), 3);
        for event in &events {
            let TimestampMetadata::Alsa(timestamp) = &event.timestamp_metadata else {
                panic!("ALSA metadata must be preserved");
            };
            assert!(timestamp.timestamped_read);
            assert_eq!(timestamp.clock, "monotonic-raw");
        }
        let TimestampMetadata::Alsa(first) = &events[0].timestamp_metadata else {
            panic!("ALSA metadata must be preserved");
        };
        let TimestampMetadata::Alsa(last) = &events[2].timestamp_metadata else {
            panic!("ALSA metadata must be preserved");
        };
        assert_eq!(first.absolute_ns, 5_000_000_000);
        assert_eq!(last.absolute_ns, 5_020_833_333);
    }

    #[test]
    fn userspace_fallback_is_labeled() {
        let mut parser = MidiParser::default();
        let mut events: Vec<CapturedEvent> = Vec::with_capacity(1);

        record_alsa_bytes(
            &mut parser,
            &mut events,
            &[0xf8],
            7_000_000_000,
            AlsaClock::Userspace,
            false,
        )
        .unwrap();

        let TimestampMetadata::Alsa(timestamp) = &events[0].timestamp_metadata else {
            panic!("ALSA metadata must be preserved");
        };
        assert!(!timestamp.timestamped_read);
        assert_eq!(timestamp.clock, "userspace-monotonic-raw");
    }

    #[test]
    fn bounded_capacity_is_enforced() {
        let mut parser = MidiParser::default();
        let mut events: Vec<CapturedEvent> = Vec::with_capacity(1);
        record_alsa_bytes(
            &mut parser,
            &mut events,
            &[0xf8],
            1,
            AlsaClock::MonotonicRaw,
            true,
        )
        .unwrap();

        let result = record_alsa_bytes(
            &mut parser,
            &mut events,
            &[0xf8],
            2,
            AlsaClock::MonotonicRaw,
            true,
        );
        assert!(matches!(result, Err(crate::AppError::CaptureOverflow)));
    }
}
