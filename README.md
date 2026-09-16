# midijitter

This project is an experimental, vibe coded Linux CLI tool for measuring and analyzing
timing jitter in MIDI Clock streams. It captures MIDI 1.0 Timing Clock messages 
(`0xF8`, 24 PPQN) with kernel/graph timing, fits an ideal clock, and reports
phase and period jitter.

midijitter is an **engineering and benchmarking tool**, not a general MIDI
utility. It measures the timing stability of MIDI Clock events as presented at
a specific Linux measurement boundary — by default, the PipeWire application
boundary. 

Be advised that this tool has been developed without in-depth knowledge of neither 
MIDI protocol implementation nor Rust programming.

## What it measures

The default PipeWire backend answers: *how stable is this MIDI Clock stream as
presented to applications using PipeWire on Linux?*

MIDI events arrive as timed control events inside a PipeWire graph cycle.
midijitter timestamps each event from the **graph cycle position plus the
event's offset within the cycle** — never from when the callback happens to
execute, and never quantized to cycle boundaries. The optional ALSA RawMIDI
backend provides a lower-level reference point, using kernel timestamped reads
with `CLOCK_MONOTONIC_RAW`.

Both backends write a versioned JSON capture that preserves the raw timing
metadata, so captures can be re-analyzed later.

## Requirements

- Debian GNU/Linux 13 (or any distro with recent PipeWire/ALSA), x86-64
- PipeWire + WirePlumber (default backend)
- Rust (current stable, e.g. via rustup) and the C dev headers:

```bash
sudo apt install build-essential pkg-config libpipewire-0.3-dev libspa-0.2-dev libasound2-dev
```

## Build

```bash
cargo build --release
```

The binary is `target/release/midijitter`. No JACK, desktop, or DAW required.

## Usage

### List MIDI sources

```bash
midijitter devices                        # PipeWire sources (default)
midijitter devices --backend alsa-raw     # ALSA RawMIDI devices (hw:card,device,subdevice)
```

### Manual PipeWire capture

1. Find the source port id (ids shift between sessions, always re-check):

```bash
midijitter devices
```

2. Start the capture — it waits for you to create the link:

```bash
midijitter record \
  --source "Scarlett 18i20 USB MIDI 1 (capture)" \
  --duration 60 \
  --output capture.json \
  --manual-connect
```

3. In another terminal, link the source port to the exposed sink input
(e.g. source port `114`; use the id from step 1, not this example):

```bash
pw-link 114 $(pw-cli ls Port | awk '/^\tid /{id=$2; gsub(/,/,"",id)} /port.alias = "midijitter-capture:input_1"/{print id; exit}')
```

or connect them in a patchbay such as `qpwgraph`. The app prints
`Link active: recording started.` once the link streams. Recording ends on
its own after `--duration`, or cleanly on `Ctrl-C`.

### Analyze

```bash
midijitter analyze capture.json              # human-readable report
midijitter analyze capture.json --json       # machine-readable JSON
midijitter analyze capture.json --csv out.csv
```

The report includes measured BPM, fitted clock period, phase jitter (RMS, σ,
mean/median absolute, P95/P99, min/max, peak-to-peak), period jitter, and
counts of missing/duplicate/anomalous clocks. Startup classification and
anomaly details are also included in the text report, JSON, and CSV. Anomalous
events are preserved in the capture and excluded from clean statistics
explicitly. Captures with graph-rate or quantum transitions are preserved but
rejected from analysis (v0.1).

JSON and CSV retain every analyzed clock row. Each row records its event
disposition (`Valid`, `StartupTransient`, `Duplicate`, or `Anomalous`) and its
independent interval disposition (`Normal`, `Missing`, or `Anomalous`), plus
`missing_before`. A missing clock is an interval property; the first event
after a gap may still be a valid phase observation.

### Plot

```bash
midijitter plot capture.json --output-dir plots/
```

Creates `phase.png` (phase error over time, zero reference),
`period.png` (inter-clock interval with fitted-period reference), and
`phase-histogram.png`.

### Compare captures

```bash
midijitter compare capture.json alsa.json
```

Re-analyzes each capture and renders metrics side by side, warning when
backends/interfaces/rates/versions differ. Independent runs cannot isolate a
backend's contribution exactly — the source may vary between runs.

### Synthetic testing

```bash
midijitter simulate --bpm 120 --duration 60 --jitter-std 1ms --output synthetic.json
```

Generates a deterministic, versioned capture for regression testing. Supports
periodic jitter, missing/duplicate rates, linear drift (`10ppm/s`), and a
`--seed`.

### Advanced metrics

```bash
midijitter rolling capture.json --window 5    # rolling BPM over a 5 s window
```

## Startup detection

PipeWire/ALSA-seq analysis waits for eight consecutive plausible one-tick
intervals by default before selecting the live anchor. The first event of that
confirmed run receives tick zero; earlier events remain in the capture as
startup transients and are excluded from fit, statistics, rolling output, and
plots. This prevents the link-time stale backlog from becoming a phase
extreme. Other backends do not enable cadence filtering unless requested.

```bash
midijitter analyze capture.json
```

Tune or disable cadence detection when diagnosing a capture:

```bash
midijitter analyze capture.json --startup-cadence 16
midijitter analyze capture.json --startup-cadence 0
```

Use `--settle` as an additional lower bound when the link needs a known warm-up
period. It does not replace cadence confirmation:

```bash
midijitter analyze capture.json --settle 1s
```

`--settle` accepts durations like `500ms` or `1s`. The report uses neutral
terms such as `Zero/duplicate backlog`; it does not claim ALSA-seq bridge
provenance unless the capture provides that evidence.

The settle boundary is strict: a candidate cadence run must begin at or after
`capture_start + settle`. Once the run is confirmed, all events before its
first event are startup transients.

## Interpretation notes

- Constant latency is removed by the fitted clock intercept; variable latency
  is part of the observed jitter.
- A stable source at 119.98 BPM reports near-zero jitter — the measured tempo
  is fitted, never assumed.
- Missing/duplicate clocks and USB batching are part of the system under test
  and are preserved, not smoothed away. Startup transients are excluded from
  the benchmark population; post-startup anomalies remain visible and receive
  a separate count and worst-residual summary.
- Clean phase metrics use only `Valid` events. Clean period metrics require
  both adjacent events to be `Valid`, a one-tick step, and a `Normal` interval,
  so an anomaly cannot contaminate either neighboring period measurement.
- Prefer RMS/P99 over peak-to-peak as the stability headline: a single
  exceptional excursion (for example, an unfiltered startup flush) dominates
  peak-to-peak while telling you nothing about the clock population.

## Development

```bash
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

The capture backends live behind a `CaptureBackend` trait; analysis is fully
independent of PipeWire and ALSA, so offline analysis works on any capture.
