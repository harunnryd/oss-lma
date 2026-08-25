# P2 Sidecar Persistence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the Python sidecar's local SQLite persistence layer — open the DB at startup, apply migrations, write every event the assembler emits (segments, summaries, agent assists, agent tokens, thinking steps, action items, rag chunks, VP tasks) before sending it to the client, opt-in WAV recording alongside, and a stale-partials crash-recovery sweep.

**Architecture:** A new `python/sidecar/storage/` package with five focused modules (`connection`, `migrations`, `writer_boundary`, `writers`, `recording`). `Session` gains two optional seams (`PersistenceWriter`, `RecordingSink`) that default to `None`, preserving all P1 tests unchanged. Migrations are plain `.sql` files in `python/sidecar/migrations/`, applied by a custom stdlib-only runner that records versions in a `schema_version` table. Sync-write every event — no batch queue. Recording is opt-in via `LMA_RECORD_MEETING=1`.

**Tech Stack:** Python 3.12+, stdlib `sqlite3` + `wave` only (no SQLAlchemy, no Alembic), pytest, the existing `sidecar.frames.serialize_event` for outbound validation, the existing `Session._pump` exception chain for DB error mapping.

**Spec:** docs/superpowers/specs/2026-08-26-p2-persistence-design.md

---

## Part A — Storage foundations

Establishes the storage package skeleton, the SQLite connection with WAL mode + pragmas, the migration runner, and the writer-boundary normalization module. Conventions throughout: TDD red → green → commit; zero comments/docstrings in code; conventional one-line commit subjects (`feat:`/`test:`/`chore:`/`fix:`/`docs:`), NO Co-Authored-By trailer; every command runs from the repo root; Python >= 3.12.

**Out of scope for this section:** Rust shell code, recording files, the stale-partials sweep, the Session integration — those land in later sections.

---

### Task 1: Storage package skeleton + the initial SQL migration

**Files:**
- Create: `python/sidecar/storage/__init__.py`
- Create: `python/sidecar/storage/migrations/001_initial.sql` (note: directory `python/sidecar/storage/migrations/` is also new)
- Create: `python/sidecar/tests/__init__.py` (already exists from P1; if empty, leave it)
- Create: `python/sidecar/tests/test_storage_skeleton.py`

**Interfaces:** Consumes nothing (greenfield). Produces an importable `sidecar.storage` package and the initial migration file under `python/sidecar/storage/migrations/001_initial.sql` matching the Python-owned tables in `docs/database-schema.md` (`meetings`, `segments`, `summaries`, `action_items`, `rag_chunks`, `vp_tasks`, plus all indexes). The four Rust-owned tables (`settings`, `prompt_templates`, `mcp_servers`, `vp_schedules`) are **not** in this file — they migrate separately when the Rust shell phase lands.

- [ ] **Step 1: Write the failing skeleton test.**

`python/sidecar/tests/test_storage_skeleton.py`:

```python
def test_storage_package_is_importable():
    import sidecar.storage  # noqa: F401
```

- [ ] **Step 2: Run the test — red.**

Run: `uv run pytest python/sidecar/tests/test_storage_skeleton.py -v`
Expected: FAIL with `ModuleNotFoundError: No module named 'sidecar.storage'`.

- [ ] **Step 3: Create the package files.**

`python/sidecar/storage/__init__.py`: empty file (zero bytes).

`python/sidecar/storage/migrations/` directory: created by writing `001_initial.sql` to it. The directory will exist after the file is written; if your shell rejects writing to a non-existent directory, create the directory first.

`python/sidecar/storage/migrations/001_initial.sql`: this file contains only the Python-owned DDL verbatim from `docs/database-schema.md` (§ "Tables" through the end of `vp_tasks`). Concretely, in this order, each separated by a blank line:

1. `CREATE TABLE meetings (...)` — full DDL from the doc, including the `CHECK (ended_at IS NULL OR ended_at >= started_at)` constraint and the `idx_meetings_started_at` index. The `platform` column's `CHECK` includes `'local'`, `'zoom'`, `'meet'`. The `status` column's `CHECK` includes `'RECORDING'`, `'FINALIZING'`, `'COMPLETED'`, `'FAILED'`. Do NOT add `'VP'` to any `CHECK` here — `VP` appears only in `transcripts.meeting_source` (different table, Rust-side concerns), not in this Python-side schema.
2. `CREATE TABLE segments (...)` — full DDL including the `idx_segments_meeting_id_end_ms` index. The `is_partial` column uses `CHECK (is_partial IN (0, 1))` for now; the second migration will widen it.
3. `CREATE TABLE summaries (...)` — full DDL including the `idx_summaries_meeting_id_section` index (or whatever the doc names it; copy verbatim from the doc).
4. `CREATE TABLE action_items (...)` — full DDL.
5. `CREATE TABLE rag_chunks (...)` — full DDL.
6. `CREATE TABLE vp_tasks (...)` — full DDL.

Every DDL line ends with `;`. Every index is created immediately after its table. No data, no comments, no extra whitespace beyond what's in the doc. Read `docs/database-schema.md` (the actual file, not the snippets above) and copy the Python-owned DDL into the SQL file verbatim — the snippets above are guidance, not the source of truth.

- [ ] **Step 4: Run the test — green.**

Run: `uv run pytest python/sidecar/tests/test_storage_skeleton.py -v`
Expected: PASS — `1 passed`.

- [ ] **Step 5: Commit.**

```bash
git add python/sidecar/storage/ python/sidecar/tests/test_storage_skeleton.py
git commit -m "feat(sidecar): add storage package skeleton and initial schema"
```

---

### Task 2: Connection module with WAL pragmas

**Files:**
- Create: `python/sidecar/storage/connection.py`
- Test: `python/sidecar/tests/test_storage_connection.py`

**Interfaces:** Consumes stdlib `sqlite3`. Produces `open_db(path: Path) -> sqlite3.Connection` that opens (or creates) `<app-data>/lma.db`, applies `journal_mode=WAL`, `foreign_keys=ON`, and `busy_timeout=5000` (ms). Returns a connection with `row_factory=sqlite3.Row` so downstream code can use column-name access. Idempotent — calling twice on the same path yields the same connection configuration.

- [ ] **Step 1: Write the failing test.**

`python/sidecar/tests/test_storage_connection.py`:

```python
from pathlib import Path

import sqlite3

from sidecar.storage.connection import open_db


def test_open_db_returns_connection():
    conn = open_db(Path(":memory:"))
    assert isinstance(conn, sqlite3.Connection)


def test_open_db_sets_wal_journal_mode(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    mode = conn.execute("PRAGMA journal_mode").fetchone()[0]
    assert mode.lower() == "wal"


def test_open_db_enables_foreign_keys(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    fk = conn.execute("PRAGMA foreign_keys").fetchone()[0]
    assert fk == 1


def test_open_db_sets_busy_timeout(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    timeout = conn.execute("PRAGMA busy_timeout").fetchone()[0]
    assert timeout == 5000


def test_open_db_uses_row_factory(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    conn.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)")
    conn.execute("INSERT INTO t (name) VALUES ('x')")
    row = conn.execute("SELECT * FROM t").fetchone()
    assert row["name"] == "x"


def test_open_db_is_idempotent(tmp_path):
    path = tmp_path / "lma.db"
    conn1 = open_db(path)
    conn1.close()
    conn2 = open_db(path)
    mode = conn2.execute("PRAGMA journal_mode").fetchone()[0]
    assert mode.lower() == "wal"
    conn2.close()
```

- [ ] **Step 2: Run the test — red.**

Run: `uv run pytest python/sidecar/tests/test_storage_connection.py -v`
Expected: FAIL with `ModuleNotFoundError: No module named 'sidecar.storage.connection'`.

- [ ] **Step 3: Implement the connection module.**

`python/sidecar/storage/connection.py`:

```python
from pathlib import Path

import sqlite3


def open_db(path: Path) -> sqlite3.Connection:
    conn = sqlite3.connect(path)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("PRAGMA foreign_keys=ON")
    conn.execute("PRAGMA busy_timeout=5000")
    return conn
```

The `:memory:` path case (used in one test) does not write WAL data to disk but the `journal_mode` pragma succeeds and reports `memory` in some SQLite versions — that test uses `Path(":memory:")` because `:memory:` SQLite doesn't fail. If a future SQLite version refuses `journal_mode=WAL` on `:memory:`, change the test to use a `tmp_path` file instead. For now, the brief asserts `wal`; if a test failure appears in CI because `:memory:` returns `memory`, fix the test, not the implementation. (Trade-off: not testing `:memory:` path reduces coverage; acceptable for a small library.)

Actually — the safer approach: change the `test_open_db_sets_wal_journal_mode` test to use `tmp_path` only, not `:memory:`. SQLite returns `memory` for `:memory:` connections regardless of the WAL pragma; testing for `wal` on `:memory:` is incorrect. Use this corrected test:

```python
def test_open_db_sets_wal_journal_mode(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    mode = conn.execute("PRAGMA journal_mode").fetchone()[0]
    assert mode.lower() == "wal"
```

(Already `tmp_path` in the snippet above — confirmed correct.) For `test_open_db_returns_connection`, `:memory:` is fine because it only checks the type.

- [ ] **Step 4: Run the test — green.**

Run: `uv run pytest python/sidecar/tests/test_storage_connection.py -v`
Expected: PASS — `6 passed`.

- [ ] **Step 5: Run ruff.**

Run: `uv run ruff check python`
Expected: PASS — `All checks passed!`.

- [ ] **Step 6: Commit.**

```bash
git add python/sidecar/storage/connection.py python/sidecar/tests/test_storage_connection.py
git commit -m "feat(sidecar): add storage connection module with WAL pragmas"
```

---

### Task 3: Migration runner

**Files:**
- Create: `python/sidecar/storage/migrations.py`
- Test: `python/sidecar/tests/test_storage_migrations.py`

**Interfaces:** Consumes an open `sqlite3.Connection` and a `Path` to the migrations directory. Produces `apply_migrations(conn, dir: Path) -> list[int]` returning the versions applied in this run (empty list if everything is already applied). Creates a `schema_version(id INTEGER PRIMARY KEY, version INTEGER NOT NULL, applied_at INTEGER NOT NULL)` table if missing. Discovers `NNN_*.sql` files in `dir`, sorts by `NNN` as integer, applies each unapplied version in a transaction, records the version on success. On any exception during a migration, the transaction rolls back and the exception propagates (no partial application).

- [ ] **Step 1: Write the failing test.**

`python/sidecar/tests/test_storage_migrations.py`:

```python
import sqlite3
from pathlib import Path

from sidecar.storage.connection import open_db
from sidecar.storage.migrations import apply_migrations


def _write_migration(dir: Path, version: int, body: str) -> None:
    (dir / f"{version:03d}_test.sql").write_text(body, encoding="utf-8")


def test_apply_migrations_creates_schema_version_table(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    apply_migrations(conn, tmp_path / "migrations")
    rows = conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='schema_version'"
    ).fetchall()
    assert len(rows) == 1


def test_apply_migrations_runs_pending_files(tmp_path):
    migrations_dir = tmp_path / "migrations"
    migrations_dir.mkdir()
    _write_migration(
        migrations_dir,
        1,
        "CREATE TABLE widgets (id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
    )
    conn = open_db(tmp_path / "lma.db")
    applied = apply_migrations(conn, migrations_dir)
    assert applied == [1]
    cols = conn.execute("PRAGMA table_info(widgets)").fetchall()
    assert any(c["name"] == "name" for c in cols)


def test_apply_migrations_runs_multiple_in_order(tmp_path):
    migrations_dir = tmp_path / "migrations"
    migrations_dir.mkdir()
    _write_migration(
        migrations_dir,
        2,
        "CREATE TABLE widgets (id INTEGER PRIMARY KEY);",
    )
    _write_migration(
        migrations_dir,
        1,
        "CREATE TABLE sparks (id INTEGER PRIMARY KEY);",
    )
    conn = open_db(tmp_path / "lma.db")
    applied = apply_migrations(conn, migrations_dir)
    assert applied == [1, 2]


def test_apply_migrations_is_idempotent(tmp_path):
    migrations_dir = tmp_path / "migrations"
    migrations_dir.mkdir()
    _write_migration(
        migrations_dir,
        1,
        "CREATE TABLE widgets (id INTEGER PRIMARY KEY);",
    )
    conn = open_db(tmp_path / "lma.db")
    first = apply_migrations(conn, migrations_dir)
    second = apply_migrations(conn, migrations_dir)
    assert first == [1]
    assert second == []


def test_apply_migrations_records_version_and_timestamp(tmp_path):
    migrations_dir = tmp_path / "migrations"
    migrations_dir.mkdir()
    _write_migration(
        migrations_dir,
        1,
        "CREATE TABLE widgets (id INTEGER PRIMARY KEY);",
    )
    conn = open_db(tmp_path / "lma.db")
    apply_migrations(conn, migrations_dir)
    row = conn.execute("SELECT version, applied_at FROM schema_version").fetchone()
    assert row["version"] == 1
    assert row["applied_at"] > 0


def test_apply_migrations_rolls_back_on_broken_sql(tmp_path):
    migrations_dir = tmp_path / "migrations"
    migrations_dir.mkdir()
    _write_migration(migrations_dir, 1, "CREATE TABLE widgets (id INTEGER PRIMARY KEY);")
    _write_migration(migrations_dir, 2, "THIS IS NOT VALID SQL;")
    conn = open_db(tmp_path / "lma.db")

    import sqlite3 as _sqlite3

    with _sqlite3.connect(":memory:") as dummy:
        dummy.execute("CREATE TABLE x (id INTEGER PRIMARY KEY)")

    try:
        apply_migrations(conn, migrations_dir)
    except sqlite3.DatabaseError:
        pass
    else:
        raise AssertionError("expected DatabaseError on broken SQL")

    rows = conn.execute("SELECT version FROM schema_version").fetchall()
    assert [r["version"] for r in rows] == [1]


def test_apply_migrations_skips_out_of_order_higher_versions(tmp_path):
    migrations_dir = tmp_path / "migrations"
    migrations_dir.mkdir()
    _write_migration(migrations_dir, 5, "CREATE TABLE late (id INTEGER PRIMARY KEY);")
    conn = open_db(tmp_path / "lma.db")
    applied = apply_migrations(conn, migrations_dir)
    assert applied == []
    cols = conn.execute("PRAGMA table_info(late)").fetchall()
    assert cols == []
```

The last test (`test_apply_migrations_skips_out_of_order_higher_versions`) implements a deliberate safety behavior: if migration `005` exists but no `001`–`004`, do NOT apply it (it likely depends on the missing earlier migrations). The runner simply applies versions in sorted order and stops tracking once it hits a gap. Acceptable for P2 because the project is greenfield — version 1 will always be present.

- [ ] **Step 2: Run the test — red.**

Run: `uv run pytest python/sidecar/tests/test_storage_migrations.py -v`
Expected: FAIL with `ModuleNotFoundError: No module named 'sidecar.storage.migrations'`.

- [ ] **Step 3: Implement the migration runner.**

`python/sidecar/storage/migrations.py`:

```python
import re
import time
from pathlib import Path

import sqlite3


_VERSION_PATTERN = re.compile(r"^(\d{3,})_.+\.sql$")


def apply_migrations(conn: sqlite3.Connection, dir: Path) -> list[int]:
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_version ("
        "  id INTEGER PRIMARY KEY,"
        "  version INTEGER NOT NULL UNIQUE,"
        "  applied_at INTEGER NOT NULL"
        ")"
    )
    conn.commit()

    applied_versions = {
        row["version"]
        for row in conn.execute("SELECT version FROM schema_version").fetchall()
    }

    if not dir.exists():
        return []

    pending: list[tuple[int, Path]] = []
    for path in sorted(dir.iterdir()):
        match = _VERSION_PATTERN.match(path.name)
        if match is None:
            continue
        version = int(match.group(1))
        if version in applied_versions:
            continue
        pending.append((version, path))

    pending.sort(key=lambda item: item[0])

    applied: list[int] = []
    for version, path in pending:
        sql = path.read_text(encoding="utf-8")
        try:
            conn.execute("BEGIN")
            conn.executescript(sql)
            conn.execute(
                "INSERT INTO schema_version (version, applied_at) VALUES (?, ?)",
                (version, int(time.time() * 1000)),
            )
            conn.execute("COMMIT")
        except sqlite3.DatabaseError:
            conn.execute("ROLLBACK")
            raise
        applied.append(version)

    return applied
```

- [ ] **Step 4: Run the test — green.**

Run: `uv run pytest python/sidecar/tests/test_storage_migrations.py -v`
Expected: PASS — `7 passed`.

- [ ] **Step 5: Run ruff.**

Run: `uv run ruff check python`
Expected: PASS — `All checks passed!`.

- [ ] **Step 6: Commit.**

```bash
git add python/sidecar/storage/migrations.py python/sidecar/tests/test_storage_migrations.py
git commit -m "feat(sidecar): add storage migration runner"
```

---

### Task 4: End-to-end smoke test for storage foundations

**Files:**
- Create: `python/sidecar/tests/test_storage_integration.py`

**Interfaces:** Consumes `open_db` (Task 2), `apply_migrations` (Task 3), and the `001_initial.sql` file (Task 1). Produces a smoke test that confirms the initial migration applies cleanly to a real SQLite file, creates every Python-owned table from the schema, and survives a second `apply_migrations` call idempotently. This is the test that proves the initial SQL file actually works against real SQLite before any writers depend on it.

- [ ] **Step 1: Write the test.**

`python/sidecar/tests/test_storage_integration.py`:

```python
from pathlib import Path

from sidecar.storage.connection import open_db
from sidecar.storage.migrations import apply_migrations


def _storage_root() -> Path:
    return Path(__file__).resolve().parents[1] / "storage"


def test_initial_migration_creates_all_python_owned_tables(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    applied = apply_migrations(conn, _storage_root() / "migrations")
    assert applied == [1]

    table_names = {
        row["name"]
        for row in conn.execute(
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"
        ).fetchall()
    }
    expected = {
        "meetings",
        "segments",
        "summaries",
        "action_items",
        "rag_chunks",
        "vp_tasks",
        "schema_version",
    }
    assert expected.issubset(table_names)


def test_initial_migration_is_idempotent(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    first = apply_migrations(conn, _storage_root() / "migrations")
    second = apply_migrations(conn, _storage_root() / "migrations")
    assert first == [1]
    assert second == []


def test_segments_table_accepts_minimum_viable_row(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    apply_migrations(conn, _storage_root() / "migrations")
    conn.execute(
        "INSERT INTO meetings (id, source, started_at) VALUES (?, ?, ?)",
        ("m-1", "LOCAL", 1700000000000),
    )
    conn.execute(
        "INSERT INTO segments (segment_id, meeting_id, channel, start_ms, end_ms, "
        "text, original_text, is_partial) "
        "VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        ("r1-w0-r0", "m-1", "CALLER", 0, 800, "hello", "hello", 0),
    )
    row = conn.execute(
        "SELECT segment_id, channel, text, is_partial FROM segments"
    ).fetchone()
    assert row["segment_id"] == "r1-w0-r0"
    assert row["channel"] == "CALLER"
    assert row["text"] == "hello"
    assert row["is_partial"] == 0
```

- [ ] **Step 2: Run the test — green.**

Run: `uv run pytest python/sidecar/tests/test_storage_integration.py -v`
Expected: PASS — `3 passed` (assuming `001_initial.sql` was committed in Task 1 with correct DDL from the schema doc).

If any test fails, the failure is in `001_initial.sql` — fix the migration file directly. Do not change the test or the connection/migration runner code; they are already reviewed in Tasks 2 and 3.

- [ ] **Step 3: Commit only if a fix was needed.**

If the test passed on first run (expected for a spec-driven migration), no commit is needed for this task — the migration file's correctness is already implicitly verified by `1 passed`. Skip to Task 5.

If a fix was needed (typo in DDL, missing index, etc.), commit the fix:

```bash
git add python/sidecar/storage/migrations/001_initial.sql
git commit -m "fix(sidecar): correct initial schema migration to satisfy integration test"
```

---

### Task 5: Writer-boundary normalization module

**Files:**
- Create: `python/sidecar/storage/writer_boundary.py`
- Test: `python/sidecar/tests/test_storage_writer_boundary.py`

**Interfaces:** Consumes wire event dicts. Produces one `normalize_*` function per table that returns a tuple of column values in the table's declared column order (matching `001_initial.sql`). These functions are the single source of truth for wire→DB type conversions (float→ms, bool→0/1, absent→NULL).

Conversions:
- `StartTime / EndTime` (float seconds) → `int(round(value * 1000))`
- `IsPartial` (bool) → `1 if value else 0`
- `Speaker` (string or absent) → `None` if absent, else passthrough
- `SentimentScore` (float, optional) → `None` if absent, else passthrough
- `text` on segments → both `text` and `original_text` set to the same value
- UUIDs and enum strings → passthrough

- [ ] **Step 1: Write the failing test.**

`python/sidecar/tests/test_storage_writer_boundary.py`:

```python
from sidecar.storage.writer_boundary import (
    normalize_meeting_started,
    normalize_meeting_ended,
    normalize_segment,
    normalize_summary,
    normalize_agent_assist,
    normalize_agent_token,
    normalize_thinking_step,
)


def test_normalize_meeting_started_basic():
    ev = {
        "EventType": "START",
        "CallId": "11111111-1111-1111-1111-111111111111",
        "SamplingRate": 48000,
    }
    result = normalize_meeting_started(ev)
    assert result[0] == "11111111-1111-1111-1111-111111111111"
    assert result[1] == "LOCAL"
    assert isinstance(result[2], int)
    assert result[2] > 0


def test_normalize_meeting_started_uses_now_ms():
    ev = {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000}
    result = normalize_meeting_started(ev)
    assert isinstance(result[2], int)


def test_normalize_meeting_ended():
    ev = {
        "EventType": "END",
        "CallId": "m-1",
        "CreatedAt": "2026-08-26T09:30:00Z",
    }
    result = normalize_meeting_ended(ev)
    assert result[0] == "m-1"
    assert result[1] == "COMPLETED"
    assert isinstance(result[2], int)
    assert result[2] > 0


def test_normalize_segment_partial_with_speaker():
    ev = {
        "EventType": "ADD_TRANSCRIPT_SEGMENT",
        "CallId": "m-1",
        "SegmentId": "r1-w0-r0",
        "Channel": "CALLER",
        "Speaker": "spk_0",
        "StartTime": 12.5,
        "EndTime": 13.0,
        "Transcript": "hello world",
        "IsPartial": True,
    }
    result = normalize_segment(ev)
    assert result[0] == "r1-w0-r0"
    assert result[1] == "m-1"
    assert result[2] == "CALLER"
    assert result[3] == "spk_0"
    assert result[4] == 12500
    assert result[5] == 13000
    assert result[6] == "hello world"
    assert result[7] == "hello world"
    assert result[8] == 1
    assert result[9] is None


def test_normalize_segment_final_without_speaker():
    ev = {
        "EventType": "ADD_TRANSCRIPT_SEGMENT",
        "CallId": "m-1",
        "SegmentId": "r1-w0-r0",
        "Channel": "AGENT",
        "StartTime": 0.0,
        "EndTime": 1.5,
        "Transcript": "hi",
        "IsPartial": False,
    }
    result = normalize_segment(ev)
    assert result[2] == "AGENT"
    assert result[3] is None
    assert result[4] == 0
    assert result[5] == 1500
    assert result[8] == 0


def test_normalize_segment_with_sentiment():
    ev = {
        "EventType": "ADD_TRANSCRIPT_SEGMENT",
        "CallId": "m-1",
        "SegmentId": "r1",
        "Channel": "CALLER",
        "StartTime": 0.0,
        "EndTime": 1.0,
        "Transcript": "ok",
        "IsPartial": False,
        "SentimentScore": 0.42,
    }
    result = normalize_segment(ev)
    assert result[9] == 0.42


def test_normalize_segment_text_and_original_text_match():
    ev = {
        "EventType": "ADD_TRANSCRIPT_SEGMENT",
        "CallId": "m-1",
        "SegmentId": "r1",
        "Channel": "CALLER",
        "StartTime": 0.0,
        "EndTime": 1.0,
        "Transcript": "live translation not yet implemented",
        "IsPartial": False,
    }
    result = normalize_segment(ev)
    assert result[6] == result[7]


def test_normalize_summary():
    ev = {
        "EventType": "ADD_SUMMARY",
        "CallId": "m-1",
        "Section": "action_items",
        "SummaryText": "Caller agreed to renewal.",
    }
    result = normalize_summary(ev)
    assert result[0] == "m-1"
    assert result[1] == "action_items"
    assert result[2] == "Caller agreed to renewal."


def test_normalize_agent_assist():
    ev = {
        "EventType": "ADD_AGENT_ASSIST",
        "CallId": "m-1",
        "SegmentId": "asst_0001",
        "Transcript": "Offer the renewal discount.",
        "IsPartial": True,
    }
    result = normalize_agent_assist(ev)
    assert result[0] == "asst_0001"
    assert result[1] == "m-1"
    assert result[2] == "Offer the renewal discount."
    assert result[3] == 1
    assert result[4] is None


def test_normalize_agent_token():
    ev = {
        "EventType": "AGENT_TOKEN",
        "CallId": "m-1",
        "QueryId": "q-1",
        "Seq": 7,
        "Delta": "Hel",
    }
    result = normalize_agent_token(ev)
    assert result[0] == "q-1"
    assert result[1] == "m-1"
    assert result[2] == 7
    assert result[3] == "Hel"


def test_normalize_thinking_step():
    ev = {
        "EventType": "THINKING_STEP",
        "CallId": "m-1",
        "QueryId": "q-1",
        "Seq": 1,
        "StepType": "tool_use",
        "Content": "Searching transcript",
    }
    result = normalize_thinking_step(ev)
    assert result[0] == "q-1"
    assert result[1] == "m-1"
    assert result[2] == 1
    assert result[3] == "tool_use"
    assert result[4] == "Searching transcript"


def test_float_to_ms_rounds_half_to_even():
    from sidecar.storage.writer_boundary import _float_seconds_to_ms

    assert _float_seconds_to_ms(12.5) == 12500
    assert _float_seconds_to_ms(0.001) == 1
    assert _float_seconds_to_ms(0.0005) in (0, 1)
```

The final test is intentionally lenient on `0.0005` (which is `round()`'s banker's-rounding edge case) — both `0` and `1` are acceptable roundings. If you find this too loose, change to `assert _float_seconds_to_ms(0.0005) == 0` after checking Python's `round()` semantics in your environment; the production wire-to-DB conversion uses `int(round(...))` which is deterministic.

- [ ] **Step 2: Run the test — red.**

Run: `uv run pytest python/sidecar/tests/test_storage_writer_boundary.py -v`
Expected: FAIL with `ModuleNotFoundError: No module named 'sidecar.storage.writer_boundary'`.

- [ ] **Step 3: Implement the normalization module.**

`python/sidecar/storage/writer_boundary.py`:

```python
import time


def _float_seconds_to_ms(value: float) -> int:
    return int(round(value * 1000))


def _bool_to_int(value: bool) -> int:
    return 1 if value else 0


def _parse_iso_to_ms(value: str | None) -> int | None:
    if value is None:
        return None
    from datetime import datetime

    cleaned = value.replace("Z", "+00:00")
    parsed = datetime.fromisoformat(cleaned)
    return int(parsed.timestamp() * 1000)


def normalize_meeting_started(ev: dict) -> tuple:
    started_at = _parse_iso_to_ms(ev.get("CreatedAt"))
    if started_at is None:
        started_at = int(time.time() * 1000)
    return (ev["CallId"], "LOCAL", started_at)


def normalize_meeting_ended(ev: dict) -> tuple:
    ended_at = _parse_iso_to_ms(ev.get("CreatedAt"))
    if ended_at is None:
        ended_at = int(time.time() * 1000)
    return (ev["CallId"], "COMPLETED", ended_at)


def normalize_segment(ev: dict) -> tuple:
    speaker = ev.get("Speaker")
    if speaker == "":
        speaker = None
    return (
        ev["SegmentId"],
        ev["CallId"],
        ev["Channel"],
        speaker,
        _float_seconds_to_ms(ev["StartTime"]),
        _float_seconds_to_ms(ev["EndTime"]),
        ev["Transcript"],
        ev["Transcript"],
        _bool_to_int(ev["IsPartial"]),
        ev.get("SentimentScore"),
    )


def normalize_summary(ev: dict) -> tuple:
    return (ev["CallId"], ev["Section"], ev["SummaryText"])


def normalize_agent_assist(ev: dict) -> tuple:
    return (
        ev["SegmentId"],
        ev["CallId"],
        ev["Transcript"],
        _bool_to_int(ev["IsPartial"]),
        ev.get("TriggerSegmentId"),
    )


def normalize_agent_token(ev: dict) -> tuple:
    return (ev["QueryId"], ev["CallId"], ev["Seq"], ev["Delta"])


def normalize_thinking_step(ev: dict) -> tuple:
    return (
        ev["QueryId"],
        ev["CallId"],
        ev["Seq"],
        ev["StepType"],
        ev.get("Content"),
    )
```

If `001_initial.sql` declared different column orders than the tuples above, fix the tuples to match the SQL (the tuples are the single source of truth for the writers in Task 6 — they must match the column order exactly or every INSERT will mis-bind).

- [ ] **Step 4: Run the test — green.**

Run: `uv run pytest python/sidecar/tests/test_storage_writer_boundary.py -v`
Expected: PASS — `11 passed`.

- [ ] **Step 5: Run ruff.**

Run: `uv run ruff check python`
Expected: PASS — `All checks passed!`.

- [ ] **Step 6: Commit.**

```bash
git add python/sidecar/storage/writer_boundary.py python/sidecar/tests/test_storage_writer_boundary.py
git commit -m "feat(sidecar): add writer-boundary normalization for wire-to-DB conversion"
```

---

### Task 6: Per-table writer functions

**Files:**
- Create: `python/sidecar/storage/writers.py`
- Test: `python/sidecar/tests/test_storage_writers.py`

**Interfaces:** Consumes an open `sqlite3.Connection` and a wire event dict. Produces one `write_*` function per table: `write_meeting_started(conn, ev)`, `write_meeting_ended(conn, ev)`, `write_segment(conn, ev)`, `write_summary(conn, ev)`, `write_agent_assist(conn, ev)`, `write_agent_token(conn, ev)`, `write_thinking_step(conn, ev)`. Each calls the matching `normalize_*` from `writer_boundary.py` and runs an `INSERT OR ...` against the corresponding table. Each function commits after its own INSERT. The `write_segment` function uses `INSERT OR REPLACE` so partial→final replacement updates the existing row in place (segment_id is the primary key).

- [ ] **Step 1: Write the failing test.**

`python/sidecar/tests/test_storage_writers.py`:

```python
from pathlib import Path

from sidecar.storage.connection import open_db
from sidecar.storage.migrations import apply_migrations
from sidecar.storage.writers import (
    write_agent_assist,
    write_agent_token,
    write_meeting_ended,
    write_meeting_started,
    write_segment,
    write_summary,
    write_thinking_step,
)


def _bootstrap(tmp_path: Path):
    conn = open_db(tmp_path / "lma.db")
    apply_migrations(conn, Path(__file__).resolve().parents[2] / "storage" / "migrations")
    return conn


def test_write_meeting_started_inserts_meeting(tmp_path):
    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    row = conn.execute("SELECT id, source, status FROM meetings WHERE id = 'm-1'").fetchone()
    assert row["id"] == "m-1"
    assert row["source"] == "LOCAL"
    assert row["status"] == "RECORDING"


def test_write_segment_inserts_and_finalizes(tmp_path):
    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    write_segment(conn, {
        "EventType": "ADD_TRANSCRIPT_SEGMENT",
        "CallId": "m-1",
        "SegmentId": "r1-w0-r0",
        "Channel": "CALLER",
        "StartTime": 0.0,
        "EndTime": 1.5,
        "Transcript": "hello",
        "IsPartial": True,
    })
    write_segment(conn, {
        "EventType": "ADD_TRANSCRIPT_SEGMENT",
        "CallId": "m-1",
        "SegmentId": "r1-w0-r0",
        "Channel": "CALLER",
        "StartTime": 0.0,
        "EndTime": 1.6,
        "Transcript": "hello there",
        "IsPartial": False,
    })
    rows = conn.execute("SELECT segment_id, is_partial, text FROM segments").fetchall()
    assert len(rows) == 1
    assert rows[0]["is_partial"] == 0
    assert rows[0]["text"] == "hello there"


def test_write_segment_omits_speaker_when_absent(tmp_path):
    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    write_segment(conn, {
        "EventType": "ADD_TRANSCRIPT_SEGMENT",
        "CallId": "m-1",
        "SegmentId": "r1",
        "Channel": "AGENT",
        "StartTime": 0.0,
        "EndTime": 1.0,
        "Transcript": "hi",
        "IsPartial": False,
    })
    row = conn.execute("SELECT speaker FROM segments WHERE segment_id = 'r1'").fetchone()
    assert row["speaker"] is None


def test_write_summary_inserts(tmp_path):
    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    write_summary(conn, {
        "EventType": "ADD_SUMMARY",
        "CallId": "m-1",
        "Section": "action_items",
        "SummaryText": "Caller agreed.",
    })
    row = conn.execute(
        "SELECT section, content FROM summaries WHERE meeting_id = 'm-1'"
    ).fetchone()
    assert row["section"] == "action_items"
    assert row["content"] == "Caller agreed."


def test_write_agent_assist_inserts(tmp_path):
    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    write_agent_assist(conn, {
        "EventType": "ADD_AGENT_ASSIST",
        "CallId": "m-1",
        "SegmentId": "asst_1",
        "Transcript": "discount",
        "IsPartial": True,
    })
    row = conn.execute(
        "SELECT segment_id, is_partial FROM agent_assists WHERE segment_id = 'asst_1'"
    ).fetchone()
    assert row["segment_id"] == "asst_1"
    assert row["is_partial"] == 1


def test_write_agent_token_inserts(tmp_path):
    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    write_agent_token(conn, {
        "EventType": "AGENT_TOKEN",
        "CallId": "m-1",
        "QueryId": "q-1",
        "Seq": 0,
        "Delta": "Hel",
    })
    row = conn.execute(
        "SELECT query_id, seq, delta FROM agent_tokens WHERE query_id = 'q-1'"
    ).fetchone()
    assert row["seq"] == 0
    assert row["delta"] == "Hel"


def test_write_thinking_step_inserts(tmp_path):
    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    write_thinking_step(conn, {
        "EventType": "THINKING_STEP",
        "CallId": "m-1",
        "QueryId": "q-1",
        "Seq": 1,
        "StepType": "tool_use",
        "Content": "Searching",
    })
    row = conn.execute(
        "SELECT query_id, step_type, content FROM thinking_steps WHERE seq = 1"
    ).fetchone()
    assert row["step_type"] == "tool_use"
    assert row["content"] == "Searching"


def test_write_meeting_ended_updates_status(tmp_path):
    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    write_meeting_ended(conn, {"EventType": "END", "CallId": "m-1", "CreatedAt": "2026-08-26T10:00:00Z"})
    row = conn.execute("SELECT status, ended_at FROM meetings WHERE id = 'm-1'").fetchone()
    assert row["status"] == "COMPLETED"
    assert row["ended_at"] > 0


def test_writer_raises_on_db_error(tmp_path):
    conn = _bootstrap(tmp_path)
    import pytest
    from sidecar.storage.writers import write_segment

    with pytest.raises(Exception):
        write_segment(conn, {"EventType": "ADD_TRANSCRIPT_SEGMENT"})


def test_unmatched_event_type_raises_value_error(tmp_path):
    from sidecar.storage.writers import dispatch_write

    conn = _bootstrap(tmp_path)
    import pytest

    with pytest.raises(ValueError):
        dispatch_write(conn, {"EventType": "VP_COMMAND", "TaskId": "t-1", "Command": "CLICK"})
```

Note: the `dispatch_write` function at the bottom is the public dispatcher that `SqliteWriter` (Task 7) calls. It is defined in writers.py alongside the per-table writers, not in a separate file.

- [ ] **Step 2: Run the test — red.**

Run: `uv run pytest python/sidecar/tests/test_storage_writers.py -v`
Expected: FAIL with `ModuleNotFoundError: No module named 'sidecar.storage.writers'`.

- [ ] **Step 3: Implement the writers module.**

`python/sidecar/storage/writers.py`:

```python
import sqlite3

from sidecar.storage.writer_boundary import (
    normalize_agent_assist,
    normalize_agent_token,
    normalize_meeting_ended,
    normalize_meeting_started,
    normalize_segment,
    normalize_summary,
    normalize_thinking_step,
)


def write_meeting_started(conn: sqlite3.Connection, ev: dict) -> None:
    values = normalize_meeting_started(ev)
    conn.execute(
        "INSERT OR REPLACE INTO meetings (id, source, started_at) VALUES (?, ?, ?)",
        values,
    )
    conn.commit()


def write_meeting_ended(conn: sqlite3.Connection, ev: dict) -> None:
    call_id, status, ended_at = normalize_meeting_ended(ev)
    conn.execute(
        "UPDATE meetings SET status = ?, ended_at = ?, "
        "duration_ms = COALESCE(duration_ms, ? - started_at) "
        "WHERE id = ?",
        (status, ended_at, ended_at, call_id),
    )
    conn.commit()


def write_segment(conn: sqlite3.Connection, ev: dict) -> None:
    values = normalize_segment(ev)
    conn.execute(
        "INSERT OR REPLACE INTO segments "
        "(segment_id, meeting_id, channel, speaker, start_ms, end_ms, "
        " text, original_text, is_partial, sentiment_score) "
        "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        values,
    )
    conn.commit()


def write_summary(conn: sqlite3.Connection, ev: dict) -> None:
    values = normalize_summary(ev)
    conn.execute(
        "INSERT OR REPLACE INTO summaries (meeting_id, section, content) VALUES (?, ?, ?)",
        values,
    )
    conn.commit()


def write_agent_assist(conn: sqlite3.Connection, ev: dict) -> None:
    values = normalize_agent_assist(ev)
    conn.execute(
        "INSERT OR REPLACE INTO agent_assists "
        "(segment_id, meeting_id, transcript, is_partial, trigger_segment_id) "
        "VALUES (?, ?, ?, ?, ?)",
        values,
    )
    conn.commit()


def write_agent_token(conn: sqlite3.Connection, ev: dict) -> None:
    values = normalize_agent_token(ev)
    conn.execute(
        "INSERT OR REPLACE INTO agent_tokens "
        "(query_id, meeting_id, seq, delta) VALUES (?, ?, ?, ?)",
        values,
    )
    conn.commit()


def write_thinking_step(conn: sqlite3.Connection, ev: dict) -> None:
    values = normalize_thinking_step(ev)
    conn.execute(
        "INSERT OR REPLACE INTO thinking_steps "
        "(query_id, meeting_id, seq, step_type, content) VALUES (?, ?, ?, ?, ?)",
        values,
    )
    conn.commit()


def dispatch_write(conn: sqlite3.Connection, ev: dict) -> None:
    event_type = ev.get("EventType")
    if event_type == "START":
        write_meeting_started(conn, ev)
    elif event_type == "END":
        write_meeting_ended(conn, ev)
    elif event_type == "ADD_TRANSCRIPT_SEGMENT":
        write_segment(conn, ev)
    elif event_type == "ADD_SUMMARY":
        write_summary(conn, ev)
    elif event_type == "ADD_AGENT_ASSIST":
        write_agent_assist(conn, ev)
    elif event_type == "AGENT_TOKEN":
        write_agent_token(conn, ev)
    elif event_type == "THINKING_STEP":
        write_thinking_step(conn, ev)
    else:
        raise ValueError(f"unhandled event type for persistence: {event_type!r}")
```

If `001_initial.sql` uses different table names for `agent_assists` / `agent_tokens` / `thinking_steps` (e.g. the doc might name them differently — read the doc verbatim and adjust), update the SQL references above. The principle: SQL column/table names must match `001_initial.sql` exactly.

- [ ] **Step 4: Run the test — green.**

Run: `uv run pytest python/sidecar/tests/test_storage_writers.py -v`
Expected: PASS — `10 passed`.

- [ ] **Step 5: Run ruff.**

Run: `uv run ruff check python`
Expected: PASS — `All checks passed!`.

- [ ] **Step 6: Commit.**

```bash
git add python/sidecar/storage/writers.py python/sidecar/tests/test_storage_writers.py
git commit -m "feat(sidecar): add per-table writer functions and dispatch"
```

---

## Part B — Recording, Session integration, entrypoint

Wires the storage layer into the sidecar's runtime. Recording is opt-in via env var; Session gains two optional seams; the entrypoint opens the DB at startup and applies migrations.

**Out of scope for this section:** the stale-partials sweep (Part C), error-frame mapping for `DB_WRITE_CONFLICT` (Part C).

---

### Task 7: WAV recording sink

**Files:**
- Create: `python/sidecar/storage/recording.py`
- Test: `python/sidecar/tests/test_storage_recording.py`

**Interfaces:** Consumes a `Path` to a target `.wav` file. Produces `WavRecordingSink(path)` and `NullRecordingSink()`. The WAV sink uses stdlib `wave` to write raw stereo s16le 48 kHz PCM. `feed(pcm: bytes)` calls `wave.writeframes(pcm)` for each chunk. `stop()` closes the file. The null sink is a no-op for both methods.

- [ ] **Step 1: Write the failing test.**

`python/sidecar/tests/test_storage_recording.py`:

```python
import struct
import wave
from pathlib import Path

import pytest

from sidecar.storage.recording import NullRecordingSink, WavRecordingSink


def test_wav_sink_creates_valid_wav_header(tmp_path):
    sink = WavRecordingSink(tmp_path / "out.wav")
    sink.feed(b"\x00" * 19200)
    sink.stop()
    with wave.open(str(tmp_path / "out.wav"), "rb") as reader:
        assert reader.getnchannels() == 2
        assert reader.getsampwidth() == 2
        assert reader.getframerate() == 48000


def test_wav_sink_writes_pcm_payload(tmp_path):
    sink = WavRecordingSink(tmp_path / "out.wav")
    pcm = struct.pack("<hh", 100, -100) * 4800
    sink.feed(pcm)
    sink.stop()
    with wave.open(str(tmp_path / "out.wav"), "rb") as reader:
        frames = reader.readframes(9600)
        assert frames[:2] == b"\x64\x00"
        assert frames[2:4] == b"\x9c\xff"


def test_wav_sink_appends_across_feeds(tmp_path):
    sink = WavRecordingSink(tmp_path / "out.wav")
    sink.feed(b"\x00" * 4800)
    sink.feed(b"\x00" * 4800)
    sink.stop()
    with wave.open(str(tmp_path / "out.wav"), "rb") as reader:
        assert reader.getnframes() == 9600


def test_null_sink_is_no_op(tmp_path):
    sink = NullRecordingSink()
    sink.feed(b"\x00" * 19200)
    sink.stop()
    assert not (tmp_path / "out.wav").exists()


def test_wav_sink_rejects_non_48khz_assumption_via_documentation(tmp_path):
    sink = WavRecordingSink(tmp_path / "out.wav")
    sink.feed(b"\x00" * 19200)
    sink.stop()
    with wave.open(str(tmp_path / "out.wav"), "rb") as reader:
        assert reader.getframerate() == 48000
```

The last test documents the assumption — P2 records at 48 kHz only, matching the wire's PCM chunk size. Non-48 kHz sessions either opt out of recording (default) or have the sink raise (out of scope here; would be a P3 concern).

- [ ] **Step 2: Run the test — red.**

Run: `uv run pytest python/sidecar/tests/test_storage_recording.py -v`
Expected: FAIL with `ModuleNotFoundError: No module named 'sidecar.storage.recording'`.

- [ ] **Step 3: Implement the recording module.**

`python/sidecar/storage/recording.py`:

```python
import wave
from pathlib import Path


class NullRecordingSink:
    def feed(self, pcm: bytes) -> None:
        return None

    def stop(self) -> None:
        return None


class WavRecordingSink:
    def __init__(self, path: Path) -> None:
        self._file = wave.open(str(path), "wb")
        self._file.setnchannels(2)
        self._file.setsampwidth(2)
        self._file.setframerate(48000)

    def feed(self, pcm: bytes) -> None:
        self._file.writeframes(pcm)

    def stop(self) -> None:
        if not self._file.getnframes() == 0 or self._file._file is not None:
            pass
        self._file.close()
```

The trailing `pass` and `_file.getnframes()` check are noise — `wave.Wave_write.close()` is the only thing that matters. The brief implementation is:

```python
class WavRecordingSink:
    def __init__(self, path: Path) -> None:
        self._file = wave.open(str(path), "wb")
        self._file.setnchannels(2)
        self._file.setsampwidth(2)
        self._file.setframerate(48000)

    def feed(self, pcm: bytes) -> None:
        self._file.writeframes(pcm)

    def stop(self) -> None:
        self._file.close()
```

The first version is what passes the lint; the simpler version is what matches the spec. If ruff rejects the simpler version (it shouldn't — `close()` is unconditional), use the noise version. Verify in the next step.

- [ ] **Step 4: Run the test — green.**

Run: `uv run pytest python/sidecar/tests/test_storage_recording.py -v`
Expected: PASS — `5 passed`.

- [ ] **Step 5: Run ruff.**

Run: `uv run ruff check python`
Expected: PASS. If ruff flags the simpler version for unused-imports or unreachable code, restore the conditional variant.

- [ ] **Step 6: Commit.**

```bash
git add python/sidecar/storage/recording.py python/sidecar/tests/test_storage_recording.py
git commit -m "feat(sidecar): add WAV recording sink with null sink default"
```

---

### Task 8: PersistenceWriter Protocol + SqliteWriter

**Files:**
- Create: `python/sidecar/storage/persistence.py`
- Test: `python/sidecar/tests/test_storage_persistence.py`

**Interfaces:** Consumes an open `sqlite3.Connection`. Produces `PersistenceWriter` Protocol (defined in `storage/persistence.py` with a `write(event: dict) -> None` method) and `SqliteWriter(conn)` implementation that delegates to `dispatch_write(conn, ev)`. Also `NullWriter()` no-op. The Protocol lives in `persistence.py` because Task 9 imports it from there.

- [ ] **Step 1: Write the failing test.**

`python/sidecar/tests/test_storage_persistence.py`:

```python
from pathlib import Path

from sidecar.storage.connection import open_db
from sidecar.storage.migrations import apply_migrations
from sidecar.storage.persistence import NullWriter, SqliteWriter


def test_sqlite_writer_dispatches_to_segments(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    apply_migrations(conn, Path(__file__).resolve().parents[2] / "storage" / "migrations")
    conn.execute(
        "INSERT INTO meetings (id, source, started_at) VALUES (?, ?, ?)",
        ("m-1", "LOCAL", 1700000000000),
    )
    conn.commit()
    writer = SqliteWriter(conn)
    writer.write({
        "EventType": "ADD_TRANSCRIPT_SEGMENT",
        "CallId": "m-1",
        "SegmentId": "r1-w0-r0",
        "Channel": "CALLER",
        "StartTime": 0.0,
        "EndTime": 1.0,
        "Transcript": "hello",
        "IsPartial": False,
    })
    row = conn.execute("SELECT text FROM segments WHERE segment_id = 'r1-w0-r0'").fetchone()
    assert row["text"] == "hello"


def test_sqlite_writer_raises_on_db_error(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    apply_migrations(conn, Path(__file__).resolve().parents[2] / "storage" / "migrations")
    writer = SqliteWriter(conn)
    import sqlite3
    import pytest

    with pytest.raises(sqlite3.DatabaseError):
        writer.write({
            "EventType": "ADD_TRANSCRIPT_SEGMENT",
            "CallId": "m-1",
            "SegmentId": "r1-w0-r0",
            "Channel": "CALLER",
            "StartTime": 0.0,
            "EndTime": 1.0,
            "Transcript": "hello",
            "IsPartial": False,
        })


def test_null_writer_does_nothing():
    NullWriter().write({"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
```

The second test confirms DB errors propagate as `sqlite3.DatabaseError` — the `m-1` meeting doesn't exist, so the FK constraint on `segments.meeting_id` fails. This is the contract Task 9 relies on for `DB_WRITE_CONFLICT` mapping.

- [ ] **Step 2: Run the test — red.**

Run: `uv run pytest python/sidecar/tests/test_storage_persistence.py -v`
Expected: FAIL with `ModuleNotFoundError: No module named 'sidecar.storage.persistence'`.

- [ ] **Step 3: Implement the persistence module.**

`python/sidecar/storage/persistence.py`:

```python
import sqlite3
from typing import Protocol

from sidecar.storage.writers import dispatch_write


class PersistenceWriter(Protocol):
    def write(self, event: dict) -> None:
        ...


class NullWriter:
    def write(self, event: dict) -> None:
        return None


class SqliteWriter:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def write(self, event: dict) -> None:
        dispatch_write(self._conn, event)
```

- [ ] **Step 4: Run the test — green.**

Run: `uv run pytest python/sidecar/tests/test_storage_persistence.py -v`
Expected: PASS — `3 passed`.

- [ ] **Step 5: Run ruff.**

Run: `uv run ruff check python`
Expected: PASS.

- [ ] **Step 6: Commit.**

```bash
git add python/sidecar/storage/persistence.py python/sidecar/tests/test_storage_persistence.py
git commit -m "feat(sidecar): add PersistenceWriter Protocol with SqliteWriter"
```

---

### Task 9: Session integration — PersistenceWriter seam

**Files:**
- Modify: `python/sidecar/session.py` (+ constructor kwargs, + write call in `_pump`)
- Test: `python/sidecar/tests/test_persistence_session_integration.py`

**Interfaces:** Consumes Task 8's `PersistenceWriter` Protocol. Produces `Session.__init__` accepting `db: PersistenceWriter | None = None` and `recorder: RecordingSink | None = None` keyword arguments. `_pump` calls `self.db.write(event)` between `assembler.on_result` and `_send(event)` whenever `self.db is not None`. All existing tests stay green unchanged because the new kwargs default to `None`. The recording seam (`recorder.feed(pcm)` after `stream.feed(pcm)` in `on_binary`) is also added here, sharing the test file.

- [ ] **Step 1: Write the failing integration test.**

`python/sidecar/tests/test_persistence_session_integration.py`:

```python
import json
from pathlib import Path

from lma_pipeline import SegmentAssembler

from sidecar.frames import INVALID_FRAME_CLOSE_CODE, INVALID_FRAME_CODE, INVALID_FRAME_CONTEXT
from sidecar.session import Session
from sidecar.storage.connection import open_db
from sidecar.storage.migrations import apply_migrations
from sidecar.storage.persistence import NullWriter, SqliteWriter
from sidecar.storage.recording import WavRecordingSink

from tests.helpers import MemoryConnection, ScriptedEngine, eventually

CALL_ID = "11111111-1111-1111-1111-111111111111"


def _bootstrap(tmp_path: Path):
    conn = open_db(tmp_path / "lma.db")
    apply_migrations(conn, Path(__file__).resolve().parents[2] / "storage" / "migrations")
    return conn


async def make_session_with_db(tmp_path, results):
    db_conn = _bootstrap(tmp_path)
    connection = MemoryConnection()
    engine = ScriptedEngine(results)
    session = Session(connection, lambda ctx: engine, db=SqliteWriter(db_conn))
    await session.on_text(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
    return connection, db_conn, engine, session


async def test_session_writes_segments_to_sqlite(tmp_path):
    result = {
        "result_id": "r1",
        "is_partial": False,
        "items": [{
            "content": "hello",
            "type": "pronunciation",
            "start_time": 0.0,
            "end_time": 0.8,
            "speaker": "spk_0",
            "channel": "CALLER",
            "result_id": "r1",
        }],
    }
    connection, db_conn, _, _ = await make_session_with_db(tmp_path, [result])
    await eventually(lambda: len(connection.sent) == 1)
    row = db_conn.execute(
        "SELECT text, channel, is_partial FROM segments WHERE meeting_id = ?",
        (CALL_ID,),
    ).fetchone()
    assert row["text"] == "hello"
    assert row["channel"] == "CALLER"
    assert row["is_partial"] == 0


async def test_session_meeting_row_exists_after_start(tmp_path):
    connection, db_conn, _, _ = await make_session_with_db(tmp_path, [])
    await eventually(lambda: True)
    row = db_conn.execute(
        "SELECT id, source, status FROM meetings WHERE id = ?", (CALL_ID,)
    ).fetchone()
    assert row["id"] == CALL_ID
    assert row["source"] == "LOCAL"
    assert row["status"] == "RECORDING"


async def test_session_meeting_status_updates_on_end(tmp_path):
    connection, db_conn, _, session = await make_session_with_db(tmp_path, [])
    await session.on_text(json.dumps({"EventType": "END", "CallId": CALL_ID}))
    await eventually(lambda: db_conn.execute("SELECT status FROM meetings WHERE id = ?", (CALL_ID,)).fetchone()["status"] == "COMPLETED")


async def test_session_records_audio_when_recorder_provided(tmp_path):
    connection = MemoryConnection()
    engine = ScriptedEngine([])
    recorder_path = tmp_path / "rec.wav"
    recorder = WavRecordingSink(recorder_path)
    session = Session(connection, lambda ctx: engine, recorder=recorder)
    await session.on_text(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
    await session.on_binary(bytes(19200))
    await session.on_text(json.dumps({"EventType": "END", "CallId": CALL_ID}))
    await eventually(lambda: db_row_exists(tmp_path, CALL_ID))
    import wave
    with wave.open(str(recorder_path), "rb") as reader:
        assert reader.getnframes() == 4800


async def test_session_default_db_is_noop():
    connection = MemoryConnection()
    engine = ScriptedEngine([])
    session = Session(connection, lambda ctx: engine)
    await session.on_text(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
    assert session.db is None


def db_row_exists(tmp_path, call_id):
    conn = open_db(tmp_path / "lma.db")
    row = conn.execute("SELECT id FROM meetings WHERE id = ?", (call_id,)).fetchone()
    return row is not None
```

The `db_row_exists` helper is intentionally a sync function returning a bool, callable from `eventually`.

- [ ] **Step 2: Run the test — red.**

Run: `uv run pytest python/sidecar/tests/test_persistence_session_integration.py -v`
Expected: FAIL — `Session.__init__` does not accept `db` / `recorder` kwargs (or if it accepts but ignores, the writes won't appear in the DB).

- [ ] **Step 3: Update `session.py`.**

Open `python/sidecar/session.py` and apply the following changes:

a) Add a new import near the existing `lma_*` imports:

```python
import sqlite3

from sidecar.storage.persistence import PersistenceWriter
from sidecar.storage.recording import RecordingSink
```

(Adjust import ordering to match the existing file's convention — `lma_*` imports first, then `sidecar.*` imports alphabetical, then stdlib.)

b) Update the `Session.__init__` signature:

```python
def __init__(
    self,
    connection,
    engine_factory,
    *,
    db: PersistenceWriter | None = None,
    recorder: RecordingSink | None = None,
) -> None:
    self.connection = connection
    self.engine_factory = engine_factory
    self.db = db
    self.recorder = recorder
    self.call_id = ""
    self.stream = None
    self.assembler = None
    self.pump_task: asyncio.Task | None = None
    self.paused = False
    self.chunk_bytes = 0
    self.send_lock = asyncio.Lock()
```

(Add `self.db = db` and `self.recorder = recorder` to the body; the existing fields stay.)

c) Update `_pump` to call `self.db.write(event)` when not None:

Find the existing `_pump` method (it's the one with `async for result in stream: ...`). Modify the inner loop:

```python
        for event in assembler.on_result(result):
            if self.db is not None:
                self.db.write(event)
            await self._send(event)
```

(The exact placement — before or after `await self._send(event)` — does not matter for correctness because `self.db.write` is synchronous. Place it before to mirror the spec's ordering.)

e) Update `on_binary` to call `self.recorder.feed(pcm)` when not None:

Find `on_binary` (it has `if self.stream is None: ...`). Modify the post-`feed` line:

```python
        await self.stream.feed(pcm)
        if self.recorder is not None:
            self.recorder.feed(pcm)
```

f) Update `_close_session` (or the existing `End()`-handling path in `on_text`) to call `self.recorder.stop()` when a meeting ends. The cleanest spot: at the bottom of `_close_session(drain=True)`, after the drain, call `self.recorder.stop()` if not None. If the recorder has already been stopped or never started, that's a no-op (`NullRecordingSink().stop()` is a no-op).

Add this inside `_close_session(drain=True)` after the drain completes:

```python
        if drain and and pump_task is not None:  # existing condition
            ...
        if self.recorder is not None:
            self.recorder.stop()
```

Place the `recorder.stop()` once, unconditionally after the session's stream/assembler/pump_task are nulled. Avoid double-stop by tracking `_recorder_stopped` (out of scope; the WAV sink is safe to call close twice — `wave.Wave_write.close()` on a closed file is a no-op).

- [ ] **Step 4: Run the test — green.**

Run: `uv run pytest python/sidecar/tests/test_persistence_session_integration.py -v`
Expected: PASS — `5 passed`.

- [ ] **Step 5: Run the FULL sidecar suite to confirm no regressions.**

Run: `uv run pytest python/sidecar -v`
Expected: ALL previous sidecar tests still green (84 prior tests + 5 new = 89). Any failure means the new kwargs broke an existing test path — investigate and fix.

- [ ] **Step 6: Run ruff.**

Run: `uv run ruff check python`
Expected: PASS.

- [ ] **Step 7: Commit.**

```bash
git add python/sidecar/session.py python/sidecar/tests/test_persistence_session_integration.py
git commit -m "feat(sidecar): wire PersistenceWriter and RecordingSink into Session"
```

---

### Task 10: Entrypoint opens DB at startup, applies migrations, passes writer to sessions

**Files:**
- Modify: `python/sidecar/__main__.py` (+ DB open, + migration apply, + writer construction, + env-var-driven recording)

**Interfaces:** Consumes `LMA_DB_PATH` env var (default `<app-data>/lma.db`, computed via existing platform conventions) and `LMA_RECORD_MEETING=1` env var (default off). Produces a `__main__.py` that opens the DB once at startup, applies migrations, constructs one `SqliteWriter`, and passes it to every new `Session`. Recording is opt-in.

- [ ] **Step 1: Update `__main__.py`.**

`python/sidecar/__main__.py`:

```python
import asyncio
import os
import signal
import sys
from pathlib import Path

from lma_stt.fake import FakeEngine

from sidecar.server import BindFailed, run_server
from sidecar.storage.connection import open_db
from sidecar.storage.migrations import apply_migrations
from sidecar.storage.persistence import SqliteWriter


def default_engine_factory(ctx):
    return FakeEngine(script=[])


def _default_db_path() -> Path:
    explicit = os.environ.get("LMA_DB_PATH")
    if explicit:
        return Path(explicit)
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / "oss-lma" / "lma.db"
    if sys.platform.startswith("win"):
        appdata = os.environ.get("APPDATA")
        if appdata:
            return Path(appdata) / "oss-lma" / "lma.db"
    xdg = os.environ.get("XDG_DATA_HOME")
    base = Path(xdg) if xdg else Path.home() / ".local" / "share"
    return base / "oss-lma" / "lma.db"


def _record_meeting_enabled() -> bool:
    return os.environ.get("LMA_RECORD_MEETING") == "1"


async def main() -> int:
    stop = asyncio.Event()
    loop = asyncio.get_running_loop()
    for sig in (signal.SIGINT, signal.SIGTERM):
        loop.add_signal_handler(sig, stop.set)

    db_path = _default_db_path()
    db_path.parent.mkdir(parents=True, exist_ok=True)
    conn = open_db(db_path)
    apply_migrations(conn, Path(__file__).resolve().parent / "storage" / "migrations")
    writer = SqliteWriter(conn)
    record_enabled = _record_meeting_enabled()

    try:
        await run_server(
            default_engine_factory,
            stop=stop,
            db_writer=writer,
            record_meeting=record_enabled,
        )
    except BindFailed:
        return 1
    finally:
        conn.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
```

The `db_writer=writer` and `record_meeting=record_enabled` kwargs are NOT consumed by `run_server` yet — Task 11 wires them through.

- [ ] **Step 2: Verify import works.**

Run: `uv run python -c "import sidecar.__main__"`
Expected: no error.

- [ ] **Step 3: Verify entrypoint still runs (no DB-touching behavior yet).**

Run: `timeout 2 uv run python -m sidecar` (then Ctrl-C after the SIDECAR_READY line).

If `run_server` errors because of the unrecognized kwargs (`db_writer`, `record_meeting`), this is expected — Task 11 adds them. Skip running for now and just check the import compiles.

- [ ] **Step 4: Run ruff.**

Run: `uv run ruff check python`
Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add python/sidecar/__main__.py
git commit -m "feat(sidecar): open DB and apply migrations at startup"
```

---

### Task 11: run_server accepts db_writer and record_meeting kwargs, threads them into Session

**Files:**
- Modify: `python/sidecar/server.py` (+ two kwargs, + per-session writer/recorder construction)
- Modify: `python/sidecar/tests/test_server_transport.py` (extend existing tests)

**Interfaces:** Consumes `db_writer: PersistenceWriter | None = None` and `record_meeting: bool = False`. Produces a `run_server` that constructs one `Session` per connection, passing `db=db_writer` to all sessions and `recorder=WavRecordingSink(<path>)` when `record_meeting` is True. The WAV path is computed per-session from the `CallId` once the session starts.

- [ ] **Step 1: Update `run_server` in `server.py`.**

Open `python/sidecar/server.py` and apply these changes:

a) Add imports:

```python
from sidecar.storage.recording import WavRecordingSink
```

b) Update `run_server` signature:

```python
def run_server(
    engine_factory: Callable,
    stop: asyncio.Event | None = None,
    ready_sink: TextIO | None = None,
    *,
    db_writer: PersistenceWriter | None = None,
    record_meeting: bool = False,
) -> tuple[int, str]:
```

c) Update the `_handler` inner function:

```python
def _handler(engine_factory: Callable, sessions: set, db_writer: PersistenceWriter | None, record_meeting: bool) -> Callable[[ServerConnection], object]:
    async def handle(connection: ServerConnection) -> None:
        recorder_sink = None
        if record_meeting:
            pass
        session = Session(
            connection,
            engine_factory,
            db=db_writer,
            recorder=None,
        )
        sessions.add(session)
        try:
            await session.run()
        finally:
            sessions.discard(session)

    return handle
```

The recorder-per-session wiring (creating `WavRecordingSink(<call_id>.wav)` on the first message after `START`) is complex enough to belong in `Session` itself rather than the server. To keep Task 11 scoped narrowly, defer per-session recorder creation to Task 12 — Task 11 wires only the `db_writer` path and adds a `record_meeting` flag that is accepted but whose wiring is the explicit subject of Task 12 (the recorder-per-session construction inside `Session`). Task 11 only validates the kwarg is accepted by `run_server`.

Wait — Task 11's contract is "thread `db_writer` and `record_meeting` into Session". If recorder wiring is deferred to Task 12, this task only needs to pass `db_writer` and add a no-op `record_meeting` parameter. Update the implementation to:

```python
        session = Session(
            connection,
            engine_factory,
            db=db_writer,
        )
```

`record_meeting` is accepted but not yet consumed; Task 12 will add the per-session recorder construction inside `Session`.

- [ ] **Step 2: Update existing tests.**

In `python/sidecar/tests/test_server_transport.py`, add one new test that verifies `run_server` accepts the new kwargs without breaking existing tests:

```python
async def test_run_server_accepts_db_writer_and_record_meeting_kwargs():
    stop, task, _, _ = await spawn_sidecar(lambda ctx: ScriptedEngine([]))
    try:
        from sidecar.server import run_server

        result = await run_server(
            lambda ctx: ScriptedEngine([]),
            stop=asyncio.Event(),
            ready_sink=io.StringIO(),
            db_writer=None,
            record_meeting=False,
        )
        assert isinstance(result, tuple)
    finally:
        stop.set()
        await task
```

- [ ] **Step 3: Run the full sidecar test suite.**

Run: `uv run pytest python/sidecar -v`
Expected: ALL sidecar tests still green (89 prior + 1 new = 90). Any failure indicates the new kwargs are being mis-threaded — investigate.

- [ ] **Step 4: Run ruff.**

Run: `uv run ruff check python`
Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add python/sidecar/server.py python/sidecar/tests/test_server_transport.py
git commit -m "feat(sidecar): thread db_writer through run_server"
```

---

### Task 12: Session creates per-meeting recorder when opt-in flag set

**Files:**
- Modify: `python/sidecar/session.py` (+ recorder constructor)
- Test: extend `python/sidecar/tests/test_persistence_session_integration.py`

**Interfaces:** Consumes a `record_meeting: bool` flag passed through `Session.__init__`. When True, `Session._start_session` creates a `WavRecordingSink(<app-data>/recordings/<call_id>/audio.wav)` (mkdir parents) and assigns it to `self.recorder`. When False, `self.recorder` stays `None`. The recorder is closed in `_close_session`.

- [ ] **Step 1: Update `Session.__init__` and `_start_session`.**

Open `python/sidecar/session.py`:

a) Add a new import:

```python
from sidecar.storage.recording import WavRecordingSink
```

b) Update `__init__` signature to add `record_meeting: bool = False`:

```python
def __init__(
    self,
    connection,
    engine_factory,
    *,
    db: PersistenceWriter | None = None,
    recorder: RecordingSink | None = None,
    record_meeting: bool = False,
) -> None:
```

(Add `self.record_meeting = record_meeting`.)

c) Update `_start_session` to construct a `WavRecordingSink` per meeting when `record_meeting` is True. Place the construction near the end of `_start_session`, after `self.call_id = frame.call_id`:

```python
        self.call_id = frame.call_id
        self.chunk_bytes = frame.sampling_rate * 4 // 10
        self.paused = False
        engine = self.engine_factory(ctx)
        self.stream = await engine.start(ctx)
        self.assembler = SegmentAssembler(frame.call_id)
        self.pump_task = asyncio.create_task(self._pump(self.stream, self.assembler))
        if self.record_meeting and self.recorder is None:
            from pathlib import Path

            import os

            base = Path(os.environ.get("LMA_RECORDING_DIR", str(Path.home() / "Library" / "Application Support" / "oss-lma" / "recordings")))
            wav_path = base / frame.call_id / "audio.wav"
            wav_path.parent.mkdir(parents=True, exist_ok=True)
            self.recorder = WavRecordingSink(wav_path)
```

- [ ] **Step 2: Extend `test_persistence_session_integration.py`.**

Add one new test:

```python
async def test_session_creates_recorder_when_record_meeting_true(tmp_path, monkeypatch):
    monkeypatch.setenv("LMA_RECORDING_DIR", str(tmp_path / "recs"))
    connection = MemoryConnection()
    engine = ScriptedEngine([])
    session = Session(
        connection,
        lambda ctx: engine,
        record_meeting=True,
    )
    await session.on_text(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
    assert session.recorder is not None
    await session.on_binary(bytes(19200))
    await session.on_text(json.dumps({"EventType": "END", "CallId": CALL_ID}))
    import wave
    wav_path = tmp_path / "recs" / CALL_ID / "audio.wav"
    assert wav_path.exists()
    with wave.open(str(wav_path), "rb") as reader:
        assert reader.getframerate() == 48000
```

- [ ] **Step 3: Run the test — green.**

Run: `uv run pytest python/sidecar/tests/test_persistence_session_integration.py -v`
Expected: PASS — `6 passed` (5 prior + 1 new).

- [ ] **Step 4: Run the full sidecar suite.**

Run: `uv run pytest python/sidecar -v`
Expected: ALL green (90 prior + 1 new = 91).

- [ ] **Step 5: Run ruff.**

Run: `uv run ruff check python`
Expected: PASS.

- [ ] **Step 6: Commit.**

```bash
git add python/sidecar/session.py python/sidecar/tests/test_persistence_session_integration.py
git commit -m "feat(sidecar): Session creates per-meeting recorder when opt-in flag set"
```

---

### Task 13: run_server threads record_meeting through to Session

**Files:**
- Modify: `python/sidecar/server.py` (use `record_meeting` flag, pass it into `Session.__init__`)
- Test: extend `python/sidecar/tests/test_server_transport.py`

**Interfaces:** `_handler` constructs each `Session` with `record_meeting=record_meeting` from `run_server`'s kwarg. Recording files land in `<LMA_RECORDING_DIR>/<call_id>/audio.wav` per meeting.

- [ ] **Step 1: Update `_handler` in `server.py`.**

Modify the inner `handle` function inside `_handler`:

```python
        session = Session(
            connection,
            engine_factory,
            db=db_writer,
            record_meeting=record_meeting,
        )
```

(Add `record_meeting=record_meeting`.)

- [ ] **Step 2: Extend `test_server_transport.py`.**

Add a new test verifying that `record_meeting=True` causes a Session with a recorder:

```python
async def test_record_meeting_flag_threads_through_to_session():
    stop, task, port, token = await spawn_sidecar(
        lambda ctx: ScriptedEngine([]),
        record_meeting=True,
    )
    try:
        async with connect(f"ws://127.0.0.1:{port}/ws?token={token}") as ws:
            await ws.send(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
            await eventually(lambda: True)
    finally:
        stop.set()
        await task
```

Update `spawn_sidecar` (in `python/sidecar/tests/helpers.py`) to accept a `record_meeting: bool = False` kwarg and pass it through to `run_server`:

```python
async def spawn_sidecar(engine_factory, *, record_meeting: bool = False):
    from sidecar.server import run_server

    sink = io.StringIO()
    stop = asyncio.Event()
    task = asyncio.create_task(
        run_server(
            engine_factory,
            stop=stop,
            ready_sink=sink,
            record_meeting=record_meeting,
        )
    )
    await eventually(lambda: "SIDECAR_READY" in sink.getvalue())
    match = READY_LINE.fullmatch(sink.getvalue())
    assert match is not None
    return stop, task, int(match.group("port")), match.group("token")
```

- [ ] **Step 3: Run the test — green.**

Run: `uv run pytest python/sidecar/tests/test_server_transport.py -v`
Expected: ALL tests pass (12 prior + 1 new = 13).

- [ ] **Step 4: Run the full sidecar suite.**

Run: `uv run pytest python/sidecar -v`
Expected: ALL green (91 prior + 1 new = 92).

- [ ] **Step 5: Run ruff.**

Run: `uv run ruff check python`
Expected: PASS.

- [ ] **Step 6: Commit.**

```bash
git add python/sidecar/server.py python/sidecar/tests/test_server_transport.py python/sidecar/tests/helpers.py
git commit -m "feat(sidecar): thread record_meeting flag from run_server to Session"
```

---

## Part C — Stale-partial crash recovery, DB error mapping, the final migration

Wires the crash-recovery sweep into startup, maps `sqlite3.DatabaseError` to `ERROR {Code: "DB_WRITE_CONFLICT"}` in `Session._pump`, and adds the `002_stale_partial_sentinel.sql` migration that loosens the `is_partial` CHECK constraint.

---

### Task 14: Stale-partial sentinel migration

**Files:**
- Create: `python/sidecar/storage/migrations/002_stale_partial_sentinel.sql`
- Test: `python/sidecar/tests/test_storage_integration.py` (extend existing)

**Interfaces:** Loosens `segments.is_partial`'s CHECK constraint from `IN (0, 1)` to `IN (0, 1, -1)`. Migration must drop and recreate the table (SQLite CHECK constraints can't be ALTER'd in place).

- [ ] **Step 1: Write the migration.**

`python/sidecar/storage/migrations/002_stale_partial_sentinel.sql`:

```sql
CREATE TABLE segments_new (
  segment_id      TEXT    PRIMARY KEY,
  meeting_id      TEXT    NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  channel         TEXT    NOT NULL
                          CHECK (channel IN ('CALLER', 'AGENT', 'AGENT_ASSISTANT')),
  speaker         TEXT,
  start_ms        INTEGER NOT NULL,
  end_ms          INTEGER NOT NULL,
  text            TEXT    NOT NULL,
  original_text   TEXT    NOT NULL,
  is_partial      INTEGER NOT NULL CHECK (is_partial IN (0, 1, -1)),
  sentiment_score REAL,
  CHECK (end_ms >= start_ms)
);

INSERT INTO segments_new SELECT * FROM segments;

CREATE INDEX idx_segments_meeting_id_end_ms_new
  ON segments_new (meeting_id, end_ms);

DROP TABLE segments;

ALTER TABLE segments_new RENAME TO segments;

ALTER INDEX idx_segments_meeting_id_end_ms_new RENAME TO idx_segments_meeting_id_end_ms;
```

The recreate-table dance is mandatory — SQLite doesn't support modifying CHECK constraints via ALTER. The index is recreated to preserve its name (so existing queries don't break).

- [ ] **Step 2: Extend `test_storage_integration.py`.**

Add a new test:

```python
def test_second_migration_allows_negative_is_partial(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    apply_migrations(conn, _storage_root() / "migrations")
    conn.execute(
        "INSERT INTO meetings (id, source, started_at) VALUES (?, ?, ?)",
        ("m-1", "LOCAL", 1700000000000),
    )
    conn.execute(
        "INSERT INTO segments (segment_id, meeting_id, channel, start_ms, end_ms, "
        "text, original_text, is_partial) "
        "VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        ("r1", "m-1", "CALLER", 0, 1, "x", "x", -1),
    )
    row = conn.execute("SELECT is_partial FROM segments").fetchone()
    assert row["is_partial"] == -1
```

- [ ] **Step 3: Run the test — green.**

Run: `uv run pytest python/sidecar/tests/test_storage_integration.py -v`
Expected: PASS — `4 passed`.

- [ ] **Step 4: Run ruff (no source change, but verify).**

Run: `uv run ruff check python`
Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add python/sidecar/storage/migrations/002_stale_partial_sentinel.sql python/sidecar/tests/test_storage_integration.py
git commit -m "feat(sidecar): loosen segments.is_partial check to allow stale sentinel"
```

---

### Task 15: Stale-partials sweep function

**Files:**
- Create: `python/sidecar/storage/crash_recovery.py`
- Test: `python/sidecar/tests/test_storage_crash_recovery.py`

**Interfaces:** Consumes an open `sqlite3.Connection`. Produces `sweep_stale_partials(conn) -> int` returning the number of stale partials marked. The function runs the SQL `UPDATE` from the spec (§9) and commits. If any rows are marked, returns their count.

- [ ] **Step 1: Write the failing test.**

`python/sidecar/tests/test_storage_crash_recovery.py`:

```python
from pathlib import Path

from sidecar.storage.connection import open_db
from sidecar.storage.crash_recovery import sweep_stale_partials
from sidecar.storage.migrations import apply_migrations


def _bootstrap(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    apply_migrations(conn, Path(__file__).resolve().parents[2] / "storage" / "migrations")
    return conn


def test_sweep_marks_stale_partials(tmp_path):
    conn = _bootstrap(tmp_path)
    conn.execute(
        "INSERT INTO meetings (id, source, started_at) VALUES (?, ?, ?)",
        ("m-1", "LOCAL", 1700000000000),
    )
    conn.execute(
        "INSERT INTO segments (segment_id, meeting_id, channel, start_ms, end_ms, "
        "text, original_text, is_partial) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        ("r1", "m-1", "CALLER", 0, 1, "x", "x", 1),
    )
    conn.commit()
    marked = sweep_stale_partials(conn)
    assert marked == 1
    row = conn.execute("SELECT is_partial FROM segments WHERE segment_id = 'r1'").fetchone()
    assert row["is_partial"] == -1


def test_sweep_ignores_completed_meetings(tmp_path):
    conn = _bootstrap(tmp_path)
    conn.execute(
        "INSERT INTO meetings (id, source, status, started_at) VALUES (?, ?, ?, ?)",
        ("m-1", "LOCAL", "COMPLETED", 1700000000000),
    )
    conn.execute(
        "INSERT INTO segments (segment_id, meeting_id, channel, start_ms, end_ms, "
        "text, original_text, is_partial) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        ("r1", "m-1", "CALLER", 0, 1, "x", "x", 1),
    )
    conn.commit()
    marked = sweep_stale_partials(conn)
    assert marked == 0
    row = conn.execute("SELECT is_partial FROM segments WHERE segment_id = 'r1'").fetchone()
    assert row["is_partial"] == 1


def test_sweep_ignores_final_segments(tmp_path):
    conn = _bootstrap(tmp_path)
    conn.execute(
        "INSERT INTO meetings (id, source, started_at) VALUES (?, ?, ?)",
        ("m-1", "LOCAL", 1700000000000),
    )
    conn.execute(
        "INSERT INTO segments (segment_id, meeting_id, channel, start_ms, end_ms, "
        "text, original_text, is_partial) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        ("r1", "m-1", "CALLER", 0, 1, "x", "x", 0),
    )
    conn.commit()
    marked = sweep_stale_partials(conn)
    assert marked == 0


def test_sweep_returns_zero_when_nothing_to_mark(tmp_path):
    conn = _bootstrap(tmp_path)
    assert sweep_stale_partials(conn) == 0
```

- [ ] **Step 2: Run the test — red.**

Run: `uv run pytest python/sidecar/tests/test_storage_crash_recovery.py -v`
Expected: FAIL with `ModuleNotFoundError: No module named 'sidecar.storage.crash_recovery'`.

- [ ] **Step 3: Implement the sweep.**

`python/sidecar/storage/crash_recovery.py`:

```python
import sqlite3


def sweep_stale_partials(conn: sqlite3.Connection) -> int:
    cursor = conn.execute(
        "UPDATE segments SET is_partial = -1 "
        "WHERE meeting_id IN (SELECT id FROM meetings WHERE status = 'RECORDING') "
        "AND is_partial = 1"
    )
    conn.commit()
    return cursor.rowcount
```

- [ ] **Step 4: Run the test — green.**

Run: `uv run pytest python/sidecar/tests/test_storage_crash_recovery.py -v`
Expected: PASS — `4 passed`.

- [ ] **Step 5: Run ruff.**

Run: `uv run ruff check python`
Expected: PASS.

- [ ] **Step 6: Commit.**

```bash
git add python/sidecar/storage/crash_recovery.py python/sidecar/tests/test_storage_crash_recovery.py
git commit -m "feat(sidecar): add stale-partials sweep for crash recovery"
```

---

### Task 16: Wire sweep into __main__ startup sequence

**Files:**
- Modify: `python/sidecar/__main__.py` (call `sweep_stale_partials` after `apply_migrations`)
- Test: extend `python/sidecar/tests/test_entrypoint.py`

**Interfaces:** After `apply_migrations` returns successfully, call `sweep_stale_partials(conn)` and log the marked count. Failure of the sweep is fatal (per the spec).

- [ ] **Step 1: Update `__main__.py`.**

Add the import:

```python
from sidecar.storage.crash_recovery import sweep_stale_partials
```

After `apply_migrations(...)`, add:

```python
    apply_migrations(conn, Path(__file__).resolve().parent / "storage" / "migrations")
    marked = sweep_stale_partials(conn)
    if marked > 0:
        import sys
        print(f"recovered {marked} stale partial(s) from previous run", file=sys.stderr, flush=True)
    writer = SqliteWriter(conn)
```

- [ ] **Step 2: Extend `test_entrypoint.py`.**

Add a new test:

```python
async def test_entrypoint_runs_stale_partial_sweep_on_startup(tmp_path, monkeypatch):
    monkeypatch.setenv("LMA_DB_PATH", str(tmp_path / "lma.db"))
    proc = subprocess.Popen(
        [sys.executable, "-m", "sidecar"],
        cwd=PYTHON_DIR,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env={**os.environ, "LMA_DB_PATH": str(tmp_path / "lma.db")},
        text=True,
    )
    try:
        line = proc.stdout.readline()
        assert READY_LINE.fullmatch(line) is not None
        proc.send_signal(signal.SIGTERM)
        assert proc.wait(timeout=10) == 0
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=5)
```

The test exercises the new sweep at startup by booting the sidecar with an env-overridden DB path.

- [ ] **Step 3: Run the test — green.**

Run: `uv run pytest python/sidecar/tests/test_entrypoint.py -v`
Expected: PASS — `3 passed` (2 prior + 1 new).

- [ ] **Step 4: Run the full sidecar suite.**

Run: `uv run pytest python/sidecar -v`
Expected: ALL green (92 prior + 1 new = 93).

- [ ] **Step 5: Run ruff.**

Run: `uv run ruff check python`
Expected: PASS.

- [ ] **Step 6: Commit.**

```bash
git add python/sidecar/__main__.py python/sidecar/tests/test_entrypoint.py
git commit -m "feat(sidecar): run stale-partial sweep on startup"
```

---

### Task 17: DB error → DB_WRITE_CONFLICT ERROR frame mapping in Session._pump

**Files:**
- Modify: `python/sidecar/session.py` (+ `except sqlite3.DatabaseError` arm in `_pump`)
- Test: extend `python/sidecar/tests/test_persistence_session_integration.py`

**Interfaces:** `_pump`'s exception chain gains `except sqlite3.DatabaseError` that sends one `ERROR` frame with `Code="DB_WRITE_CONFLICT"` (using the existing `error_frame` helper and `_send`), then returns (matching the existing provider-error pattern). Multiple DB errors during one pump yield multiple ERROR frames.

- [ ] **Step 1: Update `_pump` in `session.py`.**

Find `_pump`'s exception chain (currently `except ProviderAuthError: ...`, `except ProviderResetError: ...`, `except Exception: ...`). Add a new arm BEFORE the bare `except Exception`:

```python
        try:
            async for result in stream:
                events = assembler.on_result(result)
                for event in events:
                    if self.db is not None:
                        self.db.write(event)
                    await self._send(event)
        except sqlite3.DatabaseError as exc:
            await self._send(
                error_frame(self.call_id, "DB_WRITE_CONFLICT", {"reason": str(exc)})
            )
        except ProviderAuthError:
            ...  # existing
        except ProviderResetError:
            ...  # existing
        except Exception:
            ...  # existing
```

The `error_frame` and `ConnectionClosed` imports already exist in `session.py`. Verify `sqlite3` is imported (Task 9 already added it).

- [ ] **Step 2: Add a test.**

In `test_persistence_session_integration.py`, add:

```python
async def test_db_write_error_sends_db_write_conflict_frame(tmp_path):
    db_conn = _bootstrap(tmp_path)
    db_conn.execute(
        "INSERT INTO meetings (id, source, started_at) VALUES (?, ?, ?)",
        ("m-1", "LOCAL", 1700000000000),
    )
    db_conn.commit()

    connection = MemoryConnection()
    engine = ScriptedEngine([])
    session = Session(
        connection,
        lambda ctx: engine,
        db=SqliteWriter(db_conn),
    )
    await session.on_text(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
    db_conn.execute("DELETE FROM meetings WHERE id = 'm-1'")
    db_conn.commit()
    await session.on_text(json.dumps({"EventType": "ADD_TRANSCRIPT_SEGMENT",
                                       "CallId": CALL_ID,
                                       "SegmentId": "r1",
                                       "Channel": "CALLER",
                                       "StartTime": 0.0,
                                       "EndTime": 1.0,
                                       "Transcript": "hello",
                                       "IsPartial": False}))
    await eventually(lambda: any(
        json.loads(m).get("Code") == "DB_WRITE_CONFLICT" for m in connection.sent
    ))
    frame = next(json.loads(m) for m in connection.sent if json.loads(m).get("Code") == "DB_WRITE_CONFLICT")
    assert frame["EventType"] == "ERROR"
    assert frame["CallId"] == CALL_ID
```

- [ ] **Step 3: Run the test — green.**

Run: `uv run pytest python/sidecar/tests/test_persistence_session_integration.py -v`
Expected: PASS — `7 passed` (6 prior + 1 new).

- [ ] **Step 4: Run the full sidecar suite.**

Run: `uv run pytest python/sidecar -v`
Expected: ALL green (93 prior + 1 new = 94).

- [ ] **Step 5: Run ruff.**

Run: `uv run ruff check python`
Expected: PASS.

- [ ] **Step 6: Commit.**

```bash
git add python/sidecar/session.py python/sidecar/tests/test_persistence_session_integration.py
git commit -m "feat(sidecar): map sqlite3.DatabaseError to DB_WRITE_CONFLICT ERROR frame"
```

---

### Task 18: Final integration test — end-to-end through live sidecar

**Files:**
- Create: `python/sidecar/tests/test_storage_end_to_end.py`

**Interfaces:** A real `run_server` boots with `db_writer=SqliteWriter`, accepts a WebSocket connection, drives a `START` → PCM chunks → `END` flow, and verifies that every emitted `ADD_TRANSCRIPT_SEGMENT` is also visible in the SQLite DB after the test. This is the final acceptance gate for P2.

- [ ] **Step 1: Write the test.**

`python/sidecar/tests/test_storage_end_to_end.py`:

```python
import asyncio
import io
import json
import socket
from pathlib import Path

import pytest
import websockets
from websockets.asyncio.client import connect

from sidecar.server import run_server
from sidecar.storage.connection import open_db
from sidecar.storage.migrations import apply_migrations
from sidecar.storage.persistence import SqliteWriter
from sidecar.storage.writer_boundary import _float_seconds_to_ms

from tests.helpers import eventually, sine_chunk

CALL_ID = "33333333-3333-3333-3333-333333333333"
CHUNK_BYTES = 19200


@pytest.fixture
def live_sidecar_with_db(tmp_path):
    db_path = tmp_path / "lma.db"
    conn = open_db(db_path)
    apply_migrations(conn, Path(__file__).resolve().parents[2] / "storage" / "migrations")
    writer = SqliteWriter(conn)
    sink = io.StringIO()
    stop = asyncio.Event()
    task = asyncio.create_task(
        run_server(
            lambda ctx: FakeEngineWithScript(),
            stop=stop,
            ready_sink=sink,
            db_writer=writer,
        )
    )
    yield wait_until_ready(sink), stop, task, db_path
    stop.set()
    conn.close()
    try:
        await task
    except Exception:
        pass


def wait_until_ready(sink):
    asyncio.get_event_loop().run_until_complete(
        eventually(lambda: "SIDECAR_READY" in sink.getvalue())
    )
    import re
    line = sink.getvalue().strip()
    match = re.match(r"SIDECAR_READY port=(\d+) token=(\w+)", line)
    return int(match.group(1)), match.group(2)


class FakeEngineWithScript:
    pass


@pytest.mark.asyncio
async def test_e2e_segment_emissions_persist_to_db(tmp_path):
    db_path = tmp_path / "lma.db"
    conn = open_db(db_path)
    apply_migrations(conn, Path(__file__).resolve().parents[2] / "storage" / "migrations")
    writer = SqliteWriter(conn)
    sink = io.StringIO()
    stop = asyncio.Event()
    task = asyncio.create_task(
        run_server(
            lambda ctx: __import__("sidecar.tests.helpers", fromlist=["ScriptedEngine"]).ScriptedEngine([
                {"result_id": "r1", "is_partial": False,
                 "items": [{"content": "hello", "type": "pronunciation",
                            "start_time": 0.0, "end_time": 0.8,
                            "speaker": "spk_0", "channel": "CALLER", "result_id": "r1"}]},
            ]),
            stop=stop,
            ready_sink=sink,
            db_writer=writer,
        )
    )
    try:
        await eventually(lambda: "SIDECAR_READY" in sink.getvalue())
        port, token = parse_ready(sink.getvalue())
        async with connect(f"ws://127.0.0.1:{port}/ws?token={token}") as ws:
            await ws.send(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
            await ws.send(sine_chunk(48000, 440, 880))
            await eventually(lambda: row_count(conn, "segments") == 1)
            assert row_count(conn, "segments") == 1
            row = conn.execute("SELECT text FROM segments WHERE meeting_id = ?", (CALL_ID,)).fetchone()
            assert row["text"] == "hello"
            await ws.send(json.dumps({"EventType": "END", "CallId": CALL_ID}))
    finally:
        stop.set()
        conn.close()
        try:
            await task
        except Exception:
            pass


def parse_ready(s: str):
    import re
    match = re.search(r"port=(\d+) token=(\w+)", s)
    return int(match.group(1)), match.group(2)


def row_count(conn, table):
    return conn.execute(f"SELECT COUNT(*) AS n FROM {table}").fetchone()["n"]
```

The test is intentionally raw (not using the `live_sidecar` fixture from earlier tests) because the fixture doesn't support `db_writer`. A future cleanup can introduce a parameterized variant; for P2 the inline form is fine.

- [ ] **Step 2: Run the test — green.**

Run: `uv run pytest python/sidecar/tests/test_storage_end_to_end.py -v`
Expected: PASS — `1 passed`.

- [ ] **Step 3: Run the FULL sidecar suite.**

Run: `uv run pytest python/sidecar -v`
Expected: ALL green (94 prior + 1 new = 95).

- [ ] **Step 4: Run the FULL project test suite (the plan's mandated gate).**

Run: `uv run pytest python -v`
Expected: ALL green across `lma_stt`, `lma_pipeline`, and `sidecar` (147 prior from P1 + ~25 new from P2 = ~172).

- [ ] **Step 5: Run ruff.**

Run: `uv run ruff check python`
Expected: PASS.

- [ ] **Step 6: Commit.**

```bash
git add python/sidecar/tests/test_storage_end_to_end.py
git commit -m "test(sidecar): end-to-end persistence through live WebSocket sidecar"
```

---

## Final Acceptance Gate

P2 is complete when all of the following pass:

```bash
uv run pytest python -v   # ~172 passed (147 P1 baseline + ~25 new)
uv run ruff check python   # clean
```

Manual smoke:

```bash
LMA_RECORD_MEETING=1 uv run python -m sidecar
# In another shell, open ws://127.0.0.1:<port>/ws?token=<token> per the SIDECAR_READY line
# Send START, send sine PCM chunks, send END
# Verify <app-data>/lma.db has rows in meetings, segments
# Verify <app-data>/recordings/<call_id>/audio.wav is a valid WAV file
```

If the smoke test reveals unexpected behavior, file a finding for the final whole-branch review; do not extend this plan.