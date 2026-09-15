use crate::AppError;
use pipewire as pw;

pub(super) struct PositionTiming {
    pub cycle_ticks: i64,
    pub rate_num: u32,
    pub rate_denom: u32,
    pub quantum: u32,
}

pub(super) fn position_timing(
    position: &pw::spa::sys::spa_io_position,
) -> Result<PositionTiming, AppError> {
    let clock = &position.clock;
    let (rate_num, rate_denom) = (clock.rate.num, clock.rate.denom);
    let quantum =
        u32::try_from(clock.duration).map_err(|_| AppError::TimestampArithmeticOverflow)?;
    if rate_num == 0 || rate_denom == 0 || quantum == 0 {
        return Err(AppError::PipeWireNegotiationFailed {
            detail: "PipeWire filter reported an invalid graph rate or quantum".to_owned(),
        });
    }
    let cycle_ticks =
        i64::try_from(clock.position).map_err(|_| AppError::TimestampArithmeticOverflow)?;
    Ok(PositionTiming {
        cycle_ticks,
        rate_num,
        rate_denom,
        quantum,
    })
}

#[cfg(test)]
mod tests {
    use super::position_timing;
    use pipewire as pw;

    fn position(
        rate_num: u32,
        rate_denom: u32,
        ticks: u64,
        duration: u64,
    ) -> pw::spa::sys::spa_io_position {
        // SAFETY: zeroed plain-data bindings struct, fully populated below.
        let mut position: pw::spa::sys::spa_io_position = unsafe { std::mem::zeroed() };
        position.clock.rate.num = rate_num;
        position.clock.rate.denom = rate_denom;
        position.clock.position = ticks;
        position.clock.duration = duration;
        position
    }

    #[test]
    fn position_timing_extracts_ticks_rate_and_quantum() {
        let timing = position_timing(&position(1, 48000, 480000, 1024)).unwrap();
        assert_eq!(
            (
                timing.cycle_ticks,
                timing.rate_num,
                timing.rate_denom,
                timing.quantum
            ),
            (480000, 1, 48000, 1024)
        );
    }

    #[test]
    fn position_timing_rejects_zero_rate_or_quantum() {
        assert!(position_timing(&position(0, 48000, 1, 1024)).is_err());
        assert!(position_timing(&position(1, 0, 1, 1024)).is_err());
        assert!(position_timing(&position(1, 48000, 1, 0)).is_err());
    }
}
