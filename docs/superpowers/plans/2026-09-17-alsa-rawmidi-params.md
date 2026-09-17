# ALSA RawMIDI Parameter Preservation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make ALSA RawMIDI capture preserve its opened input configuration while enabling verified kernel timestamped reads.

**Architecture:** `configure_clock` will begin from `Rawmidi::params_current()` instead of a blank `Params` allocation, then set and apply the timestamp mode and clock type. A small pure helper will validate the effective read mode and clock type after application, retaining the existing explicit userspace fallback for unsupported kernel timestamps.

**Tech Stack:** Rust 2024, `alsa` 0.12.1, ALSA RawMIDI API, `libc`

## Global Constraints

- Preserve ALSA's existing input-buffer and wakeup settings when configuring timestamps.
- Kernel timestamped reads must use `ReadMode::Timestamp` and `Clock::MonotonicRaw`.
- Userspace `CLOCK_MONOTONIC_RAW` timestamping remains opt-in via `--allow-userspace-timestamps`.
- Do not change the CLI, capture-file format, or analysis behavior.

---

### Task 1: Preserve And Verify ALSA RawMIDI Parameters

**Files:**
- Modify: `src/backend/alsa/capture.rs:232-273`
- Test: `src/backend/alsa/capture.rs:321-442`

**Interfaces:**
- Consumes: `Rawmidi::params_current() -> Result<alsa::rawmidi::Params, alsa::Error>` and existing `resolve_timestamp_setup(&alsa::Error, &str, bool) -> Result<AlsaClock, AppError>`.
- Produces: `configure_clock(&mut Rawmidi, &str, bool) -> Result<AlsaClock, AppError>` configured without erasing stream parameters, plus `has_kernel_timestamp_configuration(ReadMode, Clock) -> bool` for testable effective-setting validation.

- [ ] **Step 1: Write failing validation tests**

Add these tests to `src/backend/alsa/capture.rs`'s existing test module and import `Clock`, `ReadMode`, and `has_kernel_timestamp_configuration`:

```rust
#[test]
fn kernel_timestamp_configuration_requires_timestamp_mode_and_raw_clock() {
    assert!(has_kernel_timestamp_configuration(
        ReadMode::Timestamp,
        Clock::MonotonicRaw,
    ));
    assert!(!has_kernel_timestamp_configuration(
        ReadMode::Standard,
        Clock::MonotonicRaw,
    ));
    assert!(!has_kernel_timestamp_configuration(
        ReadMode::Timestamp,
        Clock::Monotonic,
    ));
}
```

- [ ] **Step 2: Run the focused test and verify it fails**

Run: `cargo test backend::alsa::capture::tests::kernel_timestamp_configuration_requires_timestamp_mode_and_raw_clock`

Expected: compilation failure because `has_kernel_timestamp_configuration` is undefined.

- [ ] **Step 3: Implement preserved configuration and verification**

In `configure_clock`:

```rust
let mut params = handle
    .params_current()
    .map_err(|error| map_open_error(&error, device))?;
```

Set `ReadMode::Timestamp` and `Clock::MonotonicRaw` on `params`, apply it with
`handle.params(&params)`, then fetch `handle.params_current()` again. Add:

```rust
fn has_kernel_timestamp_configuration(read_mode: ReadMode, clock: Clock) -> bool {
    read_mode == ReadMode::Timestamp && clock == Clock::MonotonicRaw
}
```

Reject a nonmatching effective configuration with an `alsa::Error` using
`libc::EINVAL`, so `resolve_timestamp_setup` retains its existing explicit
fallback behavior. Preserve existing error mapping for ALSA calls.

- [ ] **Step 4: Run the focused test and full automated checks**

Run:

```bash
cargo test backend::alsa::capture::tests
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Expected: all commands succeed.

- [ ] **Step 5: Perform hardware verification**

With the Scarlett clock source transmitting, run:

```bash
cargo run --release -- record \
  --backend alsa-raw \
  --source hw:2,0,0 \
  --duration 10 \
  --output /tmp/midijitter-alsa-raw.json
```

Then run:

```bash
cargo run --release -- analyze /tmp/midijitter-alsa-raw.json
```

Expected: `record` reports a nonzero number of MIDI clocks and kernel RawMIDI
timestamping; `analyze` produces an ALSA RawMIDI report.

- [ ] **Step 6: Commit the implementation**

```bash
git add src/backend/alsa/capture.rs
git commit -m "fix: preserve ALSA RawMIDI parameters"
```
