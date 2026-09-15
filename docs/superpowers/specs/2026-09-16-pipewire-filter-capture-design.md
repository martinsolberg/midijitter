# PipeWire Filter Capture Design

Date: 2026-09-16
Status: approved, pending implementation plan

## 1. Problem

The PipeWire backend (`src/backend/pipewire/capture.rs`) captures via
`pipewire-rs` `pw_stream` and reads graph timing with `Stream::time()`
(`pw_stream_get_time`). On the target host this returns zero
rate/quantum forever, even with an active, driver-linked MIDI source, so
no events are timestamped and `--duration` never terminates (duration is
evaluated in stream time).

Meanwhile `pw-mididump` — PipeWire's own MIDI tool — demonstrably
receives both MIDI data and a valid clock on the same host. Its
`on_process` uses `pw_filter`, which receives
`struct spa_io_position *position` directly in the process callback and
timestamps events as `position->clock` ticks plus each control's offset.
It never calls `pw_stream_get_time`.

Conclusion: the zero timing is specific to our `pw_stream` usage, not a
property of the host's PipeWire graph. An earlier "unclocked graph"
theory (plus a watchdog and a `PipeWireMidiGraphUnclocked` error built
on it) is disproven and must be reverted.

Guiding decision: get a working PipeWire capture in place first (via the
proven filter API), then work backwards toward full spec compliance.

## 2. Key facts

- `pipewire-sys` generates bindings at build time with bindgen against
  the system libpipewire (1.4.9). The generated bindings already contain
  `pw_filter_new_simple`, `pw_filter_add_port`, `pw_filter_connect`,
  `pw_filter_dequeue_buffer`, `pw_filter_queue_buffer`,
  `pw_filter_destroy`, and `pw_filter_events`.
- `spa_io_position` / `spa_io_clock` are already present in the
  generated `libspa-sys` bindings.
- Therefore the filter path needs no C declarations, no new crates, and
  no build-system changes: one Rust wrapper module over already-existing
  generated bindings, with `pw-mididump.c` as the exact reference.
- The newest released `pipewire-rs` (0.10.1) has no safe `filter`
  module, so a thin local wrapper is required.

## 3. Architecture

New module `src/backend/pipewire/filter_capture.rs` owning all
`unsafe`. It exposes one safe entry point with the same shape as the
stream path:

```rust
run_filter_capture(request) -> Result<CaptureFile, AppError>
```

It wraps: filter creation (`media.type=Midi`), one input port with
`format.dsp="8 bit raw midi"`, connect, buffer dequeue/queue, destroy.

Everything downstream is shared and untouched: `MidiParser`,
`record_spa_sequence` / `record_control_bytes`, `timing.rs` conversion,
the `CaptureFile` format, analysis, and CLI output.

The existing stream implementation stays as-is; `mod.rs` dispatches on a
new `--pw-api filter|stream` flag (default `filter`).

## 4. Timing conversion

The filter process callback receives `*mut spa_io_position` as a
callback argument — no `get_time` call. Following `pw-mididump.c`:

- cycle tick base from the position clock,
- per-event offset from each `spa_pod_control.offset`,
- rational rate from `position->clock.rate`, preserved exactly (never
  hard-coded).

So `Pevent = Pcycle_ticks + Oevent`, flowing through the existing
`pipewire_event_position` path into the unchanged `PipeWireTimestamp`
record (`cycle_position`, `event_offset`, `event_position`, `rate_num`,
`rate_denom`, `quantum`, where `quantum` is `position->clock.duration`).

Duration/tick termination is evaluated against the position clock, which
advances every cycle. `--duration` therefore terminates even with no
MIDI traffic, fixing the hang structurally instead of with a watchdog.

`SPA_CONTROL_Ump` controls are skipped in v0.1, matching how the stream
path skips non-MIDI controls.

## 5. Connection UX

- Source selection is unchanged: existing `devices` enumeration and
  `--source` matching. The exact autoconnect mechanism on the filter
  path (port `target.object` property vs. explicit link creation) is to
  be resolved during implementation; `--manual-connect` is the
  guaranteed fallback either way.
- `--manual-connect` is supported on the filter path: connect with no
  target so `midijitter-capture:input_1` appears in the patchbay for
  manual linking.
- If no link is present, the filter never reaches STREAMING and its
  process callback never fires, so stream-time termination cannot end
  the run. A wall-clock liveness bound on the main-loop thread
  (`--duration` + 5 s grace, only while zero events are captured) ends
  such runs cleanly with `NoClockEvents`. This bound never touches event
  timestamps — it only bounds total runtime when no graph callbacks
  arrive. The failure mode becomes "no events", not "no stop".

## 6. Error handling and RT discipline

- Revert the watchdog timer and the `PipeWireMidiGraphUnclocked` error.
- Filter-path failures reuse the existing spec-listed errors:
  `PipeWireNegotiationFailed`, `PipeWireSourceDisappeared`,
  `PipeWireUnsupportedControlFormat`, `NoClockEvents`.
- The filter process callback follows the spec's RT rules: parse plus
  append into the preallocated `Vec` only; no I/O, no logging, no
  unbounded allocation.

## 7. Testing and acceptance

- Unit tests: position-to-timestamp conversion vectors (including
  rational-rate cases), UMP-skip behavior, and termination on the
  position clock with zero MIDI traffic.
- Gate: `cargo test`, `cargo clippy --all-targets -- -D warnings`,
  `cargo fmt --check`.
- Live acceptance: `record --pw-api filter` against the Scarlett clock
  captures `F8` events and terminates on `--duration`; the resulting
  capture round-trips through the existing `analyze` / `plot` suite.
