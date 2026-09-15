use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use crate::analysis::{AnalysisOptions, analyze};
use crate::backend::alsa::AlsaRawBackend;
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
        /// For `alsa-raw` this is the RawMIDI identifier (`hw:card,device,subdevice`).
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
        /// Permit explicitly labeled userspace timestamping when timestamped
        /// ALSA RawMIDI reads are unavailable. Only valid with `alsa-raw`.
        #[arg(long)]
        allow_userspace_timestamps: bool,
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
    /// Render diagnostic plots from a capture file.
    Plot {
        /// Capture file produced by `record`.
        capture: PathBuf,
        /// Directory receiving phase.png, period.png and phase-histogram.png.
        #[arg(long)]
        output_dir: PathBuf,
    },
    /// Compare jitter analyses side by side.
    Compare {
        /// Capture files produced by `record`. Each file is re-analyzed.
        #[arg(num_args = 1..)]
        captures: Vec<PathBuf>,
    },
    /// Print rolling tempo over a sliding window.
    Rolling {
        /// Capture file produced by `record` or `simulate`.
        capture: PathBuf,
        /// Window length in seconds.
        #[arg(long)]
        window: u64,
    },
    /// Generate a deterministic synthetic capture for testing.
    Simulate {
        /// Nominal tempo in BPM.
        #[arg(long, default_value_t = 120.0)]
        bpm: f64,
        /// Capture duration in seconds.
        #[arg(long, default_value_t = 60)]
        duration: u64,
        /// Gaussian jitter standard deviation (e.g. 1ms, 500us).
        #[arg(long, default_value = "0")]
        jitter_std: String,
        /// Periodic jitter amplitude (e.g. 1ms). Disabled with 0.
        #[arg(long, default_value = "0")]
        periodic_jitter: String,
        /// Periodic jitter frequency in Hz.
        #[arg(long, default_value_t = 1.0)]
        periodic_hz: f64,
        /// Probability per tick of dropping a clock (0 to 1).
        #[arg(long, default_value_t = 0.0)]
        missing_rate: f64,
        /// Probability per tick of emitting a duplicate clock (0 to 1).
        #[arg(long, default_value_t = 0.0)]
        duplicate_rate: f64,
        /// Linear tempo drift (e.g. 10ppm/s). Disabled with 0.
        #[arg(long, default_value = "0")]
        drift: String,
        /// Deterministic seed; identical configs produce identical files.
        #[arg(long, default_value_t = 42)]
        seed: u64,
        /// Destination capture file.
        #[arg(long)]
        output: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Backend {
    Pipewire,
    #[value(name = "alsa-raw")]
    AlsaRaw,
}

pub fn run() -> Result<(), AppError> {
    match Cli::parse().command {
        Command::Devices { backend } => run_devices(backend),
        Command::Record {
            backend,
            source,
            duration,
            ticks,
            output,
            allow_userspace_timestamps,
        } => run_record(
            backend,
            source,
            duration,
            ticks,
            output,
            allow_userspace_timestamps,
        ),
        Command::Analyze { capture, json, csv } => run_analyze(capture, json, csv),
        Command::Plot {
            capture,
            output_dir,
        } => run_plot(capture, output_dir),
        Command::Compare { captures } => run_compare(captures),
        Command::Rolling { capture, window } => run_rolling(capture, window),
        Command::Simulate {
            bpm,
            duration,
            jitter_std,
            periodic_jitter,
            periodic_hz,
            missing_rate,
            duplicate_rate,
            drift,
            seed,
            output,
        } => run_simulate(
            bpm,
            duration,
            jitter_std,
            periodic_jitter,
            periodic_hz,
            missing_rate,
            duplicate_rate,
            drift,
            seed,
            output,
        ),
    }
}

fn run_devices(backend: Backend) -> Result<(), AppError> {
    let sources = match backend {
        Backend::Pipewire => PipeWireBackend.enumerate()?,
        Backend::AlsaRaw => AlsaRawBackend.enumerate()?,
    };
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
    backend: Backend,
    source: String,
    duration: Option<u64>,
    ticks: Option<u64>,
    output: PathBuf,
    allow_userspace_timestamps: bool,
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
    if allow_userspace_timestamps && !matches!(backend, Backend::AlsaRaw) {
        return Err(AppError::InvalidCapture(
            "--allow-userspace-timestamps is only valid with --backend alsa-raw".to_owned(),
        ));
    }

    let recording: Box<dyn CaptureBackend> = match backend {
        Backend::Pipewire => Box::new(PipeWireBackend),
        Backend::AlsaRaw => Box::new(AlsaRawBackend),
    };
    let sources = recording.enumerate()?;
    let selected = select_source(&sources, &source)?;
    let request = CaptureRequest::new(selected.clone(), termination)?;
    let request = if allow_userspace_timestamps {
        request.allow_userspace_timestamps()
    } else {
        request
    };
    match backend {
        Backend::Pipewire => {
            println!("Backend: PipeWire");
            println!("Source: {}", selected.display_name);
            println!("Timestamping: PipeWire graph position + event offset");
        }
        Backend::AlsaRaw => {
            println!("Backend: ALSA RawMIDI");
            println!("Source: {}", selected.display_name);
            println!("Device: {}", selected.stable_identity());
            println!("Timestamping: ALSA timestamped RawMIDI (CLOCK_MONOTONIC_RAW)");
        }
    }
    println!();
    println!("Recording...");
    let capture = recording.record(request)?;
    capture.write_to_file(&output)?;

    if capture.timestamp_method.contains("userspace") {
        eprintln!(
            "Warning: capture used explicitly labeled userspace timestamps, not kernel \
             RawMIDI timestamps. Do not compare it as equivalent to kernel timestamping."
        );
    }
    let clocks = capture
        .events
        .iter()
        .filter(|event| event.event == MidiEvent::Clock)
        .count();
    let timing = timing_summary(&capture);
    if matches!(backend, Backend::Pipewire) {
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
    }
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

fn run_plot(capture: PathBuf, output_dir: PathBuf) -> Result<(), AppError> {
    let capture_file = CaptureFile::read_from_file(&capture)?;
    let analysis = analyze(&capture_file, AnalysisOptions::default())?;
    for created in crate::plot::render_plots(&capture_file, &analysis, &output_dir)? {
        println!("Created: {}", created.display());
    }
    Ok(())
}

fn run_compare(captures: Vec<PathBuf>) -> Result<(), AppError> {
    print!("{}", output::compare::format_comparison(&captures)?);
    Ok(())
}

fn run_rolling(capture: PathBuf, window_s: u64) -> Result<(), AppError> {
    let capture_file = CaptureFile::read_from_file(&capture)?;
    let analysis = analyze(&capture_file, AnalysisOptions::default())?;
    if window_s == 0 || !analysis.fitted_period_ns.is_finite() || analysis.fitted_period_ns <= 0.0 {
        return Err(AppError::InvalidCapture(
            "rolling window must be positive".to_owned(),
        ));
    }
    let window_ticks =
        ((window_s as f64 * 1_000_000_000.0 / analysis.fitted_period_ns).round() as usize).max(1);
    let points = crate::rolling_bpm(&analysis, window_ticks)?;
    println!("tick_index,time_s,rolling_bpm");
    for point in points {
        println!(
            "{},{:.6},{:.4}",
            point.tick_index, point.time_s, point.rolling_bpm
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_simulate(
    bpm: f64,
    duration: u64,
    jitter_std: String,
    periodic_jitter: String,
    periodic_hz: f64,
    missing_rate: f64,
    duplicate_rate: f64,
    drift: String,
    seed: u64,
    output: PathBuf,
) -> Result<(), AppError> {
    let capture = crate::simulate::generate(crate::simulate::SimulateConfig {
        bpm,
        duration_s: duration,
        jitter_std_ns: crate::simulate::parse_duration_ns(&jitter_std)?,
        periodic_jitter_ns: crate::simulate::parse_duration_ns(&periodic_jitter)?,
        periodic_hz,
        missing_rate,
        duplicate_rate,
        drift_per_second: crate::simulate::parse_drift_per_second(&drift)?,
        seed,
    })?;
    capture.write_to_file(&output)?;
    println!(
        "Simulated {} MIDI clocks at {bpm} BPM",
        capture.events.len()
    );
    println!("Saved: {}", output.display());
    Ok(())
}
