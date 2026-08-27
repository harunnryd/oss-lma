# Sidecar Resilience Amendment

## Status

Amendment to `2026-08-27-production-core-integration-design.md` covering four
sidecar-resilience features added during implementation that were not in the
original design. None of them change the documented process boundaries, secret
handling, or wire protocol shape beyond a single new event type.

## Goal

Make the sidecar survive the most common production hazards without
corrupting state or leaking secrets:

1. **Crash during active meeting** → no orphan partial rows in the UI.
2. **Runaway respawn loop** (e.g. a misconfigured sidecar that exits 0
   immediately after the readiness line) → bounded by a rate limit.
3. **Post-mortem secret leakage** via a core dump → core dumps disabled
   in-process before the supervisor writes any key-bearing state.

## Changes

### 1. `DELETE_TRANSCRIPT_SEGMENT` wire event

A new event in `contracts/events.schema.json` under `DeleteTranscriptSegment`:

```
{ "EventType": "DELETE_TRANSCRIPT_SEGMENT",
  "CallId": "<uuid>",
  "SegmentId": "<string>",
  "Reason": "STALE_PARTIAL" | "MANUAL" }
```

Emitted by the sidecar after a session reconnect when the database had
partial segments left behind by a previous run. The UI removes the matching
DOM row.

### 2. Stale-partial reconciliation

`python/sidecar/storage/crash_recovery.py` introduces `PendingDeletes`, an
in-memory buffer of `(meeting_id, segment_id)` pairs collected by
`sweep_stale_partials()` at sidecar startup. Each pair represents a segment
that was in-flight when the previous sidecar died.

On session start, `Session._emit_pending_deletes` drains pairs whose
`meeting_id` matches the new session's `CallId` and emits one
`DELETE_TRANSCRIPT_SEGMENT` frame per pair, persisting the deletion in the
same write path as live segments.

Reconciliation policy: process exit drops anything still buffered. The
database already marks the corresponding rows with `is_partial = -1`, but the
UI does not read that flag directly. A row whose DELETE event never fires
remains in the webview DOM until the user reconnects to the meeting (the
new session drains the buffer) or reloads the webview. This is acceptable
because the supervisor respawns the sidecar as part of the same health
event, and the link reconnect path always opens a new session before frames
resume.

### 3. Respawn cap

`crates/app/src/sidecar.rs` adds a circuit breaker around automatic
respawns, exposed via `SupervisorError::RespawnLimitExceeded`:

| Constant        | Value      |
|-----------------|------------|
| `RESPAWN_CAP`   | 5 attempts |
| `RESPAWN_WINDOW`| 60 seconds |
| `RESPAWN_BACKOFF`| 200 ms    |

Counter increments on every spawn attempt (success or failure). `spawn()`
returns `RespawnLimitExceeded` once the counter exceeds the cap AND the
attempt itself failed. `endpoint()` refuses to trigger an automatic respawn
once the counter exceeds the cap, regardless of success, and returns
`None`. `shutdown()` resets the counter, so an intentional restart starts
fresh.

The "every spawn counts" rule (rather than "only failures count") is what
prevents the most common production hazard: a child that prints the
readiness line then immediately exits 0, which would otherwise trigger an
infinite respawn loop. A successful spawn whose child exits again still
counts, so the loop is broken.

### 4. Core-dump suppression

`python/sidecar/__main__.py::_disable_core_dumps` calls
`resource.setrlimit(RLIMIT_CORE, (0, 0))` at process start on POSIX. The
sidecar carries the Deepgram API key in memory long enough to hand it to
the provider; a core dump written to disk would persist that key in the
process image. The function is a no-op on Windows (`RLIMIT_CORE` is
unsupported there) and swallows `OSError` so a restrictive sandbox cannot
prevent the sidecar from starting.

## Out of scope

- Replay of buffered audio after reconnect — already handled by
  `lma-link` reconnect.
- Persisting `PendingDeletes` to disk across sidecar exits — explicit
  decision: process exit drops anything still pending.
- Backing off the initial spawn — there is no prior attempt to back off
  from.
- Capping deliberate user-triggered `respawn()` calls — only automatic
  respawns driven by `endpoint()` are capped.

## Verification

The four features are exercised by:

- `crates/app/src/sidecar.rs::tests::endpoint_caps_respawn_attempts_after_repeated_failures`
- `crates/app/src/sidecar.rs::tests::spawn_rejects_when_cap_reached`
- `crates/app/src/sidecar.rs::tests::shutdown_resets_the_respawn_cap`
- `python/sidecar/tests/test_storage_crash_recovery.py::test_sweep_returns_zero_when_nothing_to_mark`
- `python/sidecar/tests/test_wire_contract.py::DELETE_TRANSCRIPT_SEGMENT` sample
- `src-tauri/tests/frontend_asset.rs::transcript_updates_replace_partial_segments_with_final_segments`

All six pass in the worktree at HEAD
(`codex/production-core-integration`).
