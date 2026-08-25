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