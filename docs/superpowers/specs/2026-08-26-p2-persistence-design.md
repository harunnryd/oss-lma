---
title: "P2 — Sidecar Persistence Design"
date: 2026-08-26
---

# P2 — Sidecar Persistence Design

The Python sidecar writes everything it produces (transcript segments, summaries, agent assists, agent tokens, thinking steps, action items, rag chunks, VP tasks) to a local SQLite database that the Rust shell also reads from. P1 finished a sidecar that emits these events over WebSocket but discards them. P2 makes them durable.

## Goals

1. The Rust shell and the Python sidecar share one SQLite file (`<app-data>/lma.db`) under WAL mode. No table is written by both processes.
2. Every event the sidecar emits over WebSocket is also written to SQLite before the sidecar sends it to the client. The DB and the wire stream are kept in sync.
3. The schema and the writer layer match the DDL already documented in `docs/database-schema.md` — we are implementing that doc, not redesigning it.
4. Recordings are written as raw PCM passthrough (stereo s16le 48 kHz, no resampling) alongside the DB row that references them.
5. Crash recovery: a partial segment whose `is_final` never arrived is detected at startup via a stale-partials sweep and marked accordingly.

## Non-goals (out of scope for P2)

- Reconnect / backoff policy that consumes the persisted state — that lands in P3.
- Provider adapters that fill the persisted rows from real STT APIs — that lands in P4.
- Summary/agent logic that *generates* the rows this spec writes — segments and summaries are written from existing P1 emitters; the *generation* of summaries is P5.
- Vector-store ingest of persisted segments — P7.
- Virtual Participant execution — P8.
- Rust-owned tables (`settings`, `prompt_templates`, `mcp_servers`, `vp_schedules`) — P9.
- Any recording format other than PCM passthrough — Opus/AAC/Opus-WebM come later if at all.

## Architecture

```
                ┌────────────────────────────────────────────────────┐
                │  Sidecar process                                   │
                │                                                    │
  WS frames ─►  │  Session._pump ──┬──► assembler.on_result          │
                │                  │                                 │
                │                  └──► PersistenceWriter.write(ev)  │ ─► SQLite (WAL)
                │                       │                            │
                │                       └──► RecordingSink.feed(pcm)│ ─► <app-data>/recordings/<meeting_id>/audio.wav
                │                                                    │
                │                  ┌──► _send(event)                 │ ─► WebSocket frames
                └──────────────────┴─────────────────────────────────┘
```

Two new seams into `Session`:

1. `PersistenceWriter` — a `Protocol` with one method `write(event: dict) -> None`. `Session` calls it after the assembler emits, before `_send`. Default is `None` (no persistence, matching today's behavior — keeps existing tests green).
2. `RecordingSink` — a `Protocol` with one method `feed(pcm: bytes) -> None`. `Session` calls it from `on_binary` *after* the engine's `stream.feed(pcm)` succeeds and *only* while recording is enabled for the current meeting. Default is `None`.

Both seams take `None` so the existing P1 test suite (which builds `ScriptedEngine` / `MemoryConnection` and never provides either) keeps passing unchanged.

## Component specifications

### 1. SQLite layer (`python/sidecar/storage/`)

New package: `python/sidecar/storage/` containing five modules:

- `connection.py` — opens `<app-data>/lma.db`, sets `journal_mode=WAL`, `foreign_keys=ON`, `busy_timeout=5000`. Returns a `sqlite3.Connection`. Single function `open_db(path: Path) -> sqlite3.Connection`.
- `migrations.py` — discovers and applies migrations in `python/sidecar/migrations/`, records applied versions in a `schema_version` table (id INTEGER PK, version INTEGER, applied_at INTEGER). Runs in transactions; on failure rolls back and aborts startup. Single function `apply_migrations(conn, dir: Path) -> list[int]` returning the versions applied in this run.
- `writers.py` — one function per event type: `write_meeting_started(conn, event)`, `write_segment(conn, event)`, `write_summary(conn, event)`, `write_agent_assist(conn, event)`, `write_agent_token(conn, event)`, `write_thinking_step(conn, event)`, `write_meeting_ended(conn, event)`. Each takes a `sqlite3.Connection` and an event dict (the same dict that goes over WebSocket). Each does the wire-to-DB field normalization (see §5 below) and runs an INSERT or UPDATE.
- `writer_boundary.py` — the wire-to-DB field normalization module. Single function `normalize_for_table(table: str, event: dict) -> tuple` per table that returns the column values in declaration order. Every writer goes through this. See §5.
- `recording.py` — `RecordingSink` Protocol implementation backed by a `wave` module write to `<app-data>/recordings/<meeting_id>/audio.wav`. Opens the file at START, appends 19,200-byte chunks (raw PCM s16le stereo 48 kHz, no header manipulation beyond the initial `wave.open` call), closes at END.

### 2. Migration files

Directory: `python/sidecar/migrations/`. One initial file:

```
001_initial.sql
```

Contents: the complete Python-owned DDL verbatim from `docs/database-schema.md` (§ "Tables" through end of `action_items`, `rag_chunks`, `vp_tasks`). The four Rust-owned tables are NOT included — they migrate separately when the Rust shell phase lands.

`schema_version` table is created by the migration runner itself (id INTEGER PK, version INTEGER, applied_at INTEGER), not by the SQL files, so it exists before any migration runs.

### 3. Migration runner behavior

- Discovers files matching `NNN_*.sql` in the migrations dir, sorts by `NNN` as integer.
- For each not-yet-applied version, opens a transaction, runs the SQL, records the version in `schema_version`. On any exception the transaction rolls back and the runner raises — sidecar refuses to start (exit 1) rather than boot in a half-migrated state.
- Applied migrations are recorded with `applied_at = unix ms` so debugging can correlate migration timing with logs.
- Idempotent: re-running with no new files is a no-op (verified by a test).

### 4. PersistenceWriter Protocol

```python
class PersistenceWriter(Protocol):
    def write(self, event: dict) -> None: ...
```

Implementations:

- `SqliteWriter(conn: sqlite3.Connection)` — production implementation. Dispatches on `event["EventType"]` to the right `writers.write_*` function. Raises on any DB error; `Session._pump`'s existing exception handling maps that to one `ERROR {Code: "DB_WRITE_CONFLICT"}` frame per `errors.yaml`.
- `NullWriter()` — no-op. Used by every existing P1 test that does not need persistence.

`Session.__init__` gains one new keyword arg `db: PersistenceWriter | None = None`. When `None`, the existing behavior is preserved exactly. When provided, `_pump` calls `self.db.write(event)` between `assembler.on_result` and `_send`.

### 5. Writer boundary (timestamp / type normalization)

All wire-to-DB field conversions live in `writer_boundary.py`. Each `normalize_*` function returns a tuple of column values in the table's declared column order. Single source of truth so any future field type change touches one function per table.

Conversions performed:

| Wire field (type on the wire) | DB column | Conversion |
|---|---|---|
| `StartTime`, `EndTime` (float seconds) | `start_ms`, `end_ms` (INTEGER) | `int(round(value * 1000))` |
| `CallId` (UUID string) | `id` / `meeting_id` / etc. (TEXT) | passthrough |
| `Speaker` (string, or absent) | `speaker` (TEXT NULL) | `None` if absent, else passthrough |
| `IsPartial` (bool) | `is_partial` (INTEGER) | `1 if value else 0` |
| `SentimentScore` (float, optional) | `sentiment_score` (REAL NULL) | passthrough or `None` |
| `text` on segments (string) | `text`, `original_text` (TEXT) | both set to the same value (live translation deferred — when `transcription-and-translation.md`'s translation feature lands, `text` holds the translated display line and `original_text` the source) |
| UUIDs (`CallId`, `QueryId`, `SegmentId`) | (TEXT) | passthrough, no reformatting |
| Channel enum (`CALLER` / `AGENT` / `AGENT_ASSISTANT`) | `channel` (TEXT) | passthrough — the `CHECK` constraint enforces validity |
| Status enum (`RECORDING` / `FINALIZING` / `COMPLETED` / `FAILED`) | `status` (TEXT) | passthrough — same |

### 6. Recording sink

```python
class RecordingSink(Protocol):
    def feed(self, pcm: bytes) -> None: ...
```

Implementations:

- `WavRecordingSink(path: Path)` — production. Opens `<app-data>/recordings/<meeting_id>/audio.wav` at session start with `wave.open(path, "wb")` setting `nchannels=2`, `sampwidth=2`, `framerate=48000`. Calls `feed(pcm)` to `wave.writeframes(pcm.raw)`. Closes on `stop()` (called at session end).
- `NullRecordingSink()` — no-op. Default for every existing test and for sidecar runs that haven't opted into recording (see §8 below).

### 7. Session integration

`Session.__init__` signature becomes:

```python
def __init__(
    self,
    connection,
    engine_factory,
    *,
    db: PersistenceWriter | None = None,
    recorder: RecordingSink | None = None,
):
```

Behavior on `_pump`:

```python
async def _pump(self, stream, assembler) -> None:
    try:
        async for result in stream:
            events = assembler.on_result(result)
            for event in events:
                if self.db is not None:
                    self.db.write(event)
                if self.recorder is not None and event["EventType"] == "ADD_TRANSCRIPT_SEGMENT":
                    pass  # recording happens in on_binary, not here
                await self._send(event)
    except ProviderAuthError:
        ...
```

Behavior on `on_binary` (recording only):

```python
async def on_binary(self, pcm: bytes) -> None:
    if self.stream is None: ...
    if len(pcm) != self.chunk_bytes: ...
    if self.paused: return
    await self.stream.feed(pcm)
    if self.recorder is not None:
        self.recorder.feed(pcm)
```

`DB_WRITE_CONFLICT` mapping: `Session._pump`'s existing `except (ProviderAuthError, ProviderResetError, Exception)` chain gains a `except sqlite3.DatabaseError` arm that sends one `ERROR` frame with `Code="DB_WRITE_CONFLICT"` before returning. This reuses the existing ERROR-frame machinery (`error_frame`, `_send`) — DB errors propagate to the client exactly the same way provider errors do today.

### 8. Configuration (opt-in recording)

Recording is opt-in via env var `LMA_RECORD_MEETING=1` in `__main__.py`. Default off (matches today's behavior — most sidecar runs do not record). When on, `__main__` constructs a `WavRecordingSink` per session from the meeting ID and passes it to `Session`. DB writes happen regardless of recording (persistence is the spec, recording is the cherry on top).

`__main__.py` also opens the DB connection at startup, applies migrations, and constructs one `SqliteWriter` reused across all sessions (per the full-lifetime connection decision).

### 9. Crash recovery: stale-partials sweep

At startup, after migrations apply, the sidecar runs:

```sql
UPDATE segments SET is_partial = -1
WHERE meeting_id IN (SELECT id FROM meetings WHERE status = 'RECORDING')
  AND is_partial = 1;
```

`is_partial = -1` is the documented "stale" sentinel (it's currently `CHECK IN (0, 1)`, so this sweep requires a migration `002_stale_partial_sentinel.sql` that loosens the constraint to `CHECK IN (0, 1, -1)`). The dashboard / replay view (P9 Rust UI) renders stale segments differently from live ones.

This sweep is the only crash-recovery story for P2. It does not delete orphaned segments; it just marks them. A separate "purge stale after N hours" is out of scope.

### 10. Files we touch / files we create

New files:

```
python/sidecar/storage/__init__.py
python/sidecar/storage/connection.py
python/sidecar/storage/migrations.py
python/sidecar/storage/writers.py
python/sidecar/storage/writer_boundary.py
python/sidecar/storage/recording.py
python/sidecar/migrations/001_initial.sql
python/sidecar/migrations/002_stale_partial_sentinel.sql
python/sidecar/tests/test_storage.py
python/sidecar/tests/test_storage_connection.py
python/sidecar/tests/test_storage_migrations.py
python/sidecar/tests/test_storage_writer_boundary.py
python/sidecar/tests/test_storage_recording.py
python/sidecar/tests/test_persistence_session_integration.py
```

Modified files:

```
python/sidecar/session.py          # +db, +recorder kwargs, +DB_WRITE_CONFLICT arm
python/sidecar/__main__.py         # +DB open/migrate at startup, +LMA_RECORD_MEETING env
python/sidecar/tests/test_session_stream.py   # new tests covering DB-write and recording wiring
pyproject.toml                     # +nothing — pure stdlib + already-installed deps
```

## Error semantics

- Migration failure at startup → fatal, exit 1 (refuse to boot a half-migrated DB). Migration runner never silently rolls forward.
- DB open failure at startup (file not found, corrupt, locked) → fatal, exit 1.
- DB write failure at runtime → propagates as `sqlite3.DatabaseError`, caught in `_pump`'s exception chain, mapped to `ERROR {Code: "DB_WRITE_CONFLICT"}` then pump returns (current behavior for provider errors). One ERROR frame per failure — no retry logic in P2 (retry belongs in P3 reconnect policy).
- Recording write failure at runtime → logged via stderr, NOT propagated. Recording is best-effort; a missing audio file is recoverable (rerun from raw wire recording if needed), a failed DB write is not.
- Stale-partial sweep failure at startup → fatal, exit 1 (the sweep itself runs in a transaction; partial success would be worse than not starting).

## Testing strategy

- Real in-memory SQLite (`:memory:`) per test, migrations applied fresh. No mocks.
- `test_storage_connection.py` — `journal_mode=WAL`, `foreign_keys=ON`, `busy_timeout` PRAGMA assertions; idempotent open.
- `test_storage_migrations.py` — `apply_migrations` on empty DB runs all pending; running twice is a no-op; out-of-order files raise; broken SQL raises and leaves DB at prior version.
- `test_storage_writer_boundary.py` — every wire-to-DB field conversion in §5 tested independently; round-trip equality for stable fields.
- `test_storage.py` (and per-writer files) — every writer tested against `:memory:` SQLite; INSERTs verifiable via SELECT; constraint violations raise correctly.
- `test_storage_recording.py` — WavRecordingSink writes real WAV files; `wave` module reads them back; correct PCM framing; opens/closes per session.
- `test_persistence_session_integration.py` — real `ScriptedEngine` + real `SqliteWriter` + `MemoryConnection`; events that flow through `_pump` end up both in the connection's `sent` list AND in the SQLite DB. Recording variant of the same test verifies the recorder is fed in lockstep.
- All existing P1 tests stay green unchanged (default `db=None`, `recorder=None`).

## Open questions deferred to spec-writing

None — every architectural decision is captured above. Implementation-level choices (parameter ordering in `Session.__init__`, exact test fixture paths, etc.) will be settled at code-writing time per the plan's TDD discipline.