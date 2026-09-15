use midijitter::{
    AppError, CaptureFile, CapturedEvent, EnvironmentMetadata, GraphTransition, MidiEvent,
    PipeWireTimestamp, SourceMetadata, TimestampMetadata,
};

fn fixture_capture() -> CaptureFile {
    CaptureFile {
        format_version: 1,
        backend: "pipewire".to_owned(),
        source: SourceMetadata {
            identity: "alsa_input.usb-midi:0".to_owned(),
            display_name: "USB MIDI".to_owned(),
        },
        timestamp_method: "pipewire-graph-position-plus-event-offset".to_owned(),
        ppqn: 24,
        application_version: "0.1.0".to_owned(),
        environment: EnvironmentMetadata {
            operating_system: "Debian GNU/Linux 13".to_owned(),
            pipewire_version: Some("1.2.0".to_owned()),
        },
        transitions: vec![GraphTransition {
            event_sequence: 1,
            rate_num: 1,
            rate_denom: 48_000,
            quantum: 256,
        }],
        events: vec![CapturedEvent {
            sequence: 1,
            timestamp_ns: 0,
            event: MidiEvent::Clock,
            timestamp_metadata: TimestampMetadata::PipeWire(PipeWireTimestamp {
                cycle_position: 48_000,
                event_offset: 37,
                event_position: 48_037,
                rate_num: 1,
                rate_denom: 48_000,
                quantum: 256,
            }),
        }],
    }
}

#[test]
fn capture_round_trip_preserves_pipewire_graph_timestamp() {
    let capture = fixture_capture();
    let json = serde_json::to_string(&capture).unwrap();
    assert_eq!(serde_json::from_str::<CaptureFile>(&json).unwrap(), capture);
}

#[test]
fn validation_rejects_unsupported_format_version() {
    let mut capture = fixture_capture();
    capture.format_version = 2;

    assert!(matches!(
        capture.validate(),
        Err(AppError::UnsupportedCaptureFormat(2))
    ));
}

#[test]
fn validation_rejects_empty_source_identity() {
    let mut capture = fixture_capture();
    capture.source.identity.clear();

    assert!(matches!(
        capture.validate(),
        Err(AppError::InvalidCapture(message)) if message == "source identity must not be empty"
    ));
}

#[test]
fn validation_rejects_zero_pipewire_rate_denominator() {
    let mut capture = fixture_capture();
    let TimestampMetadata::PipeWire(timestamp) = &mut capture.events[0].timestamp_metadata;
    timestamp.rate_denom = 0;

    assert!(matches!(
        capture.validate(),
        Err(AppError::InvalidCapture(message)) if message == "PipeWire rate denominator must be positive"
    ));
}

#[test]
fn validation_rejects_zero_transition_rate_denominator() {
    let mut capture = fixture_capture();
    capture.transitions[0].rate_denom = 0;

    assert!(matches!(
        capture.validate(),
        Err(AppError::InvalidCapture(message)) if message == "PipeWire rate denominator must be positive"
    ));
}

#[test]
fn validation_rejects_non_monotonic_event_sequences() {
    let mut capture = fixture_capture();
    let timestamp_metadata = capture.events[0].timestamp_metadata.clone();
    capture.events.push(CapturedEvent {
        sequence: 1,
        timestamp_ns: 20_833_333,
        event: MidiEvent::Clock,
        timestamp_metadata,
    });

    assert!(matches!(
        capture.validate(),
        Err(AppError::InvalidCapture(message)) if message == "event sequence numbers must be strictly increasing"
    ));
}

#[test]
fn json_parser_rejects_unsupported_format_version() {
    let mut capture = fixture_capture();
    capture.format_version = 2;
    let json = serde_json::to_string(&capture).unwrap();

    assert!(matches!(
        CaptureFile::from_json_str(&json),
        Err(AppError::UnsupportedCaptureFormat(2))
    ));
}
