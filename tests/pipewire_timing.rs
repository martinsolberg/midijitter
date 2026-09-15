use midijitter::backend::pipewire::timing::{pipewire_event_position, relative_ns};
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
fn event_time_is_independent_of_graph_quantum_segmentation() {
    for quantum in [128_i64, 256, 512, 1024, 2048] {
        let cycle_position = (48_000 / quantum) * quantum;
        let offset = u32::try_from(48_000 - cycle_position).unwrap();
        let position = pipewire_event_position(cycle_position, offset).unwrap();

        assert_eq!(position, 48_000);
        assert_eq!(relative_ns(position, 0, 1, 48_000).unwrap(), 1_000_000_000);
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
