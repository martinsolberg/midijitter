# Remove Legacy PipeWire Stream Capture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the non-working `pw_stream` transport and its API-selection surface while retaining `filter_capture.rs` as the sole PipeWire capture transport.

**Architecture:** Delete `src/backend/pipewire/capture.rs` and route `PipeWireBackend::record` directly to `filter_capture::run_filter_capture`. Remove `PwApi` from the request model and `--pw-api` from the CLI, while preserving the filter transport, shared capture core, manual-connect behavior, capture format, and ALSA backend.

**Tech Stack:** Rust stable, Cargo, `pipewire` 0.10.1, clap, existing unit tests and CLI.

## Global Constraints

- Do not rename `filter_capture.rs`; retain it as the explicit transport module boundary for future PipeWire transports.
- Remove `src/backend/pipewire/capture.rs` completely.
- Remove all current `pw_stream`, `pw_stream_get_time`, `PwApi`, `pw_api`, and `--pw-api` implementation references.
- Historical Git commits and historical design/plan documents may retain references for provenance.
- Preserve the filter path's `spa_io_position` timing, error handling, signal handling, manual-connect behavior, and capture-file format.
- Preserve ALSA capture and analysis behavior.
- Implementation must occur in a dedicated linked Git worktree under `.worktrees/`; do not implement in the current worktree.
- Every implementation task must finish with `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check` passing before its commit.

---

## File Map

- Delete: `src/backend/pipewire/capture.rs` — obsolete `pw_stream` transport and `Stream::time()` timing path.
- Modify: `src/backend/pipewire/mod.rs` — remove the deleted module and simplify backend dispatch to the filter transport.
- Modify: `src/backend/mod.rs` — remove the `PwApi` enum, request field, builder, and API-selection tests.
- Modify: `src/cli.rs` — remove `PwApiArg`, `--pw-api`, validation, request mapping, and API preamble output.
- Modify: `README.md` — document filter-only PipeWire capture and remove stream comparison instructions.
- Add: `.worktrees/<branch>` — linked implementation worktree created before code changes; its exact directory and branch are chosen by the coding agent.

### Task 1: Create Isolated Implementation Worktree

**Files:**
- Create: `.worktrees/remove-pw-stream` as a linked Git worktree, or another clearly named `.worktrees/` path if unavailable.

**Interfaces:**
- Consumes: current `main` branch and the committed design at `docs/superpowers/specs/2026-09-16-remove-pw-stream-design.md`.
- Produces: dedicated branch `refactor/remove-pw-stream` checked out in the linked worktree.

- [ ] **Step 1: Inspect worktree state**

Run from the repository root:

```bash
git status --short
git worktree list
```

Do not alter or discard the current worktree's unrelated changes (`capture.json`, settle-window plan, or existing source edits).

- [ ] **Step 2: Create the linked worktree and branch**

Run:

```bash
git worktree add -b refactor/remove-pw-stream .worktrees/remove-pw-stream main
```

If that path is already occupied, choose a new path under `.worktrees/` and record it in the task output. All subsequent implementation commands must use the linked worktree as their working directory.

- [ ] **Step 3: Verify isolation**

Run in the linked worktree:

```bash
git branch --show-current
git status --short
git worktree list
```

Expected: branch `refactor/remove-pw-stream`, clean worktree, and a separate entry under `.worktrees/`.

- [ ] **Step 4: Commit**

No code commit is needed for this setup task. Keep the worktree clean and continue with Task 2 from that directory.

### Task 2: Remove the Stream Transport and Request API

**Files:**
- Delete: `src/backend/pipewire/capture.rs`
- Modify: `src/backend/pipewire/mod.rs:1-25`
- Modify: `src/backend/mod.rs:25-79,118-167`

**Interfaces:**
- Consumes: `filter_capture::run_filter_capture(&CaptureRequest)` and `common.rs` unchanged.
- Produces: `PipeWireBackend::record(request)` directly invokes the filter transport; `CaptureRequest` contains no PipeWire API selector.

- [ ] **Step 1: Remove the obsolete module and dispatch branch**

Delete `src/backend/pipewire/capture.rs`. In `src/backend/pipewire/mod.rs`, remove `mod capture;`, remove `PwApi` from the imports, and replace the match-based implementation with:

```rust
fn record(&self, request: CaptureRequest) -> Result<CaptureFile, AppError> {
    filter_capture::run_filter_capture(&request)
}
```

- [ ] **Step 2: Remove request-level API selection**

In `src/backend/mod.rs`, delete the `PwApi` enum, delete `CaptureRequest.pw_api`, remove its initialization in `CaptureRequest::new`, and delete the `.pw_api` builder. Keep all other fields and builders unchanged.

- [ ] **Step 3: Remove obsolete backend tests**

Delete `capture_request_defaults_to_filter_api`, `capture_request_builder_selects_stream_api`, and `manual_connect_composes_with_either_pipewire_api`. Retain the positive termination test and all source/request behavior tests unrelated to API selection.

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test backend::
```

Expected: compilation succeeds and all remaining backend tests pass.

- [ ] **Step 5: Run the full quality gate**

Run:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Expected: all commands pass.

- [ ] **Step 6: Commit**

```bash
git add src/backend/pipewire/capture.rs src/backend/pipewire/mod.rs src/backend/mod.rs
git commit -m "refactor: remove legacy PipeWire stream transport"
```

### Task 3: Remove the CLI Selection Surface

**Files:**
- Modify: `src/cli.rs:27-56,138-142,147-165,225-310`

**Interfaces:**
- Consumes: simplified `CaptureRequest` with no `.pw_api` builder.
- Produces: `record` has no `--pw-api`; PipeWire output identifies the backend and source without an API label.

- [ ] **Step 1: Remove CLI API types and argument plumbing**

Delete the `pw_api` field from `Command::Record`, delete `PwApiArg`, remove the `pw_api` parameter from `run_record`, and remove the corresponding match destructuring and call argument in `run()`.

- [ ] **Step 2: Remove validation and request mapping**

Delete the `--pw-api` validation block and the `(backend, pw_api)` request mapping. Construct the request using the existing termination, userspace-timestamp, and manual-connect logic only.

- [ ] **Step 3: Remove API preamble output**

Delete `api_label` and both `println!("API: ...")` lines. Keep the PipeWire backend/source/manual-connect messages unchanged otherwise.

- [ ] **Step 4: Add CLI regression checks**

Run:

```bash
cargo run --quiet -- record --help
cargo run --quiet -- record --help | rg -- '--pw-api|pw_stream'
```

Expected: the first command prints help; the second prints no matches and exits successfully only if the pipeline is guarded appropriately, for example with `! cargo run --quiet -- record --help | rg -- '--pw-api|pw_stream'`.

- [ ] **Step 5: Run the full quality gate**

Run:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Expected: all commands pass.

- [ ] **Step 6: Commit**

```bash
git add src/cli.rs
git commit -m "refactor: remove PipeWire API selection from CLI"
```

### Task 4: Update Active Documentation and Verify Removal

**Files:**
- Modify: `README.md:81-96,98-129`
- Verify: current Rust source, README, Cargo manifests, and active docs/plans excluding historical records.

**Interfaces:**
- Consumes: filter-only CLI and backend.
- Produces: user-facing documentation that describes the supported PipeWire implementation accurately.

- [ ] **Step 1: Rewrite PipeWire usage documentation**

Replace the `### PipeWire capture API` section with a filter-only description and example that omits `--pw-api`. State that `pw_filter` receives graph position in the process callback and timestamps events using graph cycle position plus event offset. Remove claims that `stream` is available for comparison.

- [ ] **Step 2: Correct manual-connect wording**

Keep the manual-connect procedure, but remove wording that discusses filter ports as one of multiple API modes. Ensure the documented command remains valid without `--pw-api`.

- [ ] **Step 3: Search for active references**

Run:

```bash
rg -n 'pw_stream|pw_stream_get_time|PwApi|pw_api|--pw-api' src README.md Cargo.toml Cargo.lock docs/spec docs/superpowers/plans docs/superpowers/specs
```

Expected: no matches in current source, README, manifests, or active documentation. Historical design/plan records may be updated only if they are intended to describe current behavior; do not rewrite historical Git commits.

- [ ] **Step 4: Run final quality and CLI checks**

Run:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
! cargo run --quiet -- record --help | rg -- '--pw-api|pw_stream'
git status --short
```

Expected: all quality checks pass, help has no removed option, and status lists only intended files.

- [ ] **Step 5: Commit**

```bash
git add README.md
git commit -m "docs: document filter-only PipeWire capture"
```

### Task 5: Final Worktree Handoff

**Files:**
- Verify: all commits and files in the linked worktree.

**Interfaces:**
- Consumes: Tasks 1-4.
- Produces: reviewable branch containing the complete refactor, with the main worktree untouched.

- [ ] **Step 1: Inspect final diff and history**

Run in the linked worktree:

```bash
git status --short
git log --oneline --decorate main..HEAD
git diff --stat main...HEAD
git diff --check main...HEAD
```

Expected: clean worktree, only intended refactor commits, no whitespace errors, and deletion of the stream module visible in the diff.

- [ ] **Step 2: Confirm worktree isolation**

Run from the main repository:

```bash
git worktree list
git status --short
```

Expected: the implementation branch remains under `.worktrees/`, and the main worktree's pre-existing changes remain untouched.

- [ ] **Step 3: Report handoff**

Return the linked worktree path, branch name, commit list, quality-gate results, and any live PipeWire validation that could not be run because no daemon or MIDI source was available.

## Self-Review

- Spec coverage: the obsolete file and dispatch are removed in Task 2; request and CLI selectors are removed in Tasks 2-3; README is updated in Task 4; filter behavior, ALSA behavior, and historical records are explicitly preserved; worktree isolation is enforced in Tasks 1 and 5.
- Placeholder scan: no TBD, TODO, or unspecified implementation steps remain; every code change names exact symbols or replacement behavior and every task includes commands.
- Type consistency: Task 2 removes `PwApi` before Task 3 removes its CLI conversion; the existing `run_filter_capture(&CaptureRequest)` signature remains unchanged; `PipeWireBackend::record` passes its owned request by reference as required.
- Scope: this is one focused subsystem refactor with separate backend-model, CLI, and documentation tasks that each have independent compile/test gates.
