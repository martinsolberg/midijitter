# Remove Legacy PipeWire Stream Capture

Date: 2026-09-16
Status: approved

## Problem

The repository retains an older PipeWire capture implementation based on
`pw_stream`. That implementation does not provide usable graph timing on the
target system and is no longer a valid capture path. The working implementation
is the `pw_filter` transport in `filter_capture.rs`, which receives
`spa_io_position` directly and is already the default path.

Keeping both transports creates dead code, an unsupported CLI mode, duplicated
maintenance, and misleading documentation. The refactor removes the stream
architecture completely while preserving the filter implementation and leaving
its module name available as an explicit transport boundary for future work.

## Design

`PipeWireBackend::record` will directly call
`filter_capture::run_filter_capture`. The file
`src/backend/pipewire/capture.rs` will be deleted; consequently the repository
must contain no `pw_stream`, `pw_stream_get_time`, or stream transport symbols.
`filter_capture.rs` remains the native PipeWire transport module and continues
to use `common.rs` for capture parsing, timing, termination, and capture-file
assembly.

The backend request model will lose `PwApi`, `CaptureRequest.pw_api`, and the
`.pw_api` builder. Existing request behavior for termination, manual linking,
and ALSA userspace timestamp permission remains unchanged. PipeWire manual
connect continues to expose the filter input sink and use the existing link
state and liveness behavior.

The CLI will lose `--pw-api`, `PwApiArg`, its non-PipeWire validation, request
mapping, and API preamble lines. PipeWire output will continue to identify the
backend and selected source, but will not advertise a selectable transport.
README usage and development documentation will describe `pw_filter` as the
PipeWire implementation and remove stream comparison instructions. Historical
Git commits remain untouched.

## Error Handling and Compatibility

There is no compatibility mode for `--pw-api stream`; that option is
intentionally removed because the implementation is known not to work. The
filter path's existing errors, signal handling, manual-connect behavior, and
capture format are preserved. ALSA capture and analysis commands are outside
the scope of this change.

## Testing and Acceptance

- `cargo test` passes, including filter position-timing and backend tests after
  removing obsolete API-selection tests.
- `cargo clippy --all-targets -- -D warnings` passes.
- `cargo fmt --check` passes.
- Current source, active README usage, and active configuration contain no
  `pw_stream` usage; historical design and implementation records may retain
  references for provenance.
- `midijitter record --help` no longer lists `--pw-api`.
- The implementation is performed in a dedicated linked Git worktree under
  `.worktrees/`; the current worktree is not used for implementation changes.
