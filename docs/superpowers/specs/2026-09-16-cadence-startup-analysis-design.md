# Cadence-Based Startup Analysis

## Goal

Make startup handling a first-class part of MIDI Clock analysis. PipeWire and
ALSA-seq captures should establish the live periodic stream before assigning
tick zero, fitting the clock grid, or reporting jitter. This removes the
accidental privilege of the first received event and prevents stale link-time
backlog from appearing as a clock excursion.

## Scope

- Add two-phase startup detection and live-anchor selection.
- Make cadence confirmation the default for PipeWire/ALSA-seq captures.
- Add `--startup-cadence <N>`, defaulting to `8`; `0` disables cadence
  filtering.
- Retain `--settle <duration>` as an optional lower-bound constraint.
- Replace independent row booleans with one authoritative event disposition.
- Add interval classification and explicit missing-before data.
- Report startup and anomaly populations separately.
- Make numerical reports, plots, JSON, and CSV consume the same row model and
  population rules.

The change does not add an all-steady-state-including-anomalies metric block.
Anomalies remain available in JSON/CSV and receive a dedicated summary.

## Analysis Pipeline

### Phase 1: startup detection

1. Collect MIDI Clock events and calculate a robust initial period estimate
   from positive intervals using the existing median-based approach.
2. Establish the earliest eligible event position. No startup anchor candidate
   may occur before `capture_start + settle_ns`.
3. Treat zero/negative intervals in the leading startup region as transient
   backlog candidates.
4. Search for the first complete run of `N` consecutive plausible one-tick
   intervals whose first event is at or after the settle boundary.
5. Use the first event of that run as the live anchor and assign it tick zero.
   The confirmed run includes all `N + 1` events.
6. Mark all events before the live anchor as `StartupTransient`.

Plausibility is relative to the robust period estimate:

```text
abs(interval - estimated_period) / estimated_period <= integer_multiple_tolerance
```

The cadence count is configurable. For `N = 0`, startup cadence filtering is
disabled and the analyzer retains diagnostic behavior without a startup
boundary. For PipeWire/ALSA-seq captures, the default is `N = 8`. Other
backends retain cadence filtering disabled unless explicitly requested.

`--settle` does not replace cadence detection. It only prevents a candidate
run from beginning before the settle boundary. For example, if the boundary
falls between events A and B, a run that uses B as its first event is eligible;
the event before the boundary cannot be retroactively included.

### Phase 2: live-stream classification

Starting at the selected live anchor, perform the existing iterative period
fit and classification. The anchor receives tick index zero. Subsequent rows
are assigned tick indices, missing counts, duplicate status, and anomaly status
without consulting raw-event position as a special case.

## Authoritative Data Model

Each analyzed event has one primary disposition:

```rust
enum EventDisposition {
    Valid,
    StartupTransient,
    Duplicate,
    Anomalous,
}
```

Each row also carries interval information independently of event
disposition:

```rust
enum IntervalDisposition {
    Normal,
    Missing { count: u32 },
    Anomalous,
}
```

The public analysis row includes `disposition`, `missing_before`, and the
interval classification needed by output consumers. A missing clock is a
property of the interval before an event, not a mutually exclusive event
status. The first event after a multi-tick gap may therefore remain a valid
phase observation while exposing `missing_before > 0`.

Existing boolean fields may be removed or derived centrally during the
transition, but no subsystem should independently recreate validity rules.

## Population Rules

Startup-transient events are excluded from regression, phase statistics,
period statistics, rolling statistics, and plots.

Clean phase metrics include rows where:

```text
event disposition == Valid
```

Clean period metrics include an adjacent pair only when:

```text
previous disposition == Valid
current disposition == Valid
inferred tick step == 1
interval disposition == Normal
```

An anomalous event therefore cannot contaminate period standard deviation via
the interval before or after it.

Anomalies after startup are excluded from regression and clean metrics, but
remain visible in rows and receive a separate summary containing count, worst
phase residual, and worst interval residual with sequence/timestamp context.

## Reporting

Human-readable output uses neutral, observable terminology:

```text
Startup
  Transient events        ...
  Zero/duplicate backlog  ...
  Live anchor              seq ... / ... s
  Cadence confirmation     ... intervals
  Startup period estimate ...

Steady-state clock
  Valid clock events      ...
  Grid anomalies          ...

Steady-state jitter
  RMS                     ...
  P95                     ...
  P99                     ...
  Period standard dev.    ...

Anomalies
  Count                   ...
  Worst phase residual    ...
  Worst interval          ...
```

The report must not claim that startup events came from the ALSA-seq bridge
unless provenance is certain. `Zero/duplicate backlog` describes the observed
zero/negative-interval population, not its presumed cause.

JSON and CSV expose the authoritative disposition, interval disposition,
missing-before count, startup metadata, and anomaly details. Plots consume the
same analyzed rows and population predicates as the numerical report.

## CLI

```text
--startup-cadence <N>   default: 8 for PipeWire/ALSA-seq
--startup-cadence 0     disable cadence filtering
--settle <duration>     optional lower-bound constraint
```

The README documents cadence-based startup as the normal PipeWire/ALSA-seq
behavior and describes `--settle` as an explicit diagnostic constraint.

## Verification

Tests must cover:

- stale zero/negative startup burst followed by live events;
- anchor selection at the first event of an eight-interval confirmation run;
- cadence count zero preserving diagnostic behavior;
- settle boundary preventing earlier candidate anchors;
- tempo-varying period estimates with relative plausibility tolerance;
- valid phase rows versus clean adjacent period pairs;
- missing-before independent of event disposition;
- anomaly summaries and retained anomaly rows;
- plots, text, JSON, and CSV sharing the same population decisions;
- unchanged non-PipeWire defaults and existing fixtures.

The real Scarlett capture should show normal steady-state phase error around
0.1–0.2 ms and should no longer report the stale 3.1 ms event as a
steady-state extreme.
