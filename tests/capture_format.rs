use midijitter::{
    AlsaTimestamp, AppError, CaptureCompletion, CaptureDocument, CaptureFile, CapturedEvent,
    CommonGraphMetadata, CompletionStatus, EnvironmentMetadata, GraphTiming, GraphTransition,
    GraphTransitionKind, MidiEvent, PairedCapture, PairedGraphTransition, PipeWireTimestamp,
    SourceMetadata, TimestampMetadata,
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
fn capture_round_trip_preserves_alsa_timestamps() {
    let mut capture = fixture_capture();
    capture.backend = "alsa-raw".to_owned();
    capture.events[0].timestamp_metadata = TimestampMetadata::Alsa(AlsaTimestamp {
        absolute_ns: 9_000_000_000,
        clock: "monotonic-raw".to_owned(),
        timestamped_read: true,
    });

    let json = serde_json::to_string(&capture).unwrap();
    assert!(json.contains("\"alsa\""));
    let parsed = CaptureFile::from_json_str(&json).unwrap();
    assert_eq!(parsed, capture);
    parsed.validate().unwrap();
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
    let TimestampMetadata::PipeWire(timestamp) = &mut capture.events[0].timestamp_metadata else {
        panic!("test fixtures carry PipeWire timestamps");
    };
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

#[test]
fn validated_json_parser_rejects_ppqn_values_other_than_24() {
    for ppqn in [0, 23, 25, u32::MAX] {
        let mut capture = fixture_capture();
        capture.ppqn = ppqn;
        let json = serde_json::to_string(&capture).unwrap();

        assert!(matches!(
            CaptureFile::from_json_str(&json),
            Err(AppError::InvalidCapture(_))
        ));
    }
}

fn fixture_paired_capture() -> PairedCapture {
    let common = CommonGraphMetadata {
        clock_id: 7,
        rate_num: 1,
        rate_denom: 48_000,
        quantum: 256,
        origin_position: 10_000,
    };
    let origin_position = common.origin_position;
    let rate_num = common.rate_num;
    let rate_denom = common.rate_denom;
    let event = |sequence, position| CapturedEvent {
        sequence,
        timestamp_ns: (i128::from(position - origin_position)
            * i128::from(rate_num)
            * 1_000_000_000)
            / i128::from(rate_denom),
        event: MidiEvent::Clock,
        timestamp_metadata: TimestampMetadata::PipeWire(PipeWireTimestamp {
            cycle_position: position - 10,
            event_offset: 10,
            event_position: position,
            rate_num,
            rate_denom,
            quantum: 256,
        }),
    };
    PairedCapture {
        format_version: 2,
        backend: "pipewire".to_owned(),
        reference: SourceMetadata {
            identity: "ref".to_owned(),
            display_name: "Reference".to_owned(),
        },
        returned: SourceMetadata {
            identity: "ret".to_owned(),
            display_name: "Returned".to_owned(),
        },
        timestamp_method: "common-graph-origin".to_owned(),
        ppqn: 24,
        application_version: "0.1.0".to_owned(),
        environment: EnvironmentMetadata {
            operating_system: "test".to_owned(),
            pipewire_version: None,
        },
        common_graph: common,
        transitions: vec![PairedGraphTransition {
            event_sequence: 0,
            kind: GraphTransitionKind::Initial,
            clock_id: 7,
            timing: GraphTiming {
                rate_num: 1,
                rate_denom: 48_000,
                quantum: 256,
            },
        }],
        completion: CaptureCompletion {
            status: CompletionStatus::Complete,
            reason: None,
        },
        reference_events: vec![event(1, 10_000)],
        returned_events: vec![event(1, 10_048)],
    }
}

#[test]
fn paired_capture_round_trip_dispatches_as_v2() {
    let capture = fixture_paired_capture();
    let json = serde_json::to_string(&capture).unwrap();
    assert_eq!(PairedCapture::from_json_str(&json).unwrap(), capture);
    assert_eq!(
        CaptureDocument::from_json_str(&json).unwrap(),
        CaptureDocument::V2(capture)
    );
}

#[test]
fn paired_capture_rejects_event_not_relative_to_common_origin() {
    let mut capture = fixture_paired_capture();
    capture.returned_events[0].timestamp_ns += 1;
    assert!(matches!(
        capture.validate(),
        Err(AppError::InconsistentCommonTimebase(message))
            if message == "event timestamp is not relative to the common origin"
    ));
}

#[test]
fn capture_document_dispatch_preserves_v1() {
    let capture = fixture_capture();
    let json = serde_json::to_string(&capture).unwrap();
    assert_eq!(
        CaptureDocument::from_json_str(&json).unwrap(),
        CaptureDocument::V1(capture)
    );
}
