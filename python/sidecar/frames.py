import json
import sys
from dataclasses import dataclass, field
from functools import lru_cache
from pathlib import Path

import jsonschema
import yaml

CONTRACTS_DIR = (
    Path(sys._MEIPASS) / "contracts"
    if getattr(sys, "frozen", False)
    else Path(__file__).resolve().parents[2] / "contracts"
)
INVALID_FRAME_CODE = "LINK_DISCONNECTED"
INVALID_FRAME_CONTEXT = {"reason": "invalid-frame"}
INVALID_FRAME_CLOSE_CODE = 1008
INBOUND_DEFS = {
    "START": "Start",
    "SPEAKER_CHANGE": "SpeakerChange",
    "PAUSE": "Pause",
    "RESUME": "Resume",
    "END": "End",
    "VP_COMMAND": "VpCommand",
    "AGENT_QUERY": "AgentQuery",
}


@dataclass(frozen=True)
class Start:
    call_id: str
    sampling_rate: int
    diarize_system_channel: bool = False
    diarize_mic_channel: bool = False


@dataclass(frozen=True)
class SpeakerChange:
    call_id: str
    channel: str
    active_speaker: str


@dataclass(frozen=True)
class Pause:
    call_id: str


@dataclass(frozen=True)
class Resume:
    call_id: str


@dataclass(frozen=True)
class End:
    call_id: str


@dataclass(frozen=True)
class AgentQuery:
    call_id: str
    query_id: str
    message: str
    history: list[dict] = field(default_factory=list)


@dataclass(frozen=True)
class VpCommand:
    task_id: str
    command: str
    payload: dict = field(default_factory=dict)


class FrameError(ValueError):
    pass


def load_schema() -> dict:
    return json.loads((CONTRACTS_DIR / "events.schema.json").read_text())


@lru_cache(maxsize=1)
def _error_codes() -> tuple[str, ...]:
    catalog = yaml.safe_load((CONTRACTS_DIR / "errors.yaml").read_text())
    return tuple(entry["code"] for entry in catalog["errors"])


@lru_cache(maxsize=1)
def inbound_validator() -> jsonschema.Draft202012Validator:
    schema = load_schema()
    schema["oneOf"] = [{"$ref": f"#/$defs/{name}"} for name in INBOUND_DEFS.values()]
    return jsonschema.Draft202012Validator(schema)


@lru_cache(maxsize=1)
def outbound_validator() -> jsonschema.Draft202012Validator:
    schema = load_schema()
    schema["$defs"]["Error"]["properties"]["Code"] = {"enum": list(_error_codes())}
    return jsonschema.Draft202012Validator(schema)


def parse_frame(raw: str) -> Start | SpeakerChange | Pause | Resume | End | VpCommand | AgentQuery:
    try:
        payload = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise FrameError("non-json-frame") from exc
    if not isinstance(payload, dict):
        raise FrameError("non-object-frame")
    event_type = payload.get("EventType")
    if event_type not in INBOUND_DEFS:
        raise FrameError(f"unknown-event-type:{event_type}")
    if inbound_validator().is_valid(payload) is False:
        raise FrameError(f"schema-invalid:{event_type}")
    match event_type:
        case "START":
            return Start(
                payload["CallId"],
                payload["SamplingRate"],
                payload.get("DiarizeSystemChannel", False),
                payload.get("DiarizeMicChannel", False),
            )
        case "SPEAKER_CHANGE":
            return SpeakerChange(payload["CallId"], payload["Channel"], payload["ActiveSpeaker"])
        case "PAUSE":
            return Pause(payload["CallId"])
        case "RESUME":
            return Resume(payload["CallId"])
        case "END":
            return End(payload["CallId"])
        case "VP_COMMAND":
            return VpCommand(payload["TaskId"], payload["Command"], payload.get("Payload", {}))
        case "AGENT_QUERY":
            return AgentQuery(
                payload["CallId"],
                payload["QueryId"],
                payload["Message"],
                payload.get("History", []),
            )
    raise FrameError(f"unmapped-event-type:{event_type}")


def error_frame(call_id: str, code: str, context: dict | None = None) -> dict:
    frame = {"EventType": "ERROR", "CallId": call_id, "Code": code}
    if context is not None:
        frame["Context"] = context
    return frame


def delete_segment_frame(call_id: str, segment_id: str, reason: str = "STALE_PARTIAL") -> dict:
    return {
        "EventType": "DELETE_TRANSCRIPT_SEGMENT",
        "CallId": call_id,
        "SegmentId": segment_id,
        "Reason": reason,
    }


def serialize_event(event: dict) -> str:
    outbound_validator().validate(event)
    return json.dumps(event, separators=(",", ":"))
