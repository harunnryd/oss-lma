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
    apply_migrations(
        conn, Path(__file__).resolve().parents[2] / "sidecar" / "storage" / "migrations"
    )
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
    write_segment(
        conn,
        {
            "EventType": "ADD_TRANSCRIPT_SEGMENT",
            "CallId": "m-1",
            "SegmentId": "r1-w0-r0",
            "Channel": "CALLER",
            "StartTime": 0.0,
            "EndTime": 1.5,
            "Transcript": "hello",
            "IsPartial": True,
        },
    )
    write_segment(
        conn,
        {
            "EventType": "ADD_TRANSCRIPT_SEGMENT",
            "CallId": "m-1",
            "SegmentId": "r1-w0-r0",
            "Channel": "CALLER",
            "StartTime": 0.0,
            "EndTime": 1.6,
            "Transcript": "hello there",
            "IsPartial": False,
        },
    )
    rows = conn.execute("SELECT segment_id, is_partial, text FROM segments").fetchall()
    assert len(rows) == 1
    assert rows[0]["is_partial"] == 0
    assert rows[0]["text"] == "hello there"


def test_write_segment_omits_speaker_when_absent(tmp_path):
    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    write_segment(
        conn,
        {
            "EventType": "ADD_TRANSCRIPT_SEGMENT",
            "CallId": "m-1",
            "SegmentId": "r1",
            "Channel": "AGENT",
            "StartTime": 0.0,
            "EndTime": 1.0,
            "Transcript": "hi",
            "IsPartial": False,
        },
    )
    row = conn.execute("SELECT speaker FROM segments WHERE segment_id = 'r1'").fetchone()
    assert row["speaker"] is None


def test_write_summary_inserts(tmp_path):
    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    write_summary(
        conn,
        {
            "EventType": "ADD_SUMMARY",
            "CallId": "m-1",
            "Section": "action_items",
            "SummaryText": "Caller agreed.",
        },
    )
    row = conn.execute("SELECT section, content FROM summaries WHERE meeting_id = 'm-1'").fetchone()
    assert row["section"] == "action_items"
    assert row["content"] == "Caller agreed."


def test_write_agent_assist_inserts(tmp_path):
    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    write_agent_assist(
        conn,
        {
            "EventType": "ADD_AGENT_ASSIST",
            "CallId": "m-1",
            "SegmentId": "asst_1",
            "Transcript": "discount",
            "IsPartial": True,
        },
    )
    row = conn.execute(
        "SELECT segment_id, is_partial FROM agent_assists WHERE segment_id = 'asst_1'"
    ).fetchone()
    assert row["segment_id"] == "asst_1"
    assert row["is_partial"] == 1


def test_write_agent_token_inserts(tmp_path):
    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    write_agent_token(
        conn,
        {
            "EventType": "AGENT_TOKEN",
            "CallId": "m-1",
            "QueryId": "q-1",
            "Seq": 0,
            "Delta": "Hel",
        },
    )
    row = conn.execute(
        "SELECT query_id, seq, delta FROM agent_tokens WHERE query_id = 'q-1'"
    ).fetchone()
    assert row["seq"] == 0
    assert row["delta"] == "Hel"


def test_write_thinking_step_inserts(tmp_path):
    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    write_thinking_step(
        conn,
        {
            "EventType": "THINKING_STEP",
            "CallId": "m-1",
            "QueryId": "q-1",
            "Seq": 1,
            "StepType": "tool_use",
            "Content": "Searching",
        },
    )
    row = conn.execute(
        "SELECT query_id, step_type, content FROM thinking_steps WHERE seq = 1"
    ).fetchone()
    assert row["step_type"] == "tool_use"
    assert row["content"] == "Searching"


def test_write_meeting_ended_updates_status(tmp_path):
    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    started_at = conn.execute("SELECT started_at FROM meetings WHERE id = 'm-1'").fetchone()[
        "started_at"
    ]
    from datetime import datetime, UTC

    ended_iso = (
        datetime.fromtimestamp((started_at + 1000) / 1000, tz=UTC)
        .isoformat()
        .replace("+00:00", "Z")
    )
    write_meeting_ended(conn, {"EventType": "END", "CallId": "m-1", "CreatedAt": ended_iso})
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


def test_write_meeting_started_preserves_existing_row(tmp_path):
    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    conn.execute("UPDATE meetings SET time_offset_ms = ? WHERE id = ?", (7777, "m-1"))
    conn.commit()
    second = write_meeting_started(
        conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000}
    )
    assert second is None
    row = conn.execute("SELECT time_offset_ms FROM meetings WHERE id = ?", ("m-1",)).fetchone()
    assert row["time_offset_ms"] == 7777


def test_write_meeting_started_returns_offset_when_requested(tmp_path):
    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    conn.execute("UPDATE meetings SET time_offset_ms = 4321 WHERE id = 'm-1'")
    conn.commit()
    offset = write_meeting_started(
        conn,
        {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000},
        return_offset=True,
    )
    assert offset == 4321


def test_write_meeting_failed_sets_status_and_ended_at(tmp_path):
    from sidecar.storage.writers import write_meeting_failed

    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    write_meeting_failed(conn, {"EventType": "FAILED", "CallId": "m-1", "Reason": "test"})
    row = conn.execute("SELECT status, ended_at FROM meetings WHERE id = 'm-1'").fetchone()
    assert row["status"] == "FAILED"
    assert row["ended_at"] is not None and row["ended_at"] > 0


def test_write_meeting_started_update_offset_updates_time_offset(tmp_path):
    from sidecar.storage.writers import write_meeting_started_update_offset

    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    write_meeting_started_update_offset(
        conn, {"EventType": "START", "CallId": "m-1"}, time_offset_ms=9999, reconnect_attempts=2
    )
    row = conn.execute(
        "SELECT time_offset_ms, reconnect_attempts FROM meetings WHERE id = 'm-1'"
    ).fetchone()
    assert row["time_offset_ms"] == 9999
    assert row["reconnect_attempts"] == 2


def test_write_meeting_started_resets_terminal_state_on_resume(tmp_path):
    from sidecar.storage.writers import write_meeting_failed

    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    conn.execute(
        "UPDATE meetings SET time_offset_ms = ?, reconnect_attempts = ?, duration_ms = ? "
        "WHERE id = ?",
        (7777, 4, 12345, "m-1"),
    )
    conn.commit()
    write_meeting_failed(conn, {"EventType": "FAILED", "CallId": "m-1"})
    offset = write_meeting_started(
        conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000}, return_offset=True
    )
    row = conn.execute(
        "SELECT status, ended_at, duration_ms, reconnect_attempts, time_offset_ms "
        "FROM meetings WHERE id = 'm-1'"
    ).fetchone()
    assert offset == 7777
    assert row["status"] == "RECORDING"
    assert row["ended_at"] is None
    assert row["duration_ms"] is None
    assert row["reconnect_attempts"] == 0
    assert row["time_offset_ms"] == 7777


def test_write_meeting_ended_does_not_clobber_failed_status(tmp_path):
    from sidecar.storage.writers import write_meeting_failed

    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    write_meeting_failed(conn, {"EventType": "FAILED", "CallId": "m-1"})
    write_meeting_ended(conn, {"EventType": "END", "CallId": "m-1"})
    row = conn.execute("SELECT status, ended_at FROM meetings WHERE id = 'm-1'").fetchone()
    assert row["status"] == "FAILED"
    assert row["ended_at"] is not None


def test_read_max_segment_end_ms_returns_none_without_segments(tmp_path):
    from sidecar.storage.writers import read_max_segment_end_ms

    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    assert read_max_segment_end_ms(conn, "m-1") is None


def test_read_max_segment_end_ms_returns_largest_end(tmp_path):
    from sidecar.storage.writers import read_max_segment_end_ms

    conn = _bootstrap(tmp_path)
    write_meeting_started(conn, {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    for segment_id, end_time in (("s1", 1.5), ("s2", 4.25), ("s3", 3.0)):
        write_segment(
            conn,
            {
                "EventType": "ADD_TRANSCRIPT_SEGMENT",
                "CallId": "m-1",
                "SegmentId": segment_id,
                "Channel": "CALLER",
                "StartTime": 0.0,
                "EndTime": end_time,
                "Transcript": "hi",
                "IsPartial": False,
            },
        )
    assert read_max_segment_end_ms(conn, "m-1") == 4250
