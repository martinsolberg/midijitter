# Cadence Anchor Backlog Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent a duplicate-timestamp startup backlog event from being selected as the default live MIDI Clock anchor.

**Architecture:** Keep the existing cadence-based anchor scan and its tolerance unchanged. Add one prerequisite for candidates after the first captured event: the candidate's interval from the immediately preceding raw event must be strictly positive. This rejects the final event in a zero-timestamp backlog while allowing the next event, even when its first live interval is shorter than the fitted period.

**Tech Stack:** Rust, Cargo, existing `analysis::indexing` code, integration tests in `tests/analysis.rs`.

## Global Constraints

- Preserve startup rows and existing disposition/reporting behavior.
- Do not tighten the existing musical-period tolerance.
- Do not add speculative heuristics for startup patterns not present in the capture data.
- Keep the regression test and design note documenting that the rule may need revision for future backend startup patterns.
- Run `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check` before completion.

---

### Task 1: Reject backlog events as cadence anchors

**Files:**
- Modify: `src/analysis/indexing.rs`, `find_live_anchor`
- Test: `tests/analysis.rs`
- Verify: `octatrack_capture.json`

**Interfaces:**
- Consumes: existing `find_live_anchor(events, period_ns, tolerance, startup_cadence, settle_ns) -> Option<usize>` and existing cadence defaults.
- Produces: the first candidate after capture start whose preceding raw interval is positive and whose existing cadence confirmation succeeds.

- [ ] **Step 1: Add a failing regression test**

Add a capture with a duplicate-timestamp backlog, a shortened first live interval, and then a regular one-tick stream. Use the existing capture helper in `tests/analysis.rs`. The test must assert that the event at the end of the duplicate run is transient, the following event is the live anchor with tick index zero, and the clean minimum period is not the shortened backlog-to-live interval.

The essential test shape is:

```rust
#[test]
fn cadence_skips_duplicate_backlog_event_before_short_first_live_interval() {
    let mut timestamps = vec![0_i128; 4];
    timestamps.push(16_500_000);
    let mut time = 36_500_000_i128;
    for _ in 0..16 {
        timestamps.push(time);
        time += 20_000_000;
    }
    let analysis = analyze(
        &capture_with_clock_timestamps(&timestamps),
        AnalysisOptions {
            startup_cadence: Some(8),
            ..AnalysisOptions::default()
        },
    )
    .unwrap();

    let anchor = analysis
        .rows
        .iter()
        .find(|row| row.disposition == EventDisposition::Valid)
        .unwrap();
    assert_eq!(anchor.sequence, 5);
    assert_eq!(anchor.tick_index, 0);
    assert_eq!(
        analysis.rows[3].disposition,
        EventDisposition::StartupTransient
    );
    assert!(analysis.period.minimum_interval_ns > 19_000_000.0);
}
```

Use values that make the initial median period 20ms and provide at least eight plausible intervals after the intended anchor. Import `EventDisposition` if the test module does not already have it.

- [ ] **Step 2: Run the focused test and confirm the expected failure**

Run:

```bash
cargo test --test analysis cadence_skips_duplicate_backlog_event_before_short_first_live_interval -- --nocapture
```

Expected: FAIL because the current detector selects the event at the end of the duplicate run as the anchor, so the asserted sequence and transient disposition do not match.

- [ ] **Step 3: Implement the minimal anchor guard**

In `find_live_anchor`, retain the existing settle-boundary filter and cadence confirmation. Before accepting an indexed candidate after index zero, require its preceding raw interval to be positive:

```rust
let preceded_by_positive_interval = index == 0
    || events[index].timestamp_ns > events[index - 1].timestamp_ns;
if !preceded_by_positive_interval {
    return None;
}
```

Do not change the existing `candidate.windows(2)` cadence check, tolerance, anchor return value, or `settle_ns` handling. Structure the iterator so a rejected candidate continues scanning rather than terminating the search; use the condition inside the existing `find_map` and return `None` for that candidate.

- [ ] **Step 4: Run the focused test and confirm it passes**

Run the same focused test. Expected: PASS, with the duplicate-backlog event retained as `StartupTransient` and the next event selected as tick zero.

- [ ] **Step 5: Run the existing startup and analysis tests**

Run:

```bash
cargo test --test analysis startup -- --nocapture
cargo test --test analysis
```

Expected: all analysis tests pass. Confirm the explicit `startup_cadence: 0` diagnostic behavior remains unchanged.

- [ ] **Step 6: Validate the real capture**

Run:

```bash
cargo run --release -- analyze octatrack_capture.json
```

Expected: the live anchor moves after sequence 64, the shortened `16.521ms` interval is absent from `Minimum interval`, and phase peak-to-peak is approximately `0.45ms` rather than `3.539ms`. The capture remains unchanged.

- [ ] **Step 7: Run the full quality gate**

Run:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Expected: all commands pass with no warnings or formatting changes.

- [ ] **Step 8: Commit the implementation**

```bash
git add src/analysis/indexing.rs tests/analysis.rs
git commit -m "fix: skip duplicate backlog event as clock anchor"
```
