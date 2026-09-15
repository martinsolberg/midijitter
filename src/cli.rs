use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use crate::analysis::{AnalysisOptions, analyze};
use crate::backend::pipewire::PipeWireBackend;
use crate::backend::{CaptureBackend, CaptureRequest, CaptureTermination, select_source};
use crate::output::{self, duration_s, timing_summary};
use crate::{AppError, CaptureFile, MidiEvent};

#[derive(Debug, Parser)]
#[command(name = "midijitter", about = "Measure MIDI clock jitter")]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List available MIDI source ports.
    Devices {
        #[arg(long, value_enum, default_value_t = Backend::Pipewire)]
        backend: Backend,
    },
    /// Record MIDI Clock events into a versioned JSON capture file.
    Record {
        #[arg(long, value_enum, default_value_t = Backend::Pipewire)]
        backend: Backend,
        /// Source display name or stable identity, as shown by `devices`.
        #[arg(long)]
        source: String,
        /// Capture duration in seconds; mutually exclusive with --ticks.
        #[arg(long, conflicts_with = "ticks")]
        duration: Option<u64>,
        /// Stop after capturing this many Clock events; mutually exclusive with --duration.
        #[arg(long, conflicts_with = "duration")]
        ticks: Option<u64>,
        /// Destination capture file.
        #[arg(long)]
        output: PathBuf,
    },
    /// Analyze a capture file and report jitter statistics.
    Analyze {
        /// Capture file produced by `record`.
        capture: PathBuf,
        /// Print the machine-readable JSON report instead of text.
        #[arg(long)]
        json: bool,
        /// Write per-event analysis rows to this CSV file.
        #[arg(long)]
        csv: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Backend {
    Pipewire,
}

pub fn run() -> Result<(), AppError> {
    match Cli::parse().command {
        Command::Devices {
            backend: Backend::Pipewire,
        } => run_devices(),
        Command::Record {
            backend: Backend::Pipewire,
            source,
            duration,
            ticks,
            output,
        } => run_record(source, duration, ticks, output),
        Command::Analyze { capture, json, csv } => run_analyze(capture, json, csv),
    }
}

fn run_devices() -> Result<(), AppError> {
    let sources = PipeWireBackend.enumerate()?;
    if sources.is_empty() {
        println!("No MIDI source ports found.");
    } else {
        for source in sources {
            println!(
                "{}\t{}\tnode-id={} port-id={}",
                source.display_name,
                source.stable_identity(),
                source.node_id,
                source.port_id
            );
        }
    }
    Ok(())
}

fn run_record(
    source: String,
    duration: Option<u64>,
    ticks: Option<u64>,
    output: PathBuf,
) -> Result<(), AppError> {
    let termination = match (duration, ticks) {
        (Some(seconds), None) => CaptureTermination::DurationSeconds(seconds),
        (None, Some(ticks)) => CaptureTermination::Ticks(ticks),
        _ => {
            return Err(AppError::InvalidCapture(
                "record requires exactly one of --duration or --ticks".to_owned(),
            ));
        }
    };

    let sources = PipeWireBackend.enumerate()?;
    let selected = select_source(&sources, &source)?;
    let request = CaptureRequest::new(selected.clone(), termination)?;
    println!("Backend: PipeWire");
    println!("Source: {}", selected.display_name);
    println!("Timestamping: PipeWire graph position + event offset");
    println!();
    println!("Recording...");
    let capture = PipeWireBackend.record(request)?;
    capture.write_to_file(&output)?;

    let clocks = capture
        .events
        .iter()
        .filter(|event| event.event == MidiEvent::Clock)
        .count();
    let timing = timing_summary(&capture);
    println!(
        "Graph: {} / quantum {}",
        timing
            .rate_hz
            .map(|rate| format!("{rate:.2} Hz"))
            .unwrap_or_else(|| "unknown".to_owned()),
        timing
            .quantum
            .map(|quantum| quantum.to_string())
            .unwrap_or_else(|| "unknown".to_owned())
    );
    println!();
    println!("Captured: {clocks} MIDI clocks");
    println!("Duration: {:.2} s", duration_s(&capture));
    println!();
    println!("Saved: {}", output.display());
    Ok(())
}

fn run_analyze(capture: PathBuf, json: bool, csv: Option<PathBuf>) -> Result<(), AppError> {
    let capture_file = CaptureFile::read_from_file(&capture)?;
    if !capture_file.transitions.is_empty() {
        let clocks = capture_file
            .events
            .iter()
            .filter(|event| event.event == MidiEvent::Clock)
            .count();
        for warning in output::warnings(&capture_file, clocks) {
            eprintln!("{warning}");
        }
        return Err(AppError::GraphRateTransitionUnsupported);
    }

    let analysis = analyze(&capture_file, AnalysisOptions::default())?;
    if json {
        println!(
            "{}",
            output::json::format_json_report(&capture_file, &analysis)?
        );
    } else {
        print!("{}", output::text::format_report(&capture_file, &analysis));
    }
    if let Some(csv_path) = csv {
        output::csv::write_csv_report(&capture_file, &analysis, &csv_path)?;
    }
    Ok(())
}
