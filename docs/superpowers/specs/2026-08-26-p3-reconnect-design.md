---
title: "P3 — Reconnect and Time-Offset Continuity Design"
date: 2026-08-26
---

# P3 — Reconnect and Time-Offset Continuity Design

The sidecar's `Session._pump` already catches `ProviderResetError` from the STT adapter and emits an `STT_STREAM_RESET` ERROR frame (P1). After the frame, the pump ends. This phase adds the auto-reconnect loop, the cumulative time-offset, and the resume-on-sidecar-restart behavior that the wire protocol and `contracts/errors.yaml` already promise.

## Goals

1. **Auto-reconnect within the sidecar** when the STT provider drops. The client WebSocket stays open; the sidecar transparently re-establishes the STT stream and resumes emitting transcript segments.
2. **Cumulative time offset** — every segment timestamp (both wire and DB) is offset-adjusted so the client sees a single continuous timeline across reconnects. A meeting interrupted by N reconnects has segment timestamps that increase monotonically from the first START.
3. **Resume across sidecar restarts** — a sidecar process killed mid-meeting can be restarted; the next START frame with the same `CallId` resumes the meeting from its persisted offset.
4. **Backoff policy matches `contracts/errors.yaml`** — `STT_STREAM_RESET.limits`: `max_consecutive=5`, `backoff_start_ms=500`, `backoff_ceiling_ms=10000`, `reset_after_session_seconds=10`. Counter resets on first successful result OR after 10s without a failure.
5. **Budget exhaustion is fatal** — after `max_consecutive` failures, the sidecar closes the WebSocket (close code `1013` "try again later"), writes `meetings.status='FAILED'` and the END row, and stops. The client can send a fresh START to begin a new meeting (or to manually resume — see §4).

## Non-goals (out of scope for P3)

- Cross-meeting rate limiting (P3's policy is per-meeting; a future phase can add a sidecar-wide coordinator if needed).
- Reconnect across sidecar processes is handled by §4 resume, not by a "shared state" mechanism.
- A new wire-protocol event for resume signaling (silent resume is sufficient — the client sees continuous timestamps either way).
- The reconnect *reason* (e.g., "Deepgram sent `Error` frame") is logged at debug level only; it does not change the wire contract.

## Architecture

### Components

- **`sidecar/reconnect.py`** (new): defines
  - `ReconnectPolicy` (frozen dataclass, populated at module load from `errors.yaml` for `STT_STREAM_RESET`).
  - `ReconnectState` (per-Session mutable state — `consecutive_failures: int`, `next_backoff_ms: int`, `last_failure_at_ms: int | None`).
  - `next_backoff_ms(state) -> int` — computes the next backoff cap-aware.
  - `record_failure(state, now_ms) -> None` — increments counter, schedules next backoff.
  - `record_success(state) -> None` — resets counter to zero.
  - `maybe_reset_on_idle(state, now_ms) -> None` — if `now_ms - last_failure_at_ms >= reset_after_session_seconds`, reset counter to zero.
- **`Session`** (modified): gains `self.time_offset_ms: int = 0` and `self.reconnect_state: ReconnectState`. `_pump` is restructured to execute the reconnect loop inline (see §3). Two new private methods: `_reconnect_once` and `_stream_elapsed_ms`.
- **Migration `003_reconnect_state.sql`** (new): adds three columns to `meetings`:
  - `time_offset_ms INTEGER NOT NULL DEFAULT 0`
  - `reconnect_attempts INTEGER NOT NULL DEFAULT 0`
  - `last_reconnect_at INTEGER` (nullable)
- **`writer_boundary.normalize_meeting_started`** (modified): on START, if the row already exists, return the existing `time_offset_ms` instead of `0`. This is the only normalization change.
- **`writer_boundary.normalize_segment`** (modified): accept an additional `time_offset_ms: int = 0` parameter; returned tuple's `start_ms` and `end_ms` are pre-offset-adjusted. The signature change is non-breaking because every caller in `Session._pump` and the integration tests passes the offset.
- **`writers.write_meeting_started`** (modified): `INSERT OR IGNORE` semantics (not `INSERT OR REPLACE`) so an existing meeting row's `time_offset_ms` and `reconnect_attempts` are preserved across restart. The writer returns the meeting row's current `time_offset_ms` so the caller (Session) can pick it up. Signature: `def write_meeting_started(conn, ev, *, return_offset: bool = False) -> int | None`.
- **`writers.write_meeting_ended`** (modified): also writes `time_offset_ms` (snapshot at END time) and updates `status` to `COMPLETED`.

### Module-level dependencies (no circulars)

```
errors.yaml ──► sidecar/reconnect.py (ReconnectPolicy)
               └─► sidecar/session.py (ReconnectState, inline pump)
                    └─► sidecar/storage/writer_boundary.py (offset-aware normalize)
                         └─► sidecar/storage/writers.py (offset-aware write)
                              └─► sidecar/storage/migrations/003_reconnect_state.sql
```

`sidecar/reconnect.py` imports `yaml` (already in dev deps via P1) and nothing from `lma_stt`/`lma_pipeline` — it is a pure data module. `Session` imports `ReconnectState` and `ReconnectPolicy` from there. The writer/writer-boundary changes are local to the storage package.

## Reconnect loop (Session._pump restructured)

The current `_pump` is a single `async for result in stream: ... except` chain. The new `_pump` is a state machine with the same outer signature `(stream, assembler) -> None`:

```python
async def _pump(self, stream, assembler) -> None:
    self.reconnect_state = ReconnectState()
    current_stream = stream
    while True:
        try:
            async for result in current_stream:
                self.reconnect_state.maybe_reset_on_idle(now_ms())
                if self.reconnect_state.consecutive_failures > 0:
                    self.reconnect_state.record_success()
                for event in assembler.on_result(result):
                    adjusted = self._apply_offset(event, self.time_offset_ms)
                    if self.db is not None:
                        self.db.write(adjusted)
                    await self._send(adjusted)
            return
        except ProviderResetError as exc:
            consecutive = self.reconnect_state.consecutive_failures + 1
            if consecutive > ReconnectPolicy.max_consecutive:
                await self._fail_meeting(exc)
                return
            self.reconnect_state.record_failure(now_ms())
            backoff = self.reconnect_state.next_backoff_ms
            logger.warning(
                "stt provider reset for call %s (attempt %d/%d), retrying in %dms",
                self.call_id, consecutive, ReconnectPolicy.max_consecutive, backoff,
            )
            await self._send(error_frame(self.call_id, "STT_STREAM_RESET", {"attempt": consecutive}))
            await asyncio.sleep(backoff / 1000)
            try:
                new_stream = await self.engine_factory(self._ctx()).start(self._ctx())
            except Exception as reconnect_exc:
                logger.exception("reconnect attempt %d failed", consecutive)
                continue
            elapsed_ms = await self._stream_elapsed_ms(current_stream)
            self.time_offset_ms += elapsed_ms
            self.db.write_meeting_started(...)  # updates time_offset_ms, reconnect_attempts
            current_stream = new_stream
            continue
        except (ProviderAuthError, ConnectionClosed, sqlite3.DatabaseError, Exception):
            # existing arms, unchanged
            ...
```

Key invariants:

- `_pump` runs the reconnect loop in a single task — no new supervisor task.
- Backoff sleep is `asyncio.sleep(backoff / 1000)`, not `time.sleep` — the event loop stays responsive.
- Time-offset is incremented by the **dead** stream's elapsed duration. `_stream_elapsed_ms` reads the wall-clock delta from when the engine was started to when the reset fired (stored in a private `self._stream_started_at_ms` updated at every (re)start).
- The reconnect attempt counter is **only incremented on failure**. Success decrements / resets (via `record_success`).
- The `max_consecutive=5` check uses strict `>` so the 5th failure still gets a retry attempt; the 6th consecutive failure (after the 5th retry also failed) is the one that exhausts the budget.

### Backoff schedule

`next_backoff_ms` doubles on each failure, capped at `backoff_ceiling_ms`:

| consecutive | backoff |
|---|---|
| 1 | 500 |
| 2 | 1000 |
| 3 | 2000 |
| 4 | 5000 |
| 5 | 10000 (cap) |

If the 5th retry also fails, the 6th failure triggers `_fail_meeting`. The `reset_after_session_seconds=10` fallback resets the counter if no failure has occurred for 10s — `maybe_reset_on_idle` is called at the top of every iteration of the outer `async for`.

### Test seam for backoff timing

A `Clock` callable is injected into `ReconnectState` (constructor default = `lambda: int(time.time() * 1000)`). Tests pass `lambda: <fixed_ms>` to make backoff schedule deterministic without real-time sleeps. The `asyncio.sleep` in `_pump` is also injected via a `sleep` callable on `Session` (default = `asyncio.sleep`) so tests can use `lambda _seconds: asyncio.sleep(0)` to skip the wait without losing the test's correctness signal.

## Wire timestamp adjustment (the continuous-timeline invariant)

`SegmentAssembler` is unchanged — it still emits raw STT timestamps (float seconds from THIS stream's start). The transformation happens at the wire/DB write boundary in `_pump`:

```python
def _apply_offset(self, event: dict, offset_ms: int) -> dict:
    if offset_ms == 0 or event.get("EventType") != "ADD_TRANSCRIPT_SEGMENT":
        return event
    return {
        **event,
        "StartTime": round((event["StartTime"] * 1000 + offset_ms)) / 1000,
        "EndTime": round((event["EndTime"] * 1000 + offset_ms)) / 1000,
    }
```

`offset_ms == 0` fast-path preserves the original wire values bit-for-bit when no offset has accumulated (no client-visible change for fresh meetings, no float-precision drift). Non-segment events (START, SUMMARY, AGENT_ASSIST, etc.) pass through unchanged.

The DB side: `writer_boundary.normalize_segment` takes the same `time_offset_ms` parameter and returns pre-offsetted `start_ms`/`end_ms`. `Session._pump` passes the same `self.time_offset_ms` to both the wire emit and the DB write, so the two stores agree on the same absolute timeline.

**Wire-protocol clarification (no schema change)**: `contracts/events.schema.json`'s `notes` text reads "Timestamps on the wire are float seconds from stream start." After this phase, that note is updated to read "Timestamps on the wire are float seconds from the meeting's first START for this CallId. The sidecar adjusts timestamps by `time_offset_ms` on reconnect so the timeline is continuous." No `events.schema.json` JSON-structure change.

## Resume across sidecar restarts

`write_meeting_started` becomes `INSERT OR IGNORE` — the second START with the same `CallId` does NOT overwrite the existing row. `Session._start_session` then reads back the existing `time_offset_ms` and `status`:

```python
async def _start_session(self, frame):
    await self._close_session(drain=True)
    ...
    if self.db is not None:
        existing_offset = self.db.write_meeting_started(
            {"EventType": "START", "CallId": frame.call_id},
            return_offset=True,
        )
        self.time_offset_ms = existing_offset or 0
```

If `existing_offset` is non-zero, the sidecar resumes — STT connects, raw timestamps are offset-adjusted by `self.time_offset_ms`, and segments continue the timeline from where the previous run left off. If the previous meeting was `status='FAILED'` (i.e., reconnect budget was exhausted before the previous sidecar exited), the new START is a fresh retry — `INSERT OR IGNORE` still preserves the row, but the resume reads back `time_offset_ms` (which is whatever it was at the time of failure) and the new attempt picks up. A comment in `Session._start_session` notes this: "START with same CallId resumes. If the previous attempt was FAILED, this is a new attempt with offset preserved but reconnect_attempts reset to 0."

A `sweep_stale_partials`-style consistency check is NOT added in P3 — that runs only at sidecar startup, not at every START. Rationale: the per-call-id meeting row is loaded lazily from `meetings` on each START, so the most recent `time_offset_ms` is always the source of truth.

### Wire side: silent resume

No new wire event. The client sees segments with timestamps continuing from the first START, with no discontinuity. If the client tracks its own timeline (most do), it gets a continuous stream.

## Budget exhaustion — what happens to the client

When `consecutive > max_consecutive`:

```python
async def _fail_meeting(self, exc):
    logger.error("reconnect budget exhausted for call %s: %s", self.call_id, exc)
    if self.db is not None:
        self.db.write_meeting_failed({"EventType": "FAILED", "CallId": self.call_id, "Reason": str(exc)})
        self.db.write_meeting_ended({"EventType": "END", "CallId": self.call_id})
    try:
        await self.connection.close(1013, "stt-reconnect-exhausted")
    except Exception:
        pass
```

The client receives:
1. A final `ERROR` frame with `Code="STT_STREAM_RESET"`, `Context.attempts=6` (one more than `max_consecutive`).
2. A WebSocket close with code `1013` "try again later" and reason `"stt-reconnect-exhausted"`.
3. The DB writes the meeting as `status='FAILED'` (the new `write_meeting_failed` write) and the END row.

The client can now either start a fresh meeting (new `CallId`) or send a new START with the same `CallId` to manually retry — the new attempt resets `reconnect_attempts` to 0 but preserves `time_offset_ms` (so any segments that DID make it into the DB during the failed attempt are still on the timeline).

## Migration

`python/sidecar/storage/migrations/003_reconnect_state.sql`:

```sql
ALTER TABLE meetings
  ADD COLUMN time_offset_ms INTEGER NOT NULL DEFAULT 0,
  ADD COLUMN reconnect_attempts INTEGER NOT NULL DEFAULT 0,
  ADD COLUMN last_reconnect_at INTEGER;
```

No `DROP TABLE`. No FK impact (no other table references `meetings` columns). Compatible with `PRAGMA foreign_keys=ON`. The migration runner (P2 Task 3) handles new SQL files automatically.

## Error semantics

- **ProviderResetError caught and recovered**: reconnect loop runs, sidecar continues. Wire client sees one ERROR frame per failed attempt with `attempts=N`, but the WebSocket stays open.
- **All backoff retries exhausted**: WebSocket closes with 1013, meeting marked FAILED in DB, END row written.
- **STT provider auth failure (`ProviderAuthError`)**: STT_PROVIDER_AUTH is `fatal-stream` per `errors.yaml` — no reconnect, immediate ERROR + close (P1's behavior, unchanged).
- **STT provider connection-time failure (e.g., factory raises)**: treated like `ProviderResetError` for the backoff counter (increment + retry), but the client's WebSocket stays open across all retries.
- **WebSocket `ConnectionClosed` mid-reconnect**: `_close_session` is called from `run()`'s `finally`; meeting stays in `status='RECORDING'` (will be marked stale by `sweep_stale_partials` on next sidecar startup).
- **Sidecar restart mid-reconnect (process killed)**: meeting row's `time_offset_ms` and `reconnect_attempts` persist; next sidecar's START with same `CallId` resumes from the persisted offset.

## Testing strategy

- **Unit tests for `ReconnectState`**: backoff schedule, counter reset on success, idle-reset on `reset_after_session_seconds`, `max_consecutive` boundary behavior. Use injected `Clock`.
- **Unit tests for `_apply_offset`**: identity at offset=0, monotonic guarantee, non-segment events pass through.
- **Integration test: reconnect success path**: `ScriptedEngine` configured to raise `ProviderResetError` once, then succeed. Assert: (a) client receives `ERROR` frame with `attempts=1` once; (b) subsequent segments are offset-adjusted; (c) DB rows for segments have offset-adjusted `start_ms`/`end_ms`; (d) `meetings.reconnect_attempts == 1` after reconnect; (e) `meetings.time_offset_ms` matches elapsed duration of the dead stream.
- **Integration test: budget exhaustion path**: `ScriptedEngine` configured to always raise `ProviderResetError`. Assert: (a) 5 ERROR frames sent (attempts 1-5); (b) final ERROR with `attempts=6`; (c) WS closed with code 1013; (d) `meetings.status='FAILED'`; (e) END row written.
- **Integration test: resume across sidecar restart**: spin up a sidecar, drive a few segments, kill the sidecar process, restart, START with same CallId, assert time_offset_ms is loaded, new segments continue the timeline, reconnect_attempts reset to 0.
- **P1+P2 regression**: existing 210 tests must stay green. The wire emission path is byte-equivalent when `time_offset_ms == 0`.

## Open questions deferred to spec-writing

None. Every architectural decision is captured above. Implementation-level choices (Clock injection shape, exact test fixture paths) will be settled at code-writing time per the plan's TDD discipline.