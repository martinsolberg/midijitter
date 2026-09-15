use midijitter::backend::pipewire::timing::{
    normalize_pipewire_event_timestamps, pipewire_event_position, relative_ns,
};
use midijitter::{
    AnalysisOptions, AppError, CaptureFile, CapturedEvent, EnvironmentMetadata, GraphTransition,
    MidiEvent, PipeWireTimestamp, SourceMetadata, TimestampMetadata, analyze,
};

#[test]
fn canonical_graph_position_conversion() {
    assert_eq!(pipewire_event_position(480_000, 240).unwrap(), 480_240);
    assert_eq!(relative_ns(480_240, 0, 1, 48_000).unwrap(), 10_005_000_000);
}

#[test]
fn relative_ns_uses_the_supplied_rational_graph_rates() {
    for (rate_num, rate_denom, position, expected_ns) in [
        (1, 44_100, 44_100, 1_000_000_000),
        (1, 48_000, 48_000, 1_000_000_000),
        (1, 96_000, 96_000, 1_000_000_000),
        (1001, 48_000, 48_000, 1_001_000_000_000),
    ] {
        assert_eq!(
            relative_ns(position, 0, rate_num, rate_denom).unwrap(),
            expected_ns
        );
    }
}

#[test]
fn normalization_uses_the_first_nonzero_event_epoch_for_every_rate_and_quantum() {
    for rate_denom in [44_100_u32, 48_000, 96_000] {
        for quantum in [128_u32, 256, 512, 1024, 2048] {
            let first_position = 5_000_000_i64 + i64::from(quantum);
            let later_position = first_position + i64::from(rate_denom);
            let mut events = vec![
                pipewire_clock_event(1, first_position, rate_denom, quantum),
                pipewire_clock_event(2, later_position, rate_denom, quantum),
            ];

            normalize_pipewire_event_timestamps(&mut events).unwrap();

            assert_eq!(events[0].timestamp_ns, 0);
            assert_eq!(events[1].timestamp_ns, 1_000_000_000);
            assert_eq!(event_position(&events[0]), first_position);
            assert_eq!(event_position(&events[1]), later_position);
        }
    }
}

#[test]
fn graph_position_overflow_is_rejected() {
    assert!(matches!(
        pipewire_event_position(i64::MAX, 1),
        Err(AppError::TimestampArithmeticOverflow)
    ));
}

#[test]
fn analysis_rejects_captures_with_graph_rate_transitions() {
    let capture = CaptureFile {
        format_version: 1,
        backend: "pipewire".to_owned(),
        source: SourceMetadata {
            identity: "test-source".to_owned(),
            display_name: "Test source".to_owned(),
        },
        timestamp_method: "pipewire-graph-position-plus-event-offset".to_owned(),
        ppqn: 24,
        application_version: "test".to_owned(),
        environment: EnvironmentMetadata {
            operating_system: "test".to_owned(),
            pipewire_version: None,
        },
        transitions: vec![GraphTransition {
            event_sequence: 2,
            rate_num: 1,
            rate_denom: 44_100,
            quantum: 512,
        }],
        events: vec![clock_event(1, 0, 0), clock_event(2, 48_000, 1_000_000_000)],
    };

    assert!(matches!(
        analyze(&capture, AnalysisOptions::default()),
        Err(AppError::GraphRateTransitionUnsupported)
    ));
}

fn clock_event(sequence: u64, event_position: i64, timestamp_ns: i128) -> CapturedEvent {
    CapturedEvent {
        sequence,
        timestamp_ns,
        event: MidiEvent::Clock,
        timestamp_metadata: TimestampMetadata::PipeWire(PipeWireTimestamp {
            cycle_position: event_position,
            event_offset: 0,
            event_position,
            rate_num: 1,
            rate_denom: 48_000,
            quantum: 256,
        }),
    }
}

fn pipewire_clock_event(
    sequence: u64,
    event_position: i64,
    rate_denom: u32,
    quantum: u32,
) -> CapturedEvent {
    let quantum_i64 = i64::from(quantum);
    let cycle_position = (event_position / quantum_i64) * quantum_i64;

    CapturedEvent {
        sequence,
        timestamp_ns: -1,
        event: MidiEvent::Clock,
        timestamp_metadata: TimestampMetadata::PipeWire(PipeWireTimestamp {
            cycle_position,
            event_offset: u32::try_from(event_position - cycle_position).unwrap(),
            event_position,
            rate_num: 1,
            rate_denom,
            quantum,
        }),
    }
}

fn event_position(event: &CapturedEvent) -> i64 {
    let TimestampMetadata::PipeWire(timestamp) = &event.timestamp_metadata else {
        panic!("test fixtures carry PipeWire timestamps");
    };
    timestamp.event_position
}
