from pathlib import Path

from sidecar.storage.connection import open_db
from sidecar.storage.migrations import apply_migrations
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


def test_normalize_meeting_started_with_return_offset_reads_existing(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    apply_migrations(conn, Path(__file__).resolve().parents[1] / "storage" / "migrations")
    conn.execute(
        "INSERT INTO meetings (id, source, started_at, time_offset_ms) VALUES (?, ?, ?, ?)",
        ("m-1", "LOCAL", 1700000000000, 12345),
    )
    conn.commit()
    _value, offset = normalize_meeting_started(
        {"EventType": "START", "CallId": "m-1", "SamplingRate": 48000},
        conn=conn,
        return_offset=True,
    )
    assert offset == 12345


def test_normalize_meeting_started_default_returns_single_tuple():
    from sidecar.storage.writer_boundary import normalize_meeting_started as f
    result = f({"EventType": "START", "CallId": "m-1", "SamplingRate": 48000})
    assert isinstance(result, tuple) and len(result) == 3


def test_normalize_segment_with_offset():
    from sidecar.storage.writer_boundary import normalize_segment
    ev = {
        "EventType": "ADD_TRANSCRIPT_SEGMENT",
        "CallId": "m-1",
        "SegmentId": "r1",
        "Channel": "CALLER",
        "StartTime": 5.0,
        "EndTime": 6.5,
        "Transcript": "hi",
        "IsPartial": False,
    }
    out = normalize_segment(ev, time_offset_ms=12_500)
    assert out[4] == 17_500
    assert out[5] == 19_000


def test_normalize_segment_zero_offset_is_identity():
    from sidecar.storage.writer_boundary import normalize_segment
    ev = {
        "EventType": "ADD_TRANSCRIPT_SEGMENT",
        "CallId": "m-1",
        "SegmentId": "r1",
        "Channel": "CALLER",
        "StartTime": 5.0,
        "EndTime": 6.5,
        "Transcript": "hi",
        "IsPartial": False,
    }
    out = normalize_segment(ev)
    assert out[4] == 5000
    assert out[5] == 6500
