import json

import jsonschema
import pytest

from sidecar.frames import (
    INVALID_FRAME_CLOSE_CODE,
    INVALID_FRAME_CODE,
    INVALID_FRAME_CONTEXT,
    AgentQuery,
    End,
    FrameError,
    Pause,
    Resume,
    SpeakerChange,
    Start,
    VpCommand,
    error_frame,
    parse_frame,
    serialize_event,
)

CALL_ID = "11111111-1111-1111-1111-111111111111"
QUERY_ID = "33333333-3333-3333-3333-333333333333"


def test_parses_minimal_start():
    frame = parse_frame(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
    assert frame == Start(CALL_ID, 48000, False, False)


def test_parses_start_with_diarization_flags():
    frame = parse_frame(json.dumps({
        "EventType": "START",
        "CallId": CALL_ID,
        "SamplingRate": 16000,
        "DiarizeSystemChannel": True,
        "DiarizeMicChannel": True,
    }))
    assert frame == Start(CALL_ID, 16000, True, True)


def test_parses_speaker_change():
    frame = parse_frame(json.dumps({
        "EventType": "SPEAKER_CHANGE",
        "CallId": CALL_ID,
        "Channel": "AGENT",
        "ActiveSpeaker": "Ayu",
    }))
    assert frame == SpeakerChange(CALL_ID, "AGENT", "Ayu")


def test_parses_pause_resume_end():
    for event_type, expected in [
        ("PAUSE", Pause(CALL_ID)),
        ("RESUME", Resume(CALL_ID)),
        ("END", End(CALL_ID)),
    ]:
        assert parse_frame(json.dumps({"EventType": event_type, "CallId": CALL_ID})) == expected


def test_parses_agent_query_with_history():
    frame = parse_frame(json.dumps({
        "EventType": "AGENT_QUERY",
        "CallId": CALL_ID,
        "QueryId": QUERY_ID,
        "Message": "What did we just discuss?",
        "History": [{"Role": "user", "Content": "hi"}],
    }))
    assert frame == AgentQuery(CALL_ID, QUERY_ID, "What did we just discuss?", [{"Role": "user", "Content": "hi"}])


def test_parses_agent_query_without_history():
    frame = parse_frame(json.dumps({"EventType": "AGENT_QUERY", "CallId": CALL_ID, "QueryId": QUERY_ID, "Message": "q"}))
    assert frame == AgentQuery(CALL_ID, QUERY_ID, "q", [])


def test_parses_vp_command():
    frame = parse_frame(json.dumps({
        "EventType": "VP_COMMAND",
        "TaskId": "t-1",
        "Command": "CLICK",
        "Payload": {"x": 412, "y": 380},
    }))
    assert frame == VpCommand("t-1", "CLICK", {"x": 412, "y": 380})


def test_downstream_event_sent_upstream_is_unknown():
    downstream = {
        "EventType": "ADD_TRANSCRIPT_SEGMENT",
        "CallId": CALL_ID,
        "SegmentId": "s1",
        "Channel": "CALLER",
        "StartTime": 0.0,
        "EndTime": 1.0,
        "Transcript": "x",
        "IsPartial": False,
    }
    with pytest.raises(FrameError):
        parse_frame(json.dumps(downstream))


def test_unknown_event_type_raises():
    with pytest.raises(FrameError):
        parse_frame(json.dumps({"EventType": "NOPE", "CallId": CALL_ID}))


def test_schema_invalid_start_raises():
    with pytest.raises(FrameError):
        parse_frame(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 4000}))


def test_missing_required_field_raises():
    with pytest.raises(FrameError):
        parse_frame(json.dumps({"EventType": "START", "SamplingRate": 48000}))


def test_non_json_frame_raises():
    with pytest.raises(FrameError):
        parse_frame("{oops")


def test_non_object_frame_raises():
    with pytest.raises(FrameError):
        parse_frame("[1]")


def test_decision_constants():
    assert INVALID_FRAME_CODE == "LINK_DISCONNECTED"
    assert INVALID_FRAME_CONTEXT == {"reason": "invalid-frame"}
    assert INVALID_FRAME_CLOSE_CODE == 1008


def test_error_frame_round_trips_through_schema():
    raw = serialize_event(error_frame(CALL_ID, INVALID_FRAME_CODE, INVALID_FRAME_CONTEXT))
    assert json.loads(raw) == {
        "EventType": "ERROR",
        "CallId": CALL_ID,
        "Code": "LINK_DISCONNECTED",
        "Context": {"reason": "invalid-frame"},
    }


def test_error_frame_rejects_code_outside_catalog():
    with pytest.raises(jsonschema.ValidationError):
        serialize_event(error_frame(CALL_ID, "NOT_IN_ERRORS_YAML"))


def test_serialize_passes_valid_transcript_segment():
    segment = {
        "EventType": "ADD_TRANSCRIPT_SEGMENT",
        "CallId": CALL_ID,
        "SegmentId": "s1",
        "Channel": "CALLER",
        "Speaker": "spk_0",
        "StartTime": 0.0,
        "EndTime": 3.8,
        "Transcript": "hello",
        "IsPartial": True,
    }
    assert json.loads(serialize_event(segment)) == segment


def test_serialize_rejects_segment_missing_required_field():
    broken = {
        "EventType": "ADD_TRANSCRIPT_SEGMENT",
        "CallId": CALL_ID,
        "SegmentId": "s1",
        "Channel": "CALLER",
        "StartTime": 0.0,
        "EndTime": 1.0,
        "Transcript": "hello",
    }
    with pytest.raises(jsonschema.ValidationError):
        serialize_event(broken)
