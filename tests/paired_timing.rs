use midijitter::{AppError, common_timestamp_ns};

#[test]
fn common_origin_conversion_handles_same_and_cross_cycle_offsets() {
    assert_eq!(common_timestamp_ns(10_000, 10_000, 1, 48_000).unwrap(), 0);
    assert_eq!(
        common_timestamp_ns(10_037, 10_000, 1, 48_000).unwrap(),
        770_833
    );
    assert_eq!(
        common_timestamp_ns(9_900, 10_000, 1, 48_000).unwrap(),
        -2_083_333
    );
}

#[test]
fn common_origin_conversion_rejects_invalid_rates() {
    assert!(matches!(
        common_timestamp_ns(1, 0, 1, 0),
        Err(AppError::InconsistentCommonTimebase(_))
    ));
}
