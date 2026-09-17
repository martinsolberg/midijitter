# Cadence Anchor Backlog Design

## Problem

PipeWire captures can begin with a backlog of MIDI Clock events that all have
the same graph timestamp. The current cadence detector can select the final
backlog event as the live anchor when its following interval is within the
normal 20% tolerance. That makes the first backlog-to-live interval look like
a valid musical interval, contaminating phase extrema and the minimum period.

In `octatrack_capture.json`, this selects sequence 64 as tick zero. Its
16.521 ms interval to sequence 65 is not a real clock interval and produces a
3.539 ms phase peak-to-peak result. The following stream is approximately
0.45 ms peak-to-peak.

## Decision

For cadence-enabled startup detection, a candidate live anchor after the first
captured event must have a strictly positive interval from the preceding raw
event, in addition to the existing run of plausible one-tick intervals.

Therefore, when a candidate follows a zero or negative timestamp interval, it
cannot be the live anchor. The detector continues scanning and selects the
first later candidate whose preceding interval is positive and whose cadence
confirmation succeeds. In the affected capture, sequence 64 is retained as a
startup transient and sequence 65 becomes tick zero.

This is intentionally a narrow rule based on an observed timestamp property,
not a tighter musical-period threshold. It preserves legitimate capture-start
phase offsets while excluding an event still participating in a timestamped
backlog.

The rule may need to be revisited if future capture backends expose a different
startup pattern, such as a non-duplicate backlog or an explicit event-batch
boundary. Keep the regression test and this note as the prompt for that
review; do not add speculative heuristics now.

## Scope

- Update cadence anchor selection only.
- Preserve startup rows and existing disposition/reporting behavior.
- Add a regression test covering duplicate timestamps followed by a shortened
  first live interval.
- Verify the real Octatrack capture no longer reports the backlog anchor in
  clean phase or period extrema.

## Acceptance Criteria

- The new regression test fails before the implementation and passes after it.
- The affected capture selects the event after the duplicate timestamp run as
  its live anchor.
- Clean phase peak-to-peak excludes the backlog anchor.
- Clean period minimum excludes the backlog-to-live interval.
- Existing cadence, settle-window, analysis, formatting, clippy, and full test
  coverage remain passing.
