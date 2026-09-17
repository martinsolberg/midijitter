# Paired MIDI Clock Capture And Round-Trip Analysis Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement common-domain two-port PipeWire capture, v2 persistence, tick-aware paired analysis, and live/offline reporting.

**Architecture:** Preserve v1 single-stream types and add a dedicated `PairedCapture`/`PairedAnalysisResult` path. A single PipeWire filter process callback records both ports against one graph position and common origin. Offline analysis independently classifies each stream, aligns logical ticks, and computes latency only from structurally valid pairs.

**Tech Stack:** Rust 2024, PipeWire 0.10, serde/serde_json, clap, existing analysis and test infrastructure.

## Global Constraints

- Both streams must use one PipeWire graph-time domain and one common timestamp origin.
- Do not pair by raw event ordinal position.
- Preserve raw timing metadata and diagnostics; do not silently force ambiguous events into pairs.
- Existing v1 capture, commands, and tests must remain compatible.
- Use integer nanoseconds internally for latency.
- Pair CSV and plots are deferred from this implementation.

---

### Task 1: Paired Capture Model And Timing Primitives

**Files:** `src/capture/format.rs`, `src/capture/metadata.rs`, `src/capture/event.rs`, `src/capture/mod.rs`, `src/lib.rs`, `src/error.rs`, `tests/capture_format.rs`, new `tests/paired_timing.rs`.

- Add `PairedCapture`, `PairedEvent`, `StreamRole`, common graph metadata, typed graph transitions, completion metadata, and `CaptureDocument` dispatch while preserving v1 parsing.
- Add common-origin validation and `InconsistentCommonTimebase`-style errors.
- Add pure timestamp conversion tests including same-cycle and cross-cycle offsets.
- Add v2 round-trip and malformed-document tests.
- Run `cargo test --test capture_format --test paired_timing`, format, clippy, then commit.

### Task 2: Paired Analysis And Statistics

**Files:** new `src/analysis/paired.rs`, `src/analysis/mod.rs`, `src/lib.rs`, `src/output/text.rs`, `src/output/json.rs`, new `tests/paired_analysis.rs`.

- Define `PairStatus`, `PairedEvent`, `PairingResult`, `LatencyStatistics`, and `PairedAnalysisResult` with `PartialEq`.
- Analyze reference and returned streams through the existing single-stream analyzer.
- Align by logical tick indices, use transport anchors when available, and implement deterministic MAD lag selection with ambiguity rejection.
- Preserve structural anomalous/missing rows while restricting clean statistics to two valid rows.
- Add all synthetic tests listed in the design, including source-jitter cancellation and missing-event resynchronization.
- Add paired text/JSON reports and offline result equivalence tests.
- Run focused tests and commit.

### Task 3: Native Two-Port PipeWire Capture

**Files:** `src/backend/pipewire/common.rs`, `src/backend/pipewire/filter_capture.rs`, `src/backend/pipewire/mod.rs`, `src/backend/mod.rs`, new/updated PipeWire tests.

- Add a paired capture request and one filter with `reference` and `returned` MIDI input ports.
- Keep one `process(position)` callback; factor pure cycle ingestion beneath the FFI boundary.
- Record common graph metadata/transitions and event positions against the document origin.
- Preserve interrupted captures with explicit completion state and actionable source-disconnect errors.
- Add synthetic same-cycle/cross-cycle and transition tests plus an ignored live two-port smoke test.
- Run focused tests and commit.

### Task 4: CLI, Serialization Dispatch, And Documentation

**Files:** `src/cli.rs`, `src/capture/format.rs`, `src/output/mod.rs`, `README.md`, `tests/cli.rs`.

- Add `compare-live --reference --returned --duration --output` for PipeWire.
- Resolve and validate both sources, reject identical selection, capture/save/reload, then run the shared paired analyzer.
- Make `analyze` dispatch v1 versus v2 and emit paired text/JSON reports.
- Add actionable CLI errors and integration tests.
- Document topology, common graph-time behavior, and deferred output scope.
- Run full verification: `cargo test`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`.

### Final Verification

- Inspect the complete diff and confirm no unrelated files changed.
- Run the full verification commands again.
- Check immediate versus serialized/reloaded `PairedAnalysisResult` equality.
