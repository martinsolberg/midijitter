use crate::{AppError, CapturedEvent, TimestampMetadata};

const NANOSECONDS_PER_SECOND: i128 = 1_000_000_000;

/// Combines PipeWire's absolute graph cycle position and the event's offset
/// within that cycle without losing sample precision.
pub fn pipewire_event_position(cycle_position: i64, offset: u32) -> Result<i64, AppError> {
    cycle_position
        .checked_add(i64::from(offset))
        .ok_or(AppError::TimestampArithmeticOverflow)
}

/// Converts a graph position to nanoseconds relative to the first captured
/// event, using PipeWire's unmodified rational seconds-per-sample rate.
pub fn relative_ns(
    position: i64,
    first_position: i64,
    rate_num: u32,
    rate_denom: u32,
) -> Result<i128, AppError> {
    if rate_denom == 0 {
        return Err(AppError::InvalidCapture(
            "PipeWire rate denominator must be positive".to_owned(),
        ));
    }

    let position_delta = i128::from(position) - i128::from(first_position);
    position_delta
        .checked_mul(i128::from(rate_num))
        .and_then(|value| value.checked_mul(NANOSECONDS_PER_SECOND))
        .map(|value| value / i128::from(rate_denom))
        .ok_or(AppError::TimestampArithmeticOverflow)
}

/// Normalizes captured event timestamps against the first captured event.
///
/// The first event's absolute PipeWire graph position is the sole epoch
/// selection point. Raw graph positions and all other timestamp metadata are
/// retained on each event.
pub fn normalize_pipewire_event_timestamps(events: &mut [CapturedEvent]) -> Result<(), AppError> {
    let first_event = events.first().ok_or_else(|| {
        AppError::InvalidCapture("cannot normalize an empty PipeWire capture".to_owned())
    })?;
    let TimestampMetadata::PipeWire(first_timestamp) = &first_event.timestamp_metadata else {
        return Err(AppError::InvalidCapture(
            "PipeWire capture must carry PipeWire timestamps".to_owned(),
        ));
    };
    let first_position = first_timestamp.event_position;
    let rate_num = first_timestamp.rate_num;
    let rate_denom = first_timestamp.rate_denom;

    for event in events {
        let TimestampMetadata::PipeWire(timestamp) = &event.timestamp_metadata else {
            return Err(AppError::InvalidCapture(
                "PipeWire capture must carry PipeWire timestamps".to_owned(),
            ));
        };
        event.timestamp_ns = relative_ns(
            timestamp.event_position,
            first_position,
            rate_num,
            rate_denom,
        )?;
    }

    Ok(())
}
