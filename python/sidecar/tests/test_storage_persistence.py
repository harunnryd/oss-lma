from pathlib import Path

from sidecar.storage.connection import open_db
from sidecar.storage.migrations import apply_migrations
from sidecar.storage.persistence import NullWriter, SqliteWriter


def test_sqlite_writer_dispatches_to_segments(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    apply_migrations(
        conn, Path(__file__).resolve().parents[2] / "sidecar" / "storage" / "migrations"
    )
    conn.execute(
        "INSERT INTO meetings (id, source, started_at) VALUES (?, ?, ?)",
        ("m-1", "LOCAL", 1700000000000),
    )
    conn.commit()
    writer = SqliteWriter(conn)
    writer.write(
        {
            "EventType": "ADD_TRANSCRIPT_SEGMENT",
            "CallId": "m-1",
            "SegmentId": "r1-w0-r0",
            "Channel": "CALLER",
            "StartTime": 0.0,
            "EndTime": 1.0,
            "Transcript": "hello",
            "IsPartial": False,
        }
    )
    row = conn.execute("SELECT text FROM segments WHERE segment_id = 'r1-w0-r0'").fetchone()
    assert row["text"] == "hello"


def test_sqlite_writer_raises_on_db_error(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    apply_migrations(
        conn, Path(__file__).resolve().parents[2] / "sidecar" / "storage" / "migrations"
    )
    writer = SqliteWriter(conn)
    import sqlite3
    import pytest

    with pytest.raises(sqlite3.DatabaseError):
        writer.write(
            {
                "EventType": "ADD_TRANSCRIPT_SEGMENT",
                "CallId": "m-1",
                "SegmentId": "r1-w0-r0",
                "Channel": "CALLER",
                "StartTime": 0.0,
                "EndTime": 1.0,
                "Transcript": "hello",
                "IsPartial": False,
            }
        )


def test_null_writer_does_nothing():
    NullWriter().write({"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
