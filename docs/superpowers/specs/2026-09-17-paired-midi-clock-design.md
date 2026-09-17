# Paired MIDI Clock Capture And Round-Trip Analysis

## Goal

Add a PipeWire paired-capture mode that measures per-clock path latency between a DAW reference MIDI Clock and its returned physical/logical path.

## Approved Design

Use one native `pw_filter` with two MIDI input ports and one `process(position)` callback. The callback dequeues both ports under the same `spa_io_position`, preserving one graph clock domain and one common timestamp origin.

The v2 capture document contains common graph/capture metadata, source metadata for `reference` and `returned`, and separate event arrays. Stored timestamps are derived from `event_position - common_origin_position`; streams must never be normalized independently.

Existing v1 captures and single-stream analysis remain supported. Paired captures use a dedicated `PairedAnalysisResult` containing independent stream analyses, tick-aligned pair rows, pairing diagnostics, and path-latency statistics.

Pairing uses analyzed logical `tick_index`, not raw ordinal position. Transport anchors establish alignment where available. Without anchors, candidate integer lags are scored by robust latency MAD; only a unique smallest strictly-positive median latency is accepted. Ambiguous periodic correspondence is rejected.

Pairs remain structurally represented when correspondence is known, including anomalous events. Only pairs whose two stream rows are `Valid` contribute to clean latency statistics.

The initial release includes `compare-live`, offline `analyze` dispatch, text/JSON reports, and comprehensive synthetic/format/timing tests. Pair CSV and plots are deferred.

## Validity And Errors

Structural capture validity, paired-analysis eligibility, and capture completion are separate concepts. Graph clock ID/rate/quantum transitions are persisted as typed transitions; v2 analysis conservatively rejects captures containing them. Interrupted captures preserve collected events but are not eligible for paired analysis.

Validation rejects malformed common timebases, invalid PipeWire metadata, empty or identical source identities, non-monotonic stream sequences, missing stream data, ambiguous pairing, and captures with no valid pairs.

## Testing

Cover common-origin and cross-cycle timestamp conversion, two-port same-cycle processing, graph transition recording, v1/v2 round trips, malformed common-timebase rejection, source-jitter cancellation, constant/variable latency, missing/duplicate resynchronization, startup asymmetry, one-sided anomalies, anchored alignment, unanchored sub-period alignment, ambiguous multi-period delay, CLI dispatch, and immediate-versus-reloaded structured-result equivalence.
