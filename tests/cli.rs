use std::process::Command;

use midijitter::AppError;
use midijitter::backend::{MidiSource, select_source};

#[test]
fn devices_reports_unavailable_pipewire_with_a_nonzero_exit_status() {
    let output = Command::new(env!("CARGO_BIN_EXE_midijitter"))
        .args(["devices", "--backend", "pipewire"])
        .env("PIPEWIRE_REMOTE", "midijitter-test-unavailable-remote")
        .output()
        .expect("midijitter binary should run");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("PipeWire daemon unavailable"));
}

#[test]
fn duplicate_display_name_requires_a_stable_source_identity() {
    let sources = vec![
        source("USB MIDI", "usb-midi-a", "out", Some("101"), 10, 20),
        source("USB MIDI", "usb-midi-b", "out", Some("102"), 11, 21),
    ];

    let error = select_source(&sources, "USB MIDI").expect_err("duplicate name must be rejected");

    assert!(matches!(error, AppError::AmbiguousSource { .. }));
    assert_eq!(
        error.to_string(),
        "source selector \"USB MIDI\" is ambiguous; use one of: usb-midi-a/out#101, usb-midi-b/out#102"
    );
}

#[test]
fn source_selection_errors_have_documented_exit_codes() {
    let source = source("USB MIDI", "usb-midi", "out", Some("101"), 10, 20);

    let no_source = select_source(&[], "USB MIDI").expect_err("empty list must reject selection");
    let not_found = select_source(std::slice::from_ref(&source), "missing")
        .expect_err("unknown source must be rejected");
    let ambiguous = select_source(&[source.clone(), source], "USB MIDI")
        .expect_err("duplicate displayed name must be rejected");
    let permission = AppError::PipeWirePermissionDenied {
        detail: "access denied".to_owned(),
    };

    assert!(matches!(no_source, AppError::NoSource));
    assert_eq!(no_source.exit_code(), 5);
    assert!(matches!(not_found, AppError::SourceNotFound { .. }));
    assert_eq!(not_found.exit_code(), 5);
    assert!(matches!(ambiguous, AppError::AmbiguousSource { .. }));
    assert_eq!(ambiguous.exit_code(), 5);
    assert_eq!(permission.exit_code(), 4);
}

fn source(
    display_name: &str,
    node_name: &str,
    port_name: &str,
    object_serial: Option<&str>,
    node_id: u32,
    port_id: u32,
) -> MidiSource {
    MidiSource {
        display_name: display_name.to_owned(),
        node_name: node_name.to_owned(),
        port_name: port_name.to_owned(),
        object_serial: object_serial.map(str::to_owned),
        node_id,
        port_id,
    }
}
