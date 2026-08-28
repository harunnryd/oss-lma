import sqlite3
import time

from sidecar.storage.writer_boundary import (
    normalize_agent_assist,
    normalize_agent_token,
    normalize_deleted_segment,
    normalize_meeting_ended,
    normalize_meeting_started,
    normalize_segment,
    normalize_summary,
    normalize_thinking_step,
)


def write_meeting_started(
    conn: sqlite3.Connection, ev: dict, *, return_offset: bool = False
) -> int | None:
    if return_offset:
        values, offset = normalize_meeting_started(ev, conn=conn, return_offset=True)
    else:
        values = normalize_meeting_started(ev)
        offset = None
    cursor = conn.execute(
        "INSERT OR IGNORE INTO meetings (id, source, started_at) VALUES (?, ?, ?)",
        values,
    )
    if cursor.rowcount == 0:
        conn.execute(
            "UPDATE meetings SET status = 'RECORDING', ended_at = NULL, "
            "duration_ms = NULL, reconnect_attempts = 0 WHERE id = ?",
            (ev["CallId"],),
        )
    conn.commit()
    return offset


def read_max_segment_end_ms(conn: sqlite3.Connection, call_id: str) -> int | None:
    row = conn.execute(
        "SELECT MAX(end_ms) AS max_end_ms FROM segments WHERE meeting_id = ?",
        (call_id,),
    ).fetchone()
    if row is None:
        return None
    return row["max_end_ms"]


def write_meeting_ended(conn: sqlite3.Connection, ev: dict) -> None:
    call_id, status, ended_at = normalize_meeting_ended(ev)
    conn.execute(
        "UPDATE meetings SET status = CASE WHEN status = 'FAILED' THEN status ELSE ? END, "
        "ended_at = ?, duration_ms = COALESCE(duration_ms, ? - started_at) "
        "WHERE id = ?",
        (status, ended_at, ended_at, call_id),
    )
    conn.commit()


def write_segment(conn: sqlite3.Connection, ev: dict, *, time_offset_ms: int = 0) -> None:
    values = normalize_segment(ev, time_offset_ms=time_offset_ms)
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


def write_meeting_failed(conn: sqlite3.Connection, ev: dict) -> None:
    ended_at = int(time.time() * 1000)
    conn.execute(
        "UPDATE meetings SET status = 'FAILED', ended_at = ?, last_reconnect_at = ? WHERE id = ?",
        (ended_at, ended_at, ev["CallId"]),
    )
    conn.commit()


def write_meeting_started_update_offset(
    conn: sqlite3.Connection,
    ev: dict,
    *,
    time_offset_ms: int,
    reconnect_attempts: int | None = None,
) -> None:
    if reconnect_attempts is None:
        conn.execute(
            "UPDATE meetings SET time_offset_ms = ? WHERE id = ?",
            (time_offset_ms, ev["CallId"]),
        )
    else:
        conn.execute(
            "UPDATE meetings SET time_offset_ms = ?, reconnect_attempts = ?, "
            "last_reconnect_at = ? WHERE id = ?",
            (time_offset_ms, reconnect_attempts, int(time.time() * 1000), ev["CallId"]),
        )
    conn.commit()


def write_deleted_segment(conn: sqlite3.Connection, ev: dict) -> None:
    call_id, segment_id = normalize_deleted_segment(ev)
    conn.execute(
        "DELETE FROM segments WHERE meeting_id = ? AND segment_id = ?",
        (call_id, segment_id),
    )
    conn.commit()


def dispatch_write(conn: sqlite3.Connection, ev: dict, *, time_offset_ms: int = 0) -> None:
    event_type = ev.get("EventType")
    if event_type == "START":
        write_meeting_started(conn, ev)
    elif event_type == "END":
        write_meeting_ended(conn, ev)
    elif event_type == "ADD_TRANSCRIPT_SEGMENT":
        write_segment(conn, ev, time_offset_ms=time_offset_ms)
    elif event_type == "ADD_SUMMARY":
        write_summary(conn, ev)
    elif event_type == "ADD_AGENT_ASSIST":
        write_agent_assist(conn, ev)
    elif event_type == "AGENT_TOKEN":
        write_agent_token(conn, ev)
    elif event_type == "THINKING_STEP":
        write_thinking_step(conn, ev)
    elif event_type == "DELETE_TRANSCRIPT_SEGMENT":
        write_deleted_segment(conn, ev)
    else:
        raise ValueError(f"unhandled event type for persistence: {event_type!r}")
