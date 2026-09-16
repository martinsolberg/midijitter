# Cadence-Based Startup Analysis Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish the live MIDI Clock anchor with configurable cadence confirmation, then expose one authoritative event/interval model to fitting, statistics, reports, plots, JSON, and CSV.

**Architecture:** Refactor analysis into startup detection followed by live-stream classification. Startup detection uses a robust initial period estimate, an optional settle lower bound, and `N` plausible one-tick intervals; phase two assigns tick zero to the first event of the confirmed run. `EventDisposition` describes events and `IntervalDisposition` describes intervals independently, so missing gaps do not consume event disposition and every output path uses the same predicates.

**Tech Stack:** Rust, Clap, Serde/serde_json, existing analysis and plot modules, integration tests under `tests/`.

## Global Constraints

- Default PipeWire/ALSA-seq startup cadence is `8`; non-PipeWire backends remain disabled by default.
- `--startup-cadence 0` disables cadence filtering for diagnostics.
- `--settle` is an additional lower-bound constraint; it does not replace cadence detection.
- Startup-transient events are excluded from regression, phase statistics, period statistics, rolling statistics, and plots.
- Clean phase metrics require `EventDisposition::Valid`.
- Clean period metrics require valid adjacent events, inferred step `1`, and `IntervalDisposition::Normal`.
- Anomalies are excluded from regression and clean metrics but remain visible in rows and receive a dedicated summary.
- Do not add an all-steady-state-including-anomalies metric block.
- Do not claim ALSA-seq bridge provenance unless it is observable from capture data.

---

### Task 1: Add authoritative dispositions and two-phase startup classification

**Files:**
- Modify: `src/analysis/indexing.rs`
- Modify: `src/analysis/mod.rs`
- Modify: `src/analysis/fit.rs`
- Modify: `src/analysis/jitter.rs`
- Test: `tests/analysis.rs`

**Interfaces:**
- Produces `EventDisposition`, `IntervalDisposition`, and row fields `disposition`, `interval_disposition`, and `missing_before`.
- Produces startup metadata: transient count, zero/negative backlog count, live anchor sequence/timestamp, cadence confirmation count, and startup period estimate.
- `AnalysisOptions` gains `startup_cadence: usize`; defaults are selected from capture backend, with explicit `0` disabling startup filtering.

- [ ] **Step 1: Write failing tests for live-anchor selection.**

Add synthetic captures where duplicate timestamps precede a regular stream. Assert that with cadence `8`:

```rust
assert_eq!(result.rows[0].disposition, EventDisposition::StartupTransient);
assert_eq!(result.live_anchor_sequence, Some(8));
assert_eq!(result.rows.iter().find(|row| row.sequence == 8).unwrap().tick_index, 0);
assert_eq!(result.rows.iter().filter(|row| row.disposition == EventDisposition::StartupTransient).count(), 8);
```

Also add tests that `startup_cadence: 0` retains the diagnostic anchor behavior and that a settle boundary rejects a candidate run beginning before `capture_start + settle_ns`.

- [ ] **Step 2: Run focused tests and verify failure.**

Run:

```bash
cargo test --test analysis startup -- --nocapture
```

Expected: compilation or assertion failures because the new types and anchor fields do not exist.

- [ ] **Step 3: Implement the disposition and interval types.**

In `src/analysis/indexing.rs`, define serializable public enums:

```rust
pub enum EventDisposition { Valid, StartupTransient, Duplicate, Anomalous }
pub enum IntervalDisposition { Normal, Missing { count: u32 }, Anomalous }
```

Replace independent classification booleans with these fields. Keep missing information attached to the current row as `missing_before`, while retaining the interval disposition separately.

- [ ] **Step 4: Implement phase-one startup detection.**

Add a helper with an explicit contract:

```rust
fn find_live_anchor(
    events: &[&CapturedEvent],
    period_ns: f64,
    tolerance: f64,
    startup_cadence: usize,
    settle_ns: i128,
) -> Option<usize>
```

For cadence `N > 0`, scan candidate first-event indices at or after the settle boundary and require `N` consecutive positive intervals satisfying the relative one-tick tolerance. Return the candidate index, not the final event index. For cadence `0`, return the diagnostic fallback anchor index `0`.

- [ ] **Step 5: Rebase phase-two classification at the anchor.**

Mark rows before the anchor as `StartupTransient`. Start classification at the anchor, assign its tick index `0`, and classify later rows without special-casing raw `events[0]`. Preserve the existing iterative fit/refit behavior for the live rows.

- [ ] **Step 6: Implement clean population predicates and metadata.**

Centralize predicates used by all consumers:

```rust
pub fn is_clean_phase(row: &AnalysisRow) -> bool;
pub fn is_clean_period(previous: &AnalysisRow, current: &AnalysisRow) -> bool;
```

`is_clean_phase` requires `Valid`; `is_clean_period` additionally requires adjacent tick step `1` and `IntervalDisposition::Normal`. Calculate startup backlog from transient rows whose preceding raw interval is zero/negative. Calculate anomaly summaries from post-startup anomalous rows.

- [ ] **Step 7: Run focused tests and commit.**

Run:

```bash
cargo test --test analysis startup -- --nocapture
cargo test --test analysis duplicate -- --nocapture
cargo test --test analysis anomaly -- --nocapture
```

Commit:

```bash
git add src/analysis tests/analysis.rs
git commit -m "refactor: classify startup and interval dispositions"
```

### Task 2: Make statistics and rolling analysis consume shared populations

**Files:**
- Modify: `src/analysis/jitter.rs`
- Modify: `src/analysis/rolling.rs`
- Modify: `src/plot/phase.rs`
- Modify: `src/plot/period.rs`
- Modify: `src/plot/histogram.rs`
- Test: `tests/analysis.rs`
- Test: `tests/rolling.rs` if present, otherwise add the focused cases to `tests/analysis.rs`

**Interfaces:**
- Consumes `AnalysisRow`, `EventDisposition`, `IntervalDisposition`, `is_clean_phase`, and `is_clean_period` from Task 1.
- Produces clean phase/period metrics and rolling points using exactly those predicates.

- [ ] **Step 1: Write failing tests for adjacent period exclusion.**

Construct a valid-anomaly-valid sequence and assert that neither interval adjacent to the anomalous event appears in clean period statistics. Construct a valid event after a missing gap and assert that it remains eligible for phase statistics while `missing_before > 0`.

- [ ] **Step 2: Run focused tests and verify failure.**

```bash
cargo test --test analysis clean_period -- --nocapture
```

Expected: failure because current period filtering still uses independent booleans and does not model interval disposition.

- [ ] **Step 3: Update phase and period calculations.**

Use `is_clean_phase` for phase errors. Use `is_clean_period` for intervals and period errors. Ensure startup metadata and anomaly metadata are calculated without changing the clean metric population.

- [ ] **Step 4: Update rolling analysis.**

Replace the rolling filter with the shared clean-phase/clean-period predicates. Do not duplicate checks for startup, duplicate, or anomaly fields in `rolling.rs`.

- [ ] **Step 5: Update plots to consume analyzed populations.**

Replace filters in all three plot modules with shared predicates. Plot row inclusion must match text and numerical metrics exactly; no plot module may inspect raw disposition fields independently.

- [ ] **Step 6: Run tests, clippy, formatting, and commit.**

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
git add src/analysis src/plot tests
git commit -m "refactor: share clean populations across analysis and plots"
```

### Task 3: Add anomaly and startup reporting to text, JSON, and CSV

**Files:**
- Modify: `src/output/text.rs`
- Modify: `src/output/json.rs`
- Modify: `src/output/csv.rs`
- Modify: `src/analysis/jitter.rs`
- Test: `tests/output.rs` or existing output tests

**Interfaces:**
- Consumes authoritative rows and metadata from Tasks 1 and 2.
- Produces neutral startup terminology and anomaly summaries without inferred provenance claims.

- [ ] **Step 1: Write failing output assertions.**

Assert text contains sections and labels for `Startup`, `Transient events`, `Zero/duplicate backlog`, `Live anchor`, `Cadence confirmation`, `Startup period estimate`, `Steady-state jitter`, and `Anomalies`. Assert JSON includes disposition, interval disposition, `missing_before`, startup metadata, and anomaly details. Assert CSV includes the same per-row classification fields.

- [ ] **Step 2: Run output tests and verify failure.**

```bash
cargo test --test output -- --nocapture
```

Expected: missing-field or missing-label failures.

- [ ] **Step 3: Implement text reporting.**

Render startup counts and anchor metadata using observed terms only. Report clean steady-state metrics from the shared populations. Report anomaly count and worst phase/interval residual with sequence/timestamp context. Use `n/a` for unavailable summaries rather than fabricating values.

- [ ] **Step 4: Implement JSON and CSV fields.**

Serialize enum values consistently. Include each row's `disposition`, `interval_disposition`, `missing_before`, and phase error. Include startup metadata and anomaly summaries at report level. Preserve every analyzed event in CSV.

- [ ] **Step 5: Run verification and commit.**

```bash
cargo test --test output
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
git add src/output src/analysis/jitter.rs tests
git commit -m "feat: report startup and anomaly populations"
```

### Task 4: Add CLI defaults and update documentation

**Files:**
- Modify: `src/cli.rs`
- Modify: `README.md`
- Test: `tests/cli.rs`

**Interfaces:**
- `analyze` accepts `--startup-cadence <N>` with default `8` for PipeWire/ALSA-seq captures.
- `--startup-cadence 0` disables cadence filtering.
- Existing `--settle <duration>` remains available and acts as the lower-bound constraint.

- [ ] **Step 1: Write failing CLI tests.**

Test help output for both flags, parse `--startup-cadence 0`, reject invalid values, and verify a PipeWire capture uses cadence `8` when the flag is omitted while a non-PipeWire capture defaults to `0`.

- [ ] **Step 2: Run CLI tests and verify failure.**

```bash
cargo test --test cli startup_cadence -- --nocapture
```

- [ ] **Step 3: Implement CLI parsing and backend-sensitive defaults.**

Parse a nonnegative integer. Store an explicit user value separately from the default so the analyzer can choose `8` only for PipeWire/ALSA-seq captures. Pass both cadence and settle into `AnalysisOptions`.

- [ ] **Step 4: Update README usage and interpretation.**

Document the normal command:

```bash
midijitter analyze capture.json
```

Explain that PipeWire/ALSA-seq analysis waits for eight plausible one-tick intervals, that `--startup-cadence 0` is diagnostic mode, and that `--settle 1s` adds a lower bound. Update the interpretation section to distinguish startup artifacts, clean steady-state jitter, and genuine anomalies.

- [ ] **Step 5: Run CLI tests and commit.**

```bash
cargo test --test cli
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
git add src/cli.rs README.md tests/cli.rs
git commit -m "feat: make cadence startup filtering the PipeWire default"
```

### Task 5: Validate the real capture and complete integration checks

**Files:**
- Modify: `README.md` only if validation reveals inaccurate output wording
- Test: `capture.json` as a local untracked validation fixture; do not commit unless explicitly requested

**Interfaces:**
- Validates the complete pipeline from capture file through text, JSON, CSV, rolling analysis, and plots.

- [ ] **Step 1: Run the complete automated gate.**

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Expected: all tests pass, clippy is clean, and formatting has no diff.

- [ ] **Step 2: Run the default real-capture analysis.**

```bash
cargo run --release -- analyze capture.json
```

Verify the report shows cadence confirmation, a live anchor after the startup burst, approximately 0.1–0.2 ms clean phase error, and no stale 3.1 ms event in clean steady-state extrema.

- [ ] **Step 3: Run diagnostic and constrained modes.**

```bash
cargo run --release -- analyze capture.json --startup-cadence 0
cargo run --release -- analyze capture.json --settle 1s
cargo run --release -- analyze capture.json --startup-cadence 8 --settle 1s --json --csv /tmp/midijitter.csv
```

Verify diagnostic mode preserves startup rows, settle acts as a lower bound, JSON and CSV contain identical disposition decisions, and CSV retains all received clock rows.

- [ ] **Step 4: Validate plots and rolling output.**

```bash
cargo run --release -- plot capture.json --output-dir /tmp/midijitter-plots
cargo run --release -- rolling capture.json --window 5
```

Verify plots and rolling points use the same clean populations as the report and do not include startup transients or anomaly-adjacent periods.

- [ ] **Step 5: Review worktree and commit any final documentation correction.**

```bash
git status --short
git diff
```

Do not add `capture.json` or unrelated generated files. If README wording needs correction, commit only that correction:

```bash
git add README.md
git commit -m "docs: clarify cadence startup validation"
```
