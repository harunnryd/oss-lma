import base64

import pytest
from jsonschema import Draft202012Validator

from contracts import load_error_codes, load_schema

CALL_ID = "3fa85f64-5717-4562-b3fc-2c963f66afa6"
QUERY_ID = "c0a80101-0000-4000-8000-000000000001"
CREATED_AT = "2026-08-25T09:30:00Z"


def frame(event_type: str, **fields) -> dict:
    body = {
        "EventType": event_type,
        "CallId": CALL_ID,
        "CreatedAt": CREATED_AT,
    }
    body.update(fields)
    return body


SAMPLES = {
    "START": frame(
        "START",
        SamplingRate=48000,
        DiarizeSystemChannel=True,
        DiarizeMicChannel=False,
    ),
    "SPEAKER_CHANGE": frame(
        "SPEAKER_CHANGE",
        Channel="CALLER",
        ActiveSpeaker="spk_0",
    ),
    "PAUSE": frame("PAUSE"),
    "RESUME": frame("RESUME"),
    "END": frame("END"),
    "VP_COMMAND": frame(
        "VP_COMMAND",
        TaskId="task-1",
        Command="CLICK",
        Payload={"x": 512, "y": 384},
    ),
    "AGENT_QUERY": frame(
        "AGENT_QUERY",
        QueryId=QUERY_ID,
        Message="What did the caller agree to?",
        History=[
            {"Role": "user", "Content": "hello"},
            {"Role": "assistant", "Content": "Hi there"},
        ],
    ),
    "ADD_TRANSCRIPT_SEGMENT": frame(
        "ADD_TRANSCRIPT_SEGMENT",
        SegmentId="seg_0001",
        Channel="CALLER",
        Speaker="spk_0",
        StartTime=0.0,
        EndTime=2.5,
        Transcript="Hello there.",
        IsPartial=False,
        SentimentScore=0.42,
    ),
    "ADD_SUMMARY": frame(
        "ADD_SUMMARY",
        Section="action_items",
        SummaryText="Caller agreed to the renewal.",
    ),
    "ADD_AGENT_ASSIST": frame(
        "ADD_AGENT_ASSIST",
        SegmentId="asst_0001",
        TriggerSegmentId="seg_0001",
        Transcript="Offer the renewal discount.",
        IsPartial=True,
    ),
    "AGENT_TOKEN": frame(
        "AGENT_TOKEN",
        QueryId=QUERY_ID,
        Seq=0,
        Delta="Hel",
    ),
    "THINKING_STEP": frame(
        "THINKING_STEP",
        QueryId=QUERY_ID,
        Seq=1,
        StepType="tool_use",
        Content="Searching transcript",
        ToolName="query_transcripts",
        ToolInput={"q": "renewal"},
        ToolResult=None,
        Success=None,
    ),
    "VP_STATUS": frame(
        "VP_STATUS",
        TaskId="task-1",
        State="IN_MEETING",
        Detail="joined",
    ),
    "VP_SCREENSHOT": frame(
        "VP_SCREENSHOT",
        TaskId="task-1",
        ImageBase64=base64.b64encode(b"\x89PNG\r\n\x1a\n").decode("ascii"),
    ),
    "ERROR": frame(
        "ERROR",
        Code="STT_STREAM_RESET",
        Context={"attempt": 3},
    ),
    "DELETE_TRANSCRIPT_SEGMENT": frame(
        "DELETE_TRANSCRIPT_SEGMENT",
        SegmentId="seg_0001",
        Reason="STALE_PARTIAL",
    ),
}


def schema_event_types() -> set[str]:
    schema = load_schema()
    names = (branch["$ref"].split("/")[-1] for branch in schema["oneOf"])
    return {schema["$defs"][name]["properties"]["EventType"]["const"] for name in names}


def build_validator() -> Draft202012Validator:
    schema = load_schema()
    schema["$defs"]["Error"]["properties"]["Code"] = {
        "enum": sorted(load_error_codes()),
    }
    return Draft202012Validator(schema)


@pytest.mark.parametrize("event_type", sorted(SAMPLES))
def test_sample_frame_validates(event_type):
    validator = build_validator()
    errors = list(validator.iter_errors(SAMPLES[event_type]))
    assert errors == []


def test_samples_cover_every_event_type_in_oneof():
    assert {s["EventType"] for s in SAMPLES.values()} == schema_event_types()


def test_unknown_event_type_is_rejected():
    validator = build_validator()
    errors = list(validator.iter_errors(frame("TIME_TRAVEL")))
    assert errors != []
