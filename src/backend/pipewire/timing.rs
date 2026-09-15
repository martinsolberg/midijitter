use crate::AppError;

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
