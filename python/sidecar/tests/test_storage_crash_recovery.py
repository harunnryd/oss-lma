from pathlib import Path

from sidecar.storage.connection import open_db
from sidecar.storage.crash_recovery import sweep_stale_partials
from sidecar.storage.migrations import apply_migrations


def _bootstrap(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    apply_migrations(conn, Path(__file__).resolve().parents[1] / "storage" / "migrations")
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