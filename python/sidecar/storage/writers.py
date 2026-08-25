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