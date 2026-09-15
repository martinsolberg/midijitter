use crate::{AppError, CapturedEvent, TimestampMetadata};

const NANOSECONDS_PER_SECOND: i128 = 1_000_000_000;

/// Converts an ALSA `CLOCK_MONOTONIC_RAW` timestamp to absolute nanoseconds.
pub fn alsa_absolute_ns(tv_sec: i64, tv_nsec: i64) -> Result<i128, AppError> {
    if tv_sec < 0 || tv_nsec < 0 || tv_nsec >= NANOSECONDS_PER_SECOND as i64 {
        return Err(AppError::InvalidCapture(
            "ALSA timestamp is out of range".to_owned(),
        ));
    }
    i128::from(tv_sec)
        .checked_mul(NANOSECONDS_PER_SECOND)
        .and_then(|seconds| seconds.checked_add(i128::from(tv_nsec)))
        .ok_or(AppError::TimestampArithmeticOverflow)
}

/// Normalizes ALSA event timestamps against the first captured event.
///
/// The first event's absolute `CLOCK_MONOTONIC_RAW` timestamp is the sole
/// epoch selection point. Raw absolute timestamps stay on each event.
pub fn normalize_alsa_event_timestamps(events: &mut [CapturedEvent]) -> Result<(), AppError> {
    let first_event = events.first().ok_or_else(|| {
        AppError::InvalidCapture("cannot normalize an empty ALSA capture".to_owned())
    })?;
    let TimestampMetadata::Alsa(first_timestamp) = &first_event.timestamp_metadata else {
        return Err(AppError::InvalidCapture(
            "ALSA capture must carry ALSA timestamps".to_owned(),
        ));
    };
    let first_absolute_ns = first_timestamp.absolute_ns;

    for event in events {
        let TimestampMetadata::Alsa(timestamp) = &event.timestamp_metadata else {
            return Err(AppError::InvalidCapture(
                "ALSA capture must carry ALSA timestamps".to_owned(),
            ));
        };
        event.timestamp_ns = timestamp
            .absolute_ns
            .checked_sub(first_absolute_ns)
            .ok_or(AppError::TimestampArithmeticOverflow)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{AlsaTimestamp, CapturedEvent, MidiEvent, TimestampMetadata};

    use super::{alsa_absolute_ns, normalize_alsa_event_timestamps};

    fn alsa_event(sequence: u64, absolute_ns: i128) -> CapturedEvent {
        CapturedEvent {
            sequence,
            timestamp_ns: 0,
            event: MidiEvent::Clock,
            timestamp_metadata: TimestampMetadata::Alsa(AlsaTimestamp {
                absolute_ns,
                clock: "monotonic-raw".to_owned(),
                timestamped_read: true,
            }),
        }
    }

    #[test]
    fn converts_monotonic_raw_timestamps_to_nanoseconds() {
        assert_eq!(alsa_absolute_ns(10, 500_000_000).unwrap(), 10_500_000_000);
        assert_eq!(alsa_absolute_ns(0, 0).unwrap(), 0);
    }

    #[test]
    fn rejects_out_of_range_timestamps() {
        assert!(alsa_absolute_ns(-1, 0).is_err());
        assert!(alsa_absolute_ns(0, -1).is_err());
        assert!(alsa_absolute_ns(0, 1_000_000_000).is_err());
    }

    #[test]
    fn normalization_uses_the_first_nonzero_absolute_epoch() {
        let mut events = vec![
            alsa_event(0, 9_000_000_000),
            alsa_event(1, 9_020_833_333),
            alsa_event(2, 9_041_666_666),
        ];

        normalize_alsa_event_timestamps(&mut events).unwrap();

        assert_eq!(events[0].timestamp_ns, 0);
        assert_eq!(events[1].timestamp_ns, 20_833_333);
        assert_eq!(events[2].timestamp_ns, 41_666_666);
        // Raw absolute timestamps are retained.
        let TimestampMetadata::Alsa(first) = &events[0].timestamp_metadata else {
            panic!("ALSA metadata must be preserved");
        };
        assert_eq!(first.absolute_ns, 9_000_000_000);
    }

    #[test]
    fn normalization_rejects_empty_captures() {
        assert!(normalize_alsa_event_timestamps(&mut []).is_err());
    }
}
