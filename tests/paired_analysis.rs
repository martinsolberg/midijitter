use midijitter::analysis::{PairStatus, analyze_paired};
use midijitter::output::text::format_paired_report;
use midijitter::{
    CaptureCompletion, CommonGraphMetadata, CompletionStatus, EnvironmentMetadata, GraphTiming,
    GraphTransitionKind, MidiEvent, PairedCapture, PairedGraphTransition, PipeWireTimestamp,
    SourceMetadata, TimestampMetadata,
};

fn capture(reference: &[i128], returned: &[i128]) -> PairedCapture {
    let common = CommonGraphMetadata {
        clock_id: 1,
        rate_num: 1,
        rate_denom: 1_000_000_000,
        quantum: 256,
        origin_position: 0,
    };
    let events = |values: &[i128]| {
        values
            .iter()
            .enumerate()
            .map(|(index, timestamp_ns)| midijitter::CapturedEvent {
                sequence: index as u64 + 1,
                timestamp_ns: *timestamp_ns,
                event: MidiEvent::Clock,
                timestamp_metadata: TimestampMetadata::PipeWire(PipeWireTimestamp {
                    cycle_position: *timestamp_ns as i64,
                    event_offset: 0,
                    event_position: *timestamp_ns as i64,
                    rate_num: 1,
                    rate_denom: 1_000_000_000,
                    quantum: 256,
                }),
            })
            .collect()
    };
    PairedCapture {
        format_version: 2,
        backend: "pipewire".into(),
        reference: SourceMetadata {
            identity: "ref".into(),
            display_name: "Reference".into(),
        },
        returned: SourceMetadata {
            identity: "ret".into(),
            display_name: "Returned".into(),
        },
        timestamp_method: "common-graph-origin".into(),
        ppqn: 24,
        application_version: "test".into(),
        environment: EnvironmentMetadata {
            operating_system: "test".into(),
            pipewire_version: None,
        },
        common_graph: common,
        transitions: vec![],
        completion: CaptureCompletion {
            status: CompletionStatus::Complete,
            reason: None,
        },
        reference_events: events(reference),
        returned_events: events(returned),
    }
}

fn regular(offset: i128) -> PairedCapture {
    let reference: Vec<_> = (0..12).map(|tick| tick * 1_000).collect();
    let returned: Vec<_> = reference.iter().map(|time| time + offset).collect();
    capture(&reference, &returned)
}

#[test]
fn constant_latency_is_measured_in_integer_nanoseconds() {
    let result = analyze_paired(&regular(37), Default::default()).unwrap();
    assert_eq!(result.latency.median_ns, 37);
    assert_eq!(result.latency.minimum_ns, 37);
}

#[test]
fn source_jitter_cancels_from_path_latency() {
    let reference: Vec<_> = (0..12).map(|tick| tick * 1_000 + (tick % 2) * 17).collect();
    let returned: Vec<_> = reference.iter().map(|time| time + 91).collect();
    let result = analyze_paired(&capture(&reference, &returned), Default::default()).unwrap();
    assert_eq!(result.latency.maximum_ns, 91);
}

#[test]
fn variable_latency_reports_spread() {
    let reference: Vec<_> = (0..12).map(|tick| tick * 1_000).collect();
    let returned: Vec<_> = reference
        .iter()
        .enumerate()
        .map(|(i, time)| time + 20 + i as i128)
        .collect();
    let result = analyze_paired(&capture(&reference, &returned), Default::default()).unwrap();
    assert!(result.latency.maximum_ns > result.latency.minimum_ns);
}

#[test]
fn missing_and_duplicate_ticks_remain_structural_rows() {
    let result = analyze_paired(
        &capture(
            &[0, 1_000, 3_000, 4_000],
            &[25, 1_025, 1_025, 2_025, 3_025, 4_025],
        ),
        midijitter::AnalysisOptions {
            startup_cadence: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(result.pairing.rows.iter().any(|row| matches!(
        row.status,
        PairStatus::MissingReference | PairStatus::MissingReturned
    )));
    assert!(result.pairing.rows.iter().any(|row| {
        row.returned
            .as_ref()
            .is_some_and(|event| event.disposition == midijitter::EventDisposition::Duplicate)
    }));
    assert!(
        result
            .pairing
            .rows
            .iter()
            .all(|row| row.status != PairStatus::Valid
                || (row.reference.is_some() && row.returned.is_some() && row.latency_ns.is_some()))
    );
}

#[test]
fn one_sided_anomaly_does_not_enter_clean_latency_statistics() {
    let result = analyze_paired(
        &capture(
            &[0, 1_000, 2_333, 3_000, 4_000],
            &[25, 1_025, 2_025, 3_025, 4_025],
        ),
        midijitter::AnalysisOptions {
            startup_cadence: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.latency.count, 4);
    assert!(
        result
            .pairing
            .rows
            .iter()
            .any(|row| matches!(row.status, PairStatus::ReferenceAnomalous))
    );
}

#[test]
fn anchored_transport_alignment_is_preferred() {
    let mut paired = regular(25);
    paired.reference_events[0].event = MidiEvent::Start;
    paired.returned_events[0].event = MidiEvent::Start;
    let result = analyze_paired(&paired, Default::default()).unwrap();
    assert!(result.pairing.anchored);
}

#[test]
fn unanchored_sub_period_latency_aligns_without_transport() {
    let result = analyze_paired(&regular(499), Default::default()).unwrap();
    assert!(!result.pairing.anchored);
    assert_eq!(result.latency.median_ns, 499);
}

#[test]
fn startup_asymmetry_resynchronizes_by_logical_ticks() {
    let reference: Vec<_> = (0..12).map(|tick| tick * 1_000).collect();
    let returned: Vec<_> = (-1..12).map(|tick| tick * 1_000 + 500).collect();
    let result = analyze_paired(&capture(&reference, &returned), Default::default()).unwrap();
    assert_eq!(result.latency.median_ns, 500);
}

#[test]
fn ambiguous_multi_period_delay_is_rejected() {
    let reference: Vec<_> = (0..3).map(|tick| tick * 1_000).collect();
    let returned: Vec<_> = (0..3).map(|tick| tick * 1_000 + 2_000).collect();
    assert!(
        analyze_paired(
            &capture(&reference, &returned),
            midijitter::AnalysisOptions {
                startup_cadence: Some(0),
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn serialized_and_immediate_structured_results_are_equivalent() {
    let result = analyze_paired(&regular(37), Default::default()).unwrap();
    let serialized = serde_json::to_string(&result).unwrap();
    let reloaded: midijitter::PairedAnalysisResult = serde_json::from_str(&serialized).unwrap();
    assert_eq!(result, reloaded);
}

#[test]
fn interrupted_capture_is_rejected_by_paired_analysis() {
    let mut paired = regular(37);
    paired.completion.status = CompletionStatus::Interrupted;
    paired.completion.reason = Some("SIGINT".to_owned());
    let error = analyze_paired(&paired, Default::default()).unwrap_err();
    assert!(error.to_string().contains("complete capture"));
}

#[test]
fn graph_timing_transitions_are_structurally_valid_but_rejected_for_analysis() {
    let mut paired = regular(37);
    paired.transitions.push(PairedGraphTransition {
        event_sequence: 12,
        kind: GraphTransitionKind::RateChanged,
        clock_id: 1,
        timing: GraphTiming {
            rate_num: 1,
            rate_denom: 2_000_000_000,
            quantum: 256,
        },
    });
    paired.validate().unwrap();
    assert!(matches!(
        analyze_paired(&paired, Default::default()),
        Err(midijitter::AppError::GraphRateTransitionUnsupported)
    ));
}

#[test]
fn paired_text_report_explains_status_counts_and_resynchronization() {
    let result = analyze_paired(&regular(37), Default::default()).unwrap();
    let report = format_paired_report(&regular(37), &result);
    assert!(report.contains("Pairing status"));
    assert!(report.contains("Valid"));
    assert!(report.contains("Missing reference"));
    assert!(report.contains("Resynchronization"));
    assert!(report.contains("Candidate lags"));
}
