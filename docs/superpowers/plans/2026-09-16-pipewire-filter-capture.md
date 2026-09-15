# PipeWire Filter Capture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a working PipeWire MIDI capture path built on `pw_filter` (mirroring `pw-mididump.c`), selectable via `--pw-api filter|stream` with `filter` as default, while keeping the existing `pw_stream` path intact.

**Architecture:** New `src/backend/pipewire/filter_capture.rs` wraps the already-generated `pw_filter_*` / `spa_io_position` bindings (no new crates, no C declarations); all parsing, timing conversion, and capture-file assembly is shared via a new `src/backend/pipewire/common.rs` extracted from the stream path.

**Tech Stack:** Rust (stable), `pipewire` crate 0.10.1 (safe wrappers for main loop/context/core) + its generated `pw::sys` raw bindings for `pw_filter_*`, `libspa` (`pw::spa::sys`) for `spa_io_position`, existing `MidiParser` / `timing.rs`.

## Global Constraints

- PipeWire event timing MUST be graph cycle position + event offset; never `Instant::now()` / `clock_gettime()` on the PipeWire path.
- The filter process callback MUST stay RT-safe: parse + append to preallocated `Vec` only; no I/O, logging, allocation, or locking beyond the existing pattern.
- Do NOT add dependencies to `Cargo.toml`; `pw_filter_*` and `spa_io_position` come from the already-generated `pipewire-sys` / `libspa-sys` bindings.
- Preserve the rational rate (`rate_num` / `rate_denom`) exactly; never hard-code a sample rate.
- Every task ends with `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` passing.
- Tests that need a live PipeWire daemon MUST be marked `#[ignore]`; pure logic tests run unconditionally.

---

## File structure

- `src/backend/pipewire/common.rs` (new): shared capture core moved out of `capture.rs` — `STOP_*` constants, `CaptureState` + impl (`new`, `request_stop`, `observe_timing`, `termination_reached`), `event_capacity`, `spa_sequence_from_bytes`, `record_spa_sequence`, `record_control_bytes`, and the stop-reason match + `CaptureFile` assembly (moved as `finish_capture`).
- `src/backend/pipewire/capture.rs` (modify): stream transport only — keeps `record()`, listener setup, `process_buffer`; imports shared pieces from `common`; reverts the watchdog + unclocked-graph theory.
- `src/backend/pipewire/filter_capture.rs` (new): filter transport — raw `pw_filter_*` wrapper, `process` callback reading `spa_io_position`, `run_filter_capture()` producing `CaptureFile` via `common::finish_capture`.
- `src/backend/pipewire/mod.rs` (modify): declare `common` + `filter_capture`, dispatch `record()` on the new request field.
- `src/backend/mod.rs` (modify): add `pw_api: PwApi` (`Filter` default / `Stream`) to `CaptureRequest` with builder; keep `manual_connect` applicable to both PipeWire paths.
- `src/cli.rs` (modify): add `record --pw-api filter|stream` (default `filter`), validate PipeWire-only, print it in the PipeWire preambles.
- `src/error.rs` (modify): remove the disproven `PipeWireMidiGraphUnclocked` variant.
- `examples/filter_spike.rs` (temporary, Task 3 only): live spike resolving the autoconnect mechanism; deleted in Task 7.

---

### Task 1: Revert the disproven unclocked-graph theory

**Files:**
- Modify: `src/backend/pipewire/capture.rs` (timer block, `process_buffer` zero-timing comment, stop-reason match)
- Modify: `src/error.rs` (remove `PipeWireMidiGraphUnclocked` variant)

**Interfaces:**
- Consumes: nothing.
- Produces: clean stream path identical in behavior to commit `0000791` plus the kept `manual_connect` / format-dsp / data-buffer-sequence / ALSA-errno work.

**Why:** The watchdog, `STOP_UNCLOCKED_GRAPH`, and the "bridge has no clock" error message rest on a disproven theory (`pw-mididump` proves a clock is available). They must go before building on this code.

- [ ] **Step 1: Remove the error variant**

In `src/error.rs`, delete the `PipeWireMidiGraphUnclocked` variant and its `#[error(...)]` attribute (the multi-line message about the ALSA sequencer bridge having no graph clock).

- [ ] **Step 2: Remove the watchdog machinery from `capture.rs`**

Delete: the `STOP_UNCLOCKED_GRAPH` constant, the `UNCLOCKED_GRAPH_GRACE` constant, the `unclocked_since` cell, and the streamed-but-never-clocked branch of the 1ms timer. Restore the timer to only quit the loop when a stop reason is set or SIGINT/SIGTERM arrived:

```rust
let timer = main_loop.loop_().add_timer(move |_| {
    if stop_state.borrow().stop.load(Ordering::Acquire) != STOP_NONE
        || interrupted_state.load(Ordering::Acquire)
    {
        stop_loop.quit();
    }
});
```

- [ ] **Step 3: Remove the stop-reason arm and fix the zero-timing comment**

Delete the `STOP_UNCLOCKED_GRAPH => return Err(AppError::PipeWireMidiGraphUnclocked),` match arm. Change the zero rate/denom/quantum early-return comment in `process_buffer` to:

```rust
// Before a link is active, the callback may fire with a zeroed graph
// position; wait for a real cycle.
```

- [ ] **Step 4: Run the gate**

Run: `cargo test 2>&1 | rg "test result"`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`
Expected: 103 passed / 0 failed, clippy clean, fmt clean.

- [ ] **Step 5: Commit**

```bash
git add src/backend/pipewire/capture.rs src/error.rs
git commit -m "revert: drop disproven unclocked-graph watchdog and error"
```

---

### Task 2: Extract the shared capture core into `common.rs`

**Files:**
- Create: `src/backend/pipewire/common.rs`
- Modify: `src/backend/pipewire/capture.rs` (use `common::`, delete moved items)
- Modify: `src/backend/pipewire/mod.rs` (add `mod common;`)

**Interfaces:**
- Consumes: nothing new.
- Produces (all `pub(super)`, same signatures as today):
  - `STOP_NONE / STOP_COMPLETE / STOP_OVERFLOW / STOP_UNSUPPORTED_FORMAT / STOP_SOURCE_GONE / STOP_NEGOTIATION_FAILURE / STOP_TIMESTAMP_ERROR / STOP_STREAM_ERROR: u8`
  - `struct CaptureState` with `new(termination, event_capacity)`, `request_stop(reason)`, `observe_timing(rate_num, rate_denom, quantum)`, `termination_reached(cycle_position)`
  - `event_capacity(termination) -> Result<usize, AppError>`
  - `spa_sequence_from_bytes(bytes: &[u8]) -> Option<&spa::sys::spa_pod_sequence>` (widen from private to `pub(super)`)
  - `record_spa_sequence(sequence, cycle_position, rate_num, rate_denom, quantum, state) -> Result<(), ()>` (unsafe fn, unchanged body)
  - `record_control_bytes(...)` (unchanged signature and body)
  - `finish_capture(state: CaptureState, request: &CaptureRequest) -> Result<CaptureFile, AppError>` — the exact stop-reason match plus `CaptureFile` construction currently at the end of `capture::record`, including `normalize_pipewire_event_timestamps` and the `"PipeWire graph position + event offset"` timestamp method.

**Why:** The filter path must reuse parsing, termination, and file assembly byte-for-byte instead of duplicating them. Pure move, zero behavior change.

- [ ] **Step 1: Create `common.rs` with the moved items**

Move (do not rewrite) the items listed above from `capture.rs` into `src/backend/pipewire/common.rs`. Move the minimum imports they need (`MidiParser`, `CaptureTermination`, `CapturedEvent`, `GraphTransition`, `PipeWireTimestamp`, `SourceMetadata`, `TimestampMetadata`, `EnvironmentMetadata`, `CaptureFile`, `AppError`, `MetaControl`-free helpers, `pipewire as pw`, `pw::spa`, atomics). `finish_capture` takes ownership of the state and the request for source metadata:

```rust
pub(super) fn finish_capture(
    mut state: CaptureState,
    request: &CaptureRequest,
) -> Result<CaptureFile, AppError> {
    // ... exact stop-reason match moved from capture::record ...
    // ... exact CaptureFile construction moved from capture::record ...
}
```

The stream `record()` keeps its setup/listener/timer code and ends with `finish_capture(state.into_inner(), request)` — adjust borrow handling minimally so it compiles (`state` is `Rc<RefCell<...>>`; use `Rc::try_unwrap(state).map_err(...).unwrap().into_inner()` or restructure to `let state = state.borrow_mut(); ... finish_capture(std::mem::replace(...))` — simplest correct approach: change the tail to operate then call `finish_capture` with a moved-out value; verify by compiling).

- [ ] **Step 2: Rewire `capture.rs` and `mod.rs`**

In `capture.rs`, delete the moved definitions, add `use super::common::{...};`, keep `midi_control_format()` (stream-specific) and `process_buffer` where they are. In `mod.rs`, add `mod common;` before `mod capture;`.

- [ ] **Step 3: Run the gate**

Run: `cargo test 2>&1 | rg "test result"`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`
Expected: 103 passed / 0 failed, clippy clean, fmt clean.

- [ ] **Step 4: Commit**

```bash
git add src/backend/pipewire/common.rs src/backend/pipewire/capture.rs src/backend/pipewire/mod.rs
git commit -m "refactor: extract shared PipeWire capture core into common module"
```

---

### Task 3: Spike — resolve the filter autoconnect mechanism live

**Files:**
- Create: `examples/filter_spike.rs` (temporary; deleted in Task 7)

**Interfaces:**
- Consumes: generated `pw::sys` bindings only.
- Produces: a printed verdict — nonzero `position->clock.rate` + `duration` when linked to the Midi-Bridge, and which link method worked (`target.object` port property vs. manual `pw-link`).

**Why:** The design's one open point is how a filter auto-links to a selected source port. Guessing wrong here poisons Task 5. A 20-minute live spike answers it definitively.

- [ ] **Step 1: Write the spike**

Create `examples/filter_spike.rs`: init PipeWire, build a main loop, `pw_filter_new_simple` with `media.type=Midi`, `pw_filter_add_port` with `PW_DIRECTION_INPUT`, `PW_FILTER_PORT_FLAG_MAP_BUFFERS`, props `format.dsp="8 bit raw midi"` and `target.object=<Midi-Bridge port id, e.g. 61>`, connect with `PW_FILTER_FLAG_RT_PROCESS`, and a process callback that prints `position->clock.rate.num/denom`, `position->clock.position`, and `position->clock.duration` once per second for 5 seconds, then quits. Read the port id from `std::env::args()` so no value is hard-coded:

```rust
let target = std::env::args().nth(1).expect("usage: filter_spike <source-port-id>");
```

- [ ] **Step 2: Run against the live graph**

Run: `cargo run --example filter_spike -- 61`
Expected: nonzero rate (e.g. `1/48000`) and advancing `position`, proving `target.object` auto-links a filter input port. If the rate stays zero / no link appears in `pw-cli ls Link`, retry with the port prop omitted and link manually via `pw-link 61 <filter-port>`; the printed verdict decides Task 5's connect code. This step requires a live PipeWire daemon; it needs no MIDI traffic.

- [ ] **Step 3: Record the verdict in the task output**

Report: (a) rate/position/duration values seen, (b) which link method worked. Do NOT commit the example yet; it is deleted in Task 7.

---

### Task 4: Pure position-timing extraction with unit tests (TDD)

**Files:**
- Modify: `src/backend/pipewire/filter_capture.rs` (create with pure logic only)
- Test: unit tests in `filter_capture.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `pw::spa::sys::{spa_io_position, spa_fraction}` (generated bindings, plain structs — constructible in tests with no daemon).
- Produces:
  - `pub(super) struct PositionTiming { pub cycle_ticks: i64, pub rate_num: u32, pub rate_denom: u32, pub quantum: u32 }`
  - `pub(super) fn position_timing(position: &spa_io_position) -> Result<PositionTiming, AppError>`

**Why:** Isolates the only new timestamp math (position clock → our timing tuple) where it is unit-testable without a daemon or `unsafe` filter handling.

- [ ] **Step 1: Write the failing tests**

Create `src/backend/pipewire/filter_capture.rs` containing only the struct, a stub `position_timing` returning `Err(AppError::PipeWireNegotiationFailed { detail: "stub".into() })`, and these tests (declare `mod filter_capture;` in `mod.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::position_timing;
    use pipewire as pw;

    fn position(rate_num: u32, rate_denom: u32, ticks: u64, duration: u64) -> pw::spa::sys::spa_io_position {
        // SAFETY: zeroed plain-data bindings struct, fully populated below.
        let mut position: pw::spa::sys::spa_io_position = unsafe { std::mem::zeroed() };
        position.clock.rate.num = rate_num;
        position.clock.rate.denom = rate_denom;
        position.clock.position = ticks;
        position.clock.duration = duration;
        position
    }

    #[test]
    fn position_timing_extracts_ticks_rate_and_quantum() {
        let timing = position_timing(&position(1, 48000, 480000, 1024)).unwrap();
        assert_eq!((timing.cycle_ticks, timing.rate_num, timing.rate_denom, timing.quantum), (480000, 1, 48000, 1024));
    }

    #[test]
    fn position_timing_rejects_zero_rate_or_quantum() {
        assert!(position_timing(&position(0, 48000, 1, 1024)).is_err());
        assert!(position_timing(&position(1, 0, 1, 1024)).is_err());
        assert!(position_timing(&position(1, 48000, 1, 0)).is_err());
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test filter_capture 2>&1 | tail -n 8`
Expected: FAIL (stub returns Err, first test panics on `unwrap`).

- [ ] **Step 3: Implement `position_timing`**

```rust
pub(super) struct PositionTiming {
    pub cycle_ticks: i64,
    pub rate_num: u32,
    pub rate_denom: u32,
    pub quantum: u32,
}

pub(super) fn position_timing(
    position: &pw::spa::sys::spa_io_position,
) -> Result<PositionTiming, AppError> {
    let clock = &position.clock;
    let (rate_num, rate_denom) = (clock.rate.num, clock.rate.denom);
    let quantum = u32::try_from(clock.duration)
        .map_err(|_| AppError::TimestampArithmeticOverflow)?;
    if rate_num == 0 || rate_denom == 0 || quantum == 0 {
        return Err(AppError::PipeWireNegotiationFailed {
            detail: "PipeWire filter reported an invalid graph rate or quantum".to_owned(),
        });
    }
    let cycle_ticks = i64::try_from(clock.position)
        .map_err(|_| AppError::TimestampArithmeticOverflow)?;
    Ok(PositionTiming { cycle_ticks, rate_num, rate_denom, quantum })
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test filter_capture 2>&1 | tail -n 5`
Expected: 2 passed / 0 failed.

- [ ] **Step 5: Commit**

```bash
git add src/backend/pipewire/filter_capture.rs src/backend/pipewire/mod.rs
git commit -m "feat: extract PipeWire filter position timing with tests"
```

---

### Task 5: Filter wrapper and `run_filter_capture`

**Files:**
- Modify: `src/backend/pipewire/filter_capture.rs` (append wrapper + capture loop)
- Modify: `src/backend/pipewire/mod.rs` (re-export nothing new; dispatch comes in Task 6)

**Interfaces:**
- Consumes: Task 2 (`common::{CaptureState, finish_capture, record_spa_sequence, spa_sequence_from_bytes, event_capacity, STOP_*}`), Task 4 (`position_timing`), Task 3 verdict (autoconnect mechanism).
- Produces: `pub(super) fn run_filter_capture(request: &CaptureRequest) -> Result<CaptureFile, AppError>`

**Why:** This is the working capture path. It mirrors `pw-mididump.c`'s `dump_filter`/`on_process` and the stream `record()` tail, with all `unsafe` confined to this file.

- [ ] **Step 1: Write a daemon-gated construction test (ignored by default)**

```rust
#[test]
#[ignore = "needs a live PipeWire daemon"]
fn filter_builds_and_connects_without_target() {
    // init, main loop, filter_new_simple, add_port with format.dsp only,
    // connect(RT_PROCESS), assert node id != 0, destroy.
}
```

Run: `cargo test filter_builds 2>&1 | tail -n 3` (expect 0 run / 1 ignored), then `cargo test -- --ignored filter_builds 2>&1 | tail -n 3` (expect PASS on a live host).

- [ ] **Step 2: Implement the wrapper**

Requirements for the implementation (verify each name against the generated bindings — all confirmed present):
- `pw::init()`, then `MainLoopRc` / `ContextRc` / `connect_rc` exactly like the stream path for daemon errors (`pipewire_unavailable`-equivalent mapping already exists in `capture.rs`; reuse the same mapping function — move it to `common.rs` if it is not already there).
- Filter props via raw `pw::sys::pw_properties_new_string` / `pw_properties_set` with `CString`s: `media.type=Midi`, `media.category=Capture`, `media.role=Music`, `node.name=midijitter-capture`.
- Port props: `format.dsp=8 bit raw midi`, `port.name=input_1`; plus the Task 3 verdict: `target.object=<source port id>` when autoconnecting, omitted when `request.manual_connect`.
- `pw_filter_new_simple(main_loop raw loop, c"midi-dump"... )` — get the raw `*mut pw_loop` from the `MainLoopRc` the same way `pipewire-rs` internals do (check `MainLoopRc::loop_()` accessor for a raw pointer; if none is exposed, create the filter before wrapping or store the raw pointer at construction — resolve at implementation time, keep the safe wrappers for loop lifetime).
- Events struct `pw_filter_events { version: PW_VERSION_FILTER_EVENTS, process: Some(on_process), state_changed: Some(on_state_changed), ..zeroed }`; `data` pointer = leaked `Box<StateCell>` where `struct StateCell { state: Rc<RefCell<CaptureState>> }`. `on_process(data, position)`: on null `position` return immediately; `dequeue_buffer(port)` via the stored port pointer (kept in the leaked box, filled after `add_port`); parse `MetaControl` else `spa_sequence_from_bytes` (same as stream `process_buffer`); skip non-`SPA_CONTROL_Midi` controls and all `SPA_CONTROL_Ump`; record via `common::record_spa_sequence` with `position_timing`; `termination_reached(cycle_ticks)` → `STOP_COMPLETE`; always `queue_buffer` afterwards. `on_state_changed`: on `PW_FILTER_STATE_ERROR`, `request_stop(STOP_STREAM_ERROR)` (reuse the stream error slot; store detail string like the stream path does).
- Connect with `PW_FILTER_FLAG_RT_PROCESS`, run the main loop with the same 1ms stop/interrupt timer as the stream path, then `finish_capture`.
- `Drop`-equivalent cleanup: `pw_filter_disconnect` + `pw_filter_destroy` + reclaim the leaked box after `main_loop.run()` returns. No global state.
- Print `Sink: midijitter-capture:input_1 ...` when `manual_connect`, same text as the stream path.

- [ ] **Step 3: Run the gate**

Run: `cargo test 2>&1 | rg "test result"`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`
Expected: all green (new `#[ignore]` test excluded from the normal run).

- [ ] **Step 4: Commit**

```bash
git add src/backend/pipewire/filter_capture.rs
git commit -m "feat: add PipeWire filter capture path mirroring pw-mididump"
```

---

### Task 6: CLI flag, dispatch, and validation

**Files:**
- Modify: `src/backend/mod.rs` (`PwApi` enum, `CaptureRequest.pw_api` field + builder, default `Filter`)
- Modify: `src/backend/pipewire/mod.rs` (dispatch `record()` on `pw_api`)
- Modify: `src/cli.rs` (`--pw-api` flag, validation, preamble prints)

**Interfaces:**
- Consumes: Task 5 (`run_filter_capture`), existing `capture::record`.
- Produces: `record --backend pipewire [--pw-api filter|stream] [--manual-connect]`; stream path byte-identical when selected.

- [ ] **Step 1: Add `PwApi` and wire dispatch with tests**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PwApi {
    #[default]
    Filter,
    Stream,
}
```

Add `pub pw_api: PwApi` to `CaptureRequest` (default `Filter` in `new()`), plus `.pw_api(...)` builder. Dispatch in `pipewire/mod.rs`:

```rust
fn record(&self, request: CaptureRequest) -> Result<CaptureFile, AppError> {
    match request.pw_api {
        PwApi::Filter => filter_capture::run_filter_capture(&request),
        PwApi::Stream => capture::record(request),
    }
}
```

Extend the existing `backend::tests` with: default is `Filter`, builder sets `Stream`, `manual_connect` composes with either API.

- [ ] **Step 2: Run the new tests to verify they pass**

Run: `cargo test backend:: 2>&1 | tail -n 5`
Expected: PASS including the new assertions.

- [ ] **Step 3: Add the CLI flag**

In `cli.rs`: new `PwApiArg` `ValueEnum` (`filter`, `stream`, default `filter`); `Record { ..., pw_api: PwApiArg }`; reject `--pw-api` unless `--backend pipewire` (same style as the existing `manual_connect` check); thread through `run_record` into `CaptureRequest`; print `PipeWire API: filter|stream` in both PipeWire preamble branches.

- [ ] **Step 4: Run the gate and verify help text**

Run: `cargo test 2>&1 | rg "test result"`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo run --quiet -- record --help | rg "pw-api"`
Expected: green; help shows `--pw-api <FILTER|STREAM>`.

- [ ] **Step 5: Commit**

```bash
git add src/backend/mod.rs src/backend/pipewire/mod.rs src/cli.rs
git commit -m "feat: select PipeWire filter/stream capture via --pw-api"
```

---

### Task 7: Gate, remove spike, live validation procedure

**Files:**
- Delete: `examples/filter_spike.rs`

**Interfaces:**
- Consumes: Tasks 1–6.
- Produces: clean tree, full gate green, and a validated-or-reported live result.

- [ ] **Step 1: Delete the spike and run the full gate**

Run: `rm examples/filter_spike.rs && cargo test 2>&1 | rg "test result" && cargo clippy --all-targets -- -D warnings && cargo fmt --check && git status --short`
Expected: 105+ passed / 0 failed (103 baseline + Task 4/6 additions), clippy/fmt clean, only intended files modified.

- [ ] **Step 2: Live validation (requires the Scarlett clock; run by the user)**

```bash
cargo build --release
./target/release/midijitter record --backend pipewire --source "Scarlett 18i20 USB MIDI 1 (capture)" --duration 15 --output /tmp/filter.json
./target/release/midijitter analyze /tmp/filter.json
```

Expected: capture terminates on its own after ~15 s, reports nonzero MIDI clocks, and `analyze` prints phase/period jitter. Manual-connect variant:

```bash
./target/release/midijitter record --backend pipewire --source "Scarlett 18i20 USB MIDI 1 (capture)" --duration 15 --output /tmp/filter-manual.json --manual-connect &
# link 61 -> midijitter-capture:input_1 in a patchbay, then wait
./target/release/midijitter analyze /tmp/filter-manual.json
```

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat: PipeWire filter capture backend with --pw-api selection"
```

---

## Self-review

- Spec coverage: architecture (§3) → Tasks 2/5; timing (§4) → Tasks 4/5; UX (§5) → Tasks 5/6 (source matching reused, manual-connect on filter, `NoClockEvents` when unlinked — inherited from `finish_capture`); errors/RT (§6) → Tasks 1/5 (`finish_capture` reuses existing errors; RT rules stated in Task 5); testing (§7) → Tasks 4/5/6/7. The spec's "resolve autoconnect mechanism" note → Task 3.
- Placeholders: none — every code step names exact items, signatures, constants, and commands; FFI names verified against the generated bindings in `target/debug/build`.
- Type consistency: `PositionTiming` fields feed `record_spa_sequence(cycle_position, rate_num, rate_denom, quantum)` 1:1; `run_filter_capture(&CaptureRequest)` matches the dispatch site; `PwApi` naming is uniform across backend/CLI tasks.
- One known implementation-time resolution (flagged, not a placeholder): obtaining the raw `*mut pw_loop` for `pw_filter_new_simple` while keeping pipewire-rs lifetime wrappers — Task 5 states the constraint and the fallback explicitly.
