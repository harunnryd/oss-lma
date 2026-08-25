import json

from sidecar.frames import serialize_event
from sidecar.session import Session

from tests.helpers import MemoryConnection, ScriptedEngine

CALL_ID = "11111111-1111-1111-1111-111111111111"
QUERY_ID = "33333333-3333-3333-3333-333333333333"

EXPECTED_STUB = {
    "EventType": "THINKING_STEP",
    "CallId": CALL_ID,
    "QueryId": QUERY_ID,
    "Seq": 0,
    "StepType": "status",
    "Content": "agent unavailable in P1",
}


async def test_agent_query_answers_with_single_status_thinking_step():
    connection = MemoryConnection()
    session = Session(connection, lambda ctx: ScriptedEngine([]))
    await session.on_text(json.dumps({
        "EventType": "AGENT_QUERY",
        "CallId": CALL_ID,
        "QueryId": QUERY_ID,
        "Message": "What did we just discuss?",
        "History": [{"Role": "user", "Content": "hi"}],
    }))
    assert [json.loads(message) for message in connection.sent] == [EXPECTED_STUB]
    assert connection.closes == []


async def test_agent_query_works_without_active_session():
    connection = MemoryConnection()
    session = Session(connection, lambda ctx: ScriptedEngine([]))
    await session.on_text(json.dumps({
        "EventType": "AGENT_QUERY",
        "CallId": CALL_ID,
        "QueryId": QUERY_ID,
        "Message": "q",
    }))
    assert [json.loads(message) for message in connection.sent] == [EXPECTED_STUB]


async def test_vp_command_is_parsed_but_ignored_in_p1():
    connection = MemoryConnection()
    session = Session(connection, lambda ctx: ScriptedEngine([]))
    await session.on_text(json.dumps({
        "EventType": "VP_COMMAND",
        "TaskId": "t-1",
        "Command": "CLICK",
        "Payload": {"x": 412, "y": 380},
    }))
    assert connection.sent == []
    assert connection.closes == []
    assert connection.open is True


async def test_thinking_step_stub_is_schema_valid():
    raw = serialize_event(EXPECTED_STUB)
    assert json.loads(raw) == EXPECTED_STUB
