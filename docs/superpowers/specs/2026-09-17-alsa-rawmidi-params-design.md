# ALSA RawMIDI Parameter Preservation Design

## Objective

Fix ALSA RawMIDI capture on devices that receive MIDI Clock bytes but produce
an empty midijitter capture. The Scarlett 18i20 at `hw:2,0,0` is the validated
reproduction device.

## Root Cause

The ALSA backend allocates a fresh `alsa::rawmidi::Params` object, changes only
the timestamp read mode and clock, then applies it. A freshly allocated ALSA
parameter object does not contain the opened stream's current buffer and wakeup
settings. Applying it overwrites those settings, preventing the capture loop
from receiving input.

## Design

`configure_clock` will obtain parameters with `Rawmidi::params_current`, then
set `ReadMode::Timestamp` and `Clock::MonotonicRaw` on that populated object
before applying it. This retains the ALSA-established input buffer settings.

After applying the parameters, the backend will read the effective parameters
and require timestamp mode with the monotonic-raw clock. A mismatch is reported
as timestamp mode unavailable; the existing explicit userspace fallback remains
the only alternative.

No CLI, capture-file, or analysis changes are needed. Kernel timestamped reads
remain the default and userspace timestamps remain opt-in.

## Validation

Automated tests will cover the configuration-result decision logic that is
independent of physical ALSA hardware. Full test, formatting, and Clippy checks
must pass. Hardware verification will record from `hw:2,0,0` while the source
transmits clock and confirm that the output capture contains Clock events with
kernel RawMIDI timestamp metadata.
