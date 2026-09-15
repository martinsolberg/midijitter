# MIDI Jitter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a CLI-only Rust application that records MIDI Clock through native PipeWire graph timing, analyzes clock jitter offline, and exports reports and plots.

**Architecture:** PipeWire capture, persisted capture data, backend-independent analysis, and presentation are separate modules. The PipeWire realtime callback only parses MIDI bytes and writes preallocated event records; persistence, fitting, reporting, CSV export, and plotting occur after capture. v0.1 deliberately rejects captures that contain a graph-rate transition after preserving its metadata.

**Tech Stack:** Rust current stable (via rustup on Debian 13), `pipewire` 0.10.x, `libspa` 0.10.x, `clap` 4.x, `serde`, `serde_json`, `csv`, `thiserror`, `plotters` 0.3.x, `alsa`, and `libc`.

## Global Constraints

- Target Debian GNU/Linux 13 "Trixie" on x86-64, PipeWire, and WirePlumber.
- Use the native PipeWire API as the default backend; do not use JACK APIs or compatibility libraries.
- Derive PipeWire timing from graph cycle position plus the SPA event offset; never use callback execution time as event time.
- Preserve raw graph positions, rational rates, event offsets, quantums, and timing transitions in a versioned JSON capture.
- Normalize stored event time relative to the first captured event using integer nanoseconds; use `f64` only for analysis.
- Do not allocate, lock shared mutexes, perform I/O, log, sleep, serialize, or analyze in the PipeWire process callback.
- Analyze only MIDI Timing Clock (`0xF8`) events; record Start, Continue, and Stop counts separately.
- Preserve anomalies in captures. If excluded from derived fitting or statistics, report their counts and exclusion policy.
- Require exactly one of `--duration` or `--ticks` for `record`; Ctrl-C is an early termination of a bounded capture.
- v0.1 is CLI-only and includes PipeWire `devices`, `record`, `analyze`, and `plot`. ALSA, compare, simulate, and advanced metrics are later stages.

---

## Planned File Structure

- `Cargo.toml`: package metadata, binary/library targets, and dependency versions.
- `src/main.rs`: process entry point and mapped application exit codes.
- `src/lib.rs`: public module declarations.
- `src/cli.rs`: Clap command types and dispatch.
- `src/error.rs`: typed user-facing errors and exit-code mapping.
- `src/backend/mod.rs`: backend-independent source and capture contracts.
- `src/backend/pipewire/mod.rs`: PipeWire lifecycle and `PipeWireBackend`.
- `src/backend/pipewire/enumerate.rs`: PipeWire MIDI source discovery and selection.
- `src/backend/pipewire/capture.rs`: graph connection and realtime record collection.
- `src/backend/pipewire/timing.rs`: checked graph-position timestamp arithmetic.
- `src/capture/event.rs`: MIDI, raw timing, and captured event types.
- `src/capture/parser.rs`: stateful MIDI byte-stream parser that extracts realtime messages anywhere in a stream.
- `src/capture/metadata.rs`: versioned common/PipeWire metadata and graph transition records.
- `src/capture/format.rs`: JSON capture read, write, and validation.
- `src/analysis/indexing.rs`: robust tick indexing and anomaly classification.
- `src/analysis/fit.rs`: linear clock fit and BPM conversion.
- `src/analysis/jitter.rs`: phase and period sample generation.
- `src/analysis/statistics.rs`: summary, quantiles, and percentile calculations.
- `src/analysis/mod.rs`: analysis orchestration and result types.
- `src/output/{mod,text,json,csv}.rs`: terminal, JSON, and CSV output.
- `src/plot/{mod,phase,period,histogram}.rs`: offline PNG renderers.
- `tests/fixtures/*.json`: deterministic valid captures used by CLI, output, and plot tests.
- `tests/analysis.rs`, `tests/capture_format.rs`, `tests/pipewire_timing.rs`, `tests/cli.rs`: integration-level behavior tests.

---

## Stage 1: PipeWire v0.1

### Task 1: Bootstrap the package and capture model

**Files:**
- Create: `Cargo.toml`, `src/main.rs`, `src/lib.rs`, `src/error.rs`, `src/capture/mod.rs`, `src/capture/event.rs`, `src/capture/metadata.rs`, `src/capture/format.rs`, `tests/capture_format.rs`

**Interfaces:**
- Produces `CaptureFile`, `CapturedEvent`, `MidiEvent`, `TimestampMetadata`, `PipeWireTimestamp`, and `AppError` used by every later task.

- [ ] **Step 1: Add the failing JSON round-trip test**

```rust
#[test]
fn capture_round_trip_preserves_pipewire_graph_timestamp() {
    let capture = fixture_capture();
    let json = serde_json::to_string(&capture).unwrap();
    assert_eq!(serde_json::from_str::<CaptureFile>(&json).unwrap(), capture);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test capture_format`

Expected: compilation failure because `CaptureFile` is undefined.

- [ ] **Step 3: Define the stable capture types and JSON schema**

```rust
pub struct CapturedEvent {
    pub sequence: u64,
    pub timestamp_ns: i128,
    pub event: MidiEvent,
    pub timestamp_metadata: TimestampMetadata,
}

pub struct PipeWireTimestamp {
    pub cycle_position: i64,
    pub event_offset: u32,
    pub event_position: i64,
    pub rate_num: u32,
    pub rate_denom: u32,
    pub quantum: u32,
}
```

Include `format_version: 1`, backend, source, timestamp method, PPQN `24`, application version, environment metadata, transitions, and events. Derive `Serialize`, `Deserialize`, `Debug`, `Clone`, and `PartialEq` for persisted types.

- [ ] **Step 4: Validate format version and structural invariants**

Reject format versions other than `1`, empty source identities, nonpositive rational denominators, and non-monotonic event sequence numbers with `AppError::InvalidCapture` or `AppError::UnsupportedCaptureFormat`.

- [ ] **Step 5: Run verification**

Run: `cargo fmt --check && cargo test --test capture_format`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml src tests/capture_format.rs
git commit -m "feat: add versioned capture model"
```

### Task 2: Add a shared MIDI byte-stream parser

**Files:**
- Create: `src/capture/parser.rs`, `tests/midi_parser.rs`
- Modify: `src/capture/mod.rs`, `src/capture/event.rs`

**Interfaces:**
- Produces `MidiParser::push(&mut self, byte: u8) -> Option<MidiEvent>`.
- Consumed by PipeWire capture in Task 6 and ALSA capture in Task 9.

- [ ] **Step 1: Write parser tests for realtime interleaving**

```rust
#[test]
fn clock_is_emitted_inside_a_channel_message() {
    let mut parser = MidiParser::default();
    assert_eq!(parser.push(0x90), None);
    assert_eq!(parser.push(0x3c), None);
    assert_eq!(parser.push(0xf8), Some(MidiEvent::Clock));
    assert_eq!(parser.push(0x7f), None);
}
```

Add matching tests for `FA`, `FB`, `FC`, and optional `FE` interleaved with data bytes.

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test --test midi_parser`

Expected: compilation failure because `MidiParser` is undefined.

- [ ] **Step 3: Implement realtime-safe parsing**

Treat bytes `0xF8..=0xFF` as immediate realtime messages without disturbing channel-message state. Return only the recognized event and leave non-realtime message decoding out of the capture path.

- [ ] **Step 4: Run verification and commit**

Run: `cargo fmt --check && cargo test --test midi_parser`

```bash
git add src/capture tests/midi_parser.rs
git commit -m "feat: parse interleaved MIDI realtime events"
```

### Task 3: Implement backend-independent clock analysis

**Files:**
- Create: `src/analysis/{mod,indexing,fit,jitter,statistics}.rs`, `tests/analysis.rs`, `tests/fixtures/perfect-120.json`, `tests/fixtures/perfect-119.json`, `tests/fixtures/missing.json`, `tests/fixtures/duplicate.json`, `tests/fixtures/outlier.json`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes `&CaptureFile`.
- Produces `analyze(capture: &CaptureFile, options: AnalysisOptions) -> Result<AnalysisResult, AppError>`.

- [ ] **Step 1: Write deterministic fixture tests**

```rust
#[test]
fn perfect_119_bpm_is_not_reported_as_jitter() {
    let result = analyze(&load_fixture("perfect-119.json"), AnalysisOptions::default()).unwrap();
    assert!((result.measured_bpm - 119.0).abs() < 0.001);
    assert!(result.phase.rms_ns < 1_000.0);
    assert!(result.period.rms_error_ns < 1_000.0);
}
```

Add fixtures for Gaussian jitter using fixed samples, alternating `+/-1 ms` phase deviation, slow drift, one missing tick, one duplicate tick, and one 10 ms outlier.

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test --test analysis`

Expected: compilation failure because `analyze` is undefined.

- [ ] **Step 3: Implement robust indexing and fitting**

Use Clock events only. Calculate an initial median adjacent period, classify `step = round(interval / period)`, infer missing ticks only inside a configurable integer-multiple tolerance, refit by least-squares, and repeat at most 10 passes. Return `AppError::AnalysisDidNotConverge` if classifications do not stabilize.

Keep anomalous and duplicate events in analysis rows but exclude them from regression, phase statistics, and normal one-tick period statistics. Expose every exclusion count in `AnalysisResult`.

- [ ] **Step 4: Implement all required metrics**

Report phase mean, standard deviation, RMS, mean absolute, median absolute, P95 absolute, P99 absolute, minimum, maximum, and peak-to-peak. Report period mean interval, standard deviation, RMS error, minimum/maximum interval, P95 absolute error, and P99 absolute error.

- [ ] **Step 5: Run verification and commit**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --test analysis`

```bash
git add src/analysis tests/analysis.rs tests/fixtures
git commit -m "feat: add backend-independent jitter analysis"
```

### Task 4: Implement PipeWire graph-time arithmetic

**Files:**
- Create: `src/backend/mod.rs`, `src/backend/pipewire/timing.rs`, `tests/pipewire_timing.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Produces `pipewire_event_position(cycle_position: i64, offset: u32) -> Result<i64, AppError>` and `relative_ns(position: i64, first_position: i64, rate_num: u32, rate_denom: u32) -> Result<i128, AppError>`.

- [ ] **Step 1: Write graph-time conversion tests**

```rust
#[test]
fn canonical_graph_position_conversion() {
    assert_eq!(pipewire_event_position(480_000, 240).unwrap(), 480_240);
    assert_eq!(relative_ns(480_240, 0, 1, 48_000).unwrap(), 10_005_000_000);
}
```

Add tests for 44.1, 48, and 96 kHz; quantums 128, 256, 512, 1024, and 2048; overflow rejection; and equal timing across different quantum segmentation.

- [ ] **Step 2: Add rate-transition capture tests**

Create a synthetic capture containing a stored rate/quantum transition. Assert that v0.1 analysis returns `AppError::GraphRateTransitionUnsupported` and that text output warns about the transition.

- [ ] **Step 3: Implement checked integer arithmetic**

Use `i128` intermediates, preserve raw rational rate fields, and never hard-code 48 kHz. Establish the normalized epoch as the first captured event's graph position; retain raw absolute positions in every event.

- [ ] **Step 4: Run verification and commit**

Run: `cargo fmt --check && cargo test --test pipewire_timing`

```bash
git add src/backend src/capture tests/pipewire_timing.rs
git commit -m "feat: add PipeWire graph time conversion"
```

### Task 5: Implement PipeWire source enumeration

**Files:**
- Create: `src/backend/pipewire/{mod,enumerate}.rs`
- Modify: `src/backend/mod.rs`, `src/cli.rs`, `src/error.rs`, `src/main.rs`

**Interfaces:**
- Produces `CaptureBackend::enumerate(&self) -> Result<Vec<MidiSource>, AppError>`.
- `MidiSource` contains display name, stable PipeWire properties (`node.name`, `port.name`, `object.serial` when present), plus numeric node and port IDs for metadata.

- [ ] **Step 1: Write CLI failure tests**

Test unavailable PipeWire and duplicate displayed names. A duplicate source selector must fail and print candidate stable identities rather than silently selecting an arbitrary port.

- [ ] **Step 2: Implement native discovery**

Initialize PipeWire, query MIDI-capable source ports, and implement `midijitter devices` and `midijitter devices --backend pipewire`. Do not link or use JACK compatibility APIs.

- [ ] **Step 3: Add explicit record-time selection errors**

Return actionable errors for daemon unavailable, no source, ambiguous source, permission denied, and source not found. Map them to nonzero exit codes in `main.rs` without panicking.

- [ ] **Step 4: Verify and commit**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --test cli`

Manually run: `cargo run -- devices --backend pipewire`

```bash
git add src/backend src/cli.rs src/error.rs src/main.rs tests/cli.rs
git commit -m "feat: enumerate PipeWire MIDI sources"
```

### Task 6: Implement realtime-safe PipeWire capture

**Files:**
- Create: `src/backend/pipewire/capture.rs`
- Modify: `src/backend/pipewire/mod.rs`, `src/backend/mod.rs`, `src/capture/metadata.rs`, `src/error.rs`

**Interfaces:**
- Produces `CaptureBackend::record(&self, request: CaptureRequest) -> Result<CaptureFile, AppError>`.
- `CaptureRequest` contains the selected source and exactly one bounded termination condition.

- [ ] **Step 1: Add a testable event-recording seam**

Create a non-PipeWire unit-testable function that receives cycle position, event offset, rate, quantum, and bytes, then appends `CapturedEvent` values into a caller-provided fixed-capacity buffer.

- [ ] **Step 2: Implement graph capture and SPA MIDI processing**

Connect to the selected source through native PipeWire. In its process callback, derive `event_position = cycle_position + event_offset`, feed each control-event byte through `MidiParser`, and append recognized events with raw and normalized timing fields.

- [ ] **Step 3: Enforce callback safety**

Preallocate capacity from the required duration or tick count and a documented safety margin. On capacity exhaustion, atomically mark capture overflow and stop outside the callback. Do not grow a `Vec`, write a file, print, sleep, query configuration, or lock a cross-thread mutex in the callback.

- [ ] **Step 4: Capture timing transitions and failures**

Store rate/quantum changes as metadata transitions. Return clear errors for negotiation failure, unsupported MIDI/control format, source disappearance, and overflow. Never timestamp from `Instant::now()` or `clock_gettime()` in PipeWire mode.

- [ ] **Step 5: Verify timing semantics on hardware**

Capture a short hardware run and inspect JSON outside the callback. Confirm two MIDI events in one graph cycle have distinct `event_offset` and `event_position` values.

- [ ] **Step 6: Run verification and commit**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`

```bash
git add src/backend/pipewire src/backend/mod.rs src/capture src/error.rs
git commit -m "feat: capture MIDI through PipeWire graph timing"
```

### Task 7: Complete recording and analysis commands

**Files:**
- Create: `src/output/{mod,text,json,csv}.rs`, `tests/cli.rs`, `tests/fixtures/transition.json`
- Modify: `src/cli.rs`, `src/main.rs`, `src/capture/format.rs`, `src/analysis/mod.rs`

**Interfaces:**
- Consumes `CaptureFile` and `AnalysisResult`.
- Produces `midijitter record`, `midijitter analyze`, JSON reports, and analyzed CSV output.

- [ ] **Step 1: Implement bounded `record`**

Support `record --source <source> --duration <seconds> --output <file>` and `record --source <source> --ticks <count> --output <file>`. Reject no termination argument or both termination arguments. On Ctrl-C, stop cleanly, persist valid collected data, and print:

```text
Backend: PipeWire
Source: <source>
Timestamping: PipeWire graph position + event offset
Graph: <rate> Hz / quantum <quantum>
Captured: <count> MIDI clocks
Duration: <seconds> s
Saved: <path>
```

- [ ] **Step 2: Implement `analyze` and machine-readable exports**

Support `analyze <capture.json>`, `--json`, and `--csv <file>`. Report backend, source, timestamp method, PPQN, event and transport counts, duration, PipeWire version, sample rate, quantum, rate/quantum transition counts, fit results, all Task 3 metrics, missing/duplicate/anomalous/excluded counts, and a prominent transition warning.

Use exact CSV headers:

```text
event,tick_index,time_s,interval_ms,ideal_time_s,phase_error_ms,period_error_ms,backend,cycle_position,event_offset,event_position
```

- [ ] **Step 3: Add command and error-path tests**

Test malformed JSON, unknown format version, no F8 events, too few F8 events, a graph-transition capture, missing output path, and CSV fields for PipeWire and non-PipeWire rows.

- [ ] **Step 4: Run verification and commit**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`

```bash
git add src/cli.rs src/main.rs src/output src/capture src/analysis tests
git commit -m "feat: add recording analysis and CSV output"
```

### Task 8: Generate v0.1 plots and perform acceptance testing

**Files:**
- Create: `src/plot/{mod,phase,period,histogram}.rs`, `tests/plot.rs`
- Modify: `src/cli.rs`, `src/lib.rs`

**Interfaces:**
- Produces `render_plots(capture: &CaptureFile, analysis: &AnalysisResult, output_dir: &Path) -> Result<(), AppError>`.

- [ ] **Step 1: Add plot tests**

Test that a fixture capture produces nonempty `phase.png`, `period.png`, and `phase-histogram.png` in a temporary directory.

- [ ] **Step 2: Implement plots**

Render phase error in milliseconds over capture time with a zero reference, F8 interval in milliseconds over time with fitted-period reference, and a phase-error histogram. Generate PNG output; SVG remains optional.

- [ ] **Step 3: Add `plot` and run hardware acceptance**

Implement `plot <capture.json> --output-dir <directory>`. On Debian 13, run:

```bash
midijitter devices
midijitter record --source "Scarlett 18i20 MIDI" --duration 60 --output capture.json
midijitter analyze capture.json --csv capture.csv
midijitter plot capture.json --output-dir plots/
```

Inspect `capture.json` to verify raw graph positions and inspect all three plot files.

- [ ] **Step 4: Run verification and commit**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`

```bash
git add src/plot src/cli.rs src/lib.rs tests/plot.rs
git commit -m "feat: add offline jitter plots"
```

---

## Stage 2: ALSA RawMIDI Reference Backend

### Task 9: Add timestamped ALSA RawMIDI capture

**Files:**
- Create: `src/backend/alsa/{mod,enumerate,capture,timing}.rs`, `tests/alsa_parser.rs`
- Modify: `Cargo.toml`, `src/backend/mod.rs`, `src/cli.rs`, `src/capture/metadata.rs`, `src/error.rs`

**Interfaces:**
- Produces `AlsaRawBackend` implementing the same `CaptureBackend` contract as PipeWire.

- [ ] **Step 1: Add ALSA dependencies and tests**

Add `alsa` and `libc`. Test that the backend reuses `MidiParser` and serializes `TimestampMetadata::Alsa` with device, timestamp method, and clock type.

- [ ] **Step 2: Implement enumeration and timestamped reads**

Implement `devices --backend alsa-raw` with identifiers such as `hw:1,0,0`. Prefer timestamped RawMIDI reads using `SND_RAWMIDI_READ_TSTAMP`, `SND_RAWMIDI_CLOCK_MONOTONIC_RAW`, and `snd_rawmidi_tread()` or their binding equivalents.

- [ ] **Step 3: Implement explicit fallback behavior**

Only permit post-read `CLOCK_MONOTONIC_RAW` timestamps when timestamped RawMIDI is unavailable and the user explicitly accepts fallback. Persist `userspace-timestamped` in metadata and print a prominent warning; never use realtime/wall-clock timestamps.

- [ ] **Step 4: Cover ALSA errors and commit**

Return actionable no-panic errors for unavailable, busy, permission denied, device disconnect, and unsupported timestamp mode.

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`

```bash
git add Cargo.toml src/backend/alsa src/backend src/cli.rs src/capture src/error.rs tests
git commit -m "feat: add ALSA RawMIDI reference backend"
```

---

## Stage 3: Comparison, Simulation, and Advanced Metrics

### Task 10: Compare recorded captures safely

**Files:**
- Create: `src/output/compare.rs`, `tests/compare.rs`
- Modify: `src/cli.rs`, `src/output/mod.rs`

- [ ] **Step 1: Write comparison tests**

Test that `compare` re-runs analysis for each raw capture and warns on different backend, receiver/interface, PipeWire version, sample rate, quantum, or timestamp method.

- [ ] **Step 2: Implement comparison output**

Render backend, phase RMS/P99, period standard deviation, peak-to-peak, and missing ticks side by side. Always print that independent runs cannot isolate PipeWire's contribution because source variability remains.

- [ ] **Step 3: Verify and commit**

Run: `cargo fmt --check && cargo test --test compare`

```bash
git add src/output/compare.rs src/output/mod.rs src/cli.rs tests/compare.rs
git commit -m "feat: compare captured clock analyses"
```

### Task 11: Add deterministic simulation

**Files:**
- Create: `src/simulate.rs`, `tests/simulate.rs`
- Modify: `Cargo.toml`, `src/cli.rs`, `src/lib.rs`

- [ ] **Step 1: Write seeded simulation tests**

Test identical output from `simulate --bpm 120 --duration 60 --jitter-std 1ms --seed 42` and verify the resulting capture is accepted by `analyze`.

- [ ] **Step 2: Implement simulation**

Support BPM, duration, jitter standard deviation, periodic jitter, missing rate, duplicate rate, drift, and seed. Emit the same versioned JSON capture structure as hardware backends.

- [ ] **Step 3: Verify and commit**

Run: `cargo fmt --check && cargo test --test simulate`

```bash
git add Cargo.toml src/simulate.rs src/cli.rs src/lib.rs tests/simulate.rs
git commit -m "feat: add deterministic MIDI clock simulation"
```

### Task 12: Add advanced metrics as isolated features

**Files:**
- Create as needed: `src/analysis/rolling.rs`, `src/analysis/autocorrelation.rs`, `src/analysis/spectrum.rs`, `src/analysis/allan.rs`
- Modify: `src/analysis/mod.rs`, `src/cli.rs`, associated tests

- [ ] **Step 1: Add one metric with a synthetic interpretation test**

Start with rolling BPM or windowed phase RMS. Use a deterministic drift fixture and assert the metric visibly tracks the known change.

- [ ] **Step 2: Expose it as opt-in output**

Add a documented command or option without changing the concise v0.1 report.

- [ ] **Step 3: Verify and commit each metric independently**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`

```bash
git add src/analysis src/cli.rs tests
git commit -m "feat: add <metric-name> timing analysis"
```

---

## Final Verification Checklist

- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --all-targets -- -D warnings` passes.
- [ ] `cargo test` passes.
- [ ] Perfect 120 BPM and 119 BPM fixtures report RMS phase and period error below `1 us`.
- [ ] Missing, duplicate, outlier, alternating, Gaussian, and drift fixtures have expected classifications and metrics.
- [ ] Graph timestamp tests prove event offsets are retained and quantum changes do not quantize event time.
- [ ] Rate-transition fixtures are preserved and rejected with a clear diagnostic and report warning.
- [ ] Debian 13 hardware validation completes the `devices`, `record`, `analyze`, and `plot` workflow.
- [ ] A saved PipeWire capture contains raw graph position, event offset, event position, rational rate, quantum, and source metadata.
