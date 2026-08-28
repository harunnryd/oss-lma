import json

from lma_stt.types import WordItem

from sidecar.session import Session

from tests.helpers import MemoryConnection, ScriptedEngine, eventually

CALL_ID = "11111111-1111-1111-1111-111111111111"


def make_session(results: list[dict]):
    connection = MemoryConnection()
    engines = []

    def factory(ctx):
        engine = ScriptedEngine(results)
        engines.append(engine)
        return engine

    return connection, engines, Session(connection, factory)


async def open_session(session):
    await session.on_text(
        json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000})
    )


async def test_speaker_change_reaches_active_assembler(monkeypatch):
    _connection, _, session = make_session([])
    await open_session(session)
    calls = []
    monkeypatch.setattr(
        session.assembler,
        "set_active_speaker",
        lambda channel, name: calls.append((channel, name)),
    )
    await session.on_text(
        json.dumps(
            {
                "EventType": "SPEAKER_CHANGE",
                "CallId": CALL_ID,
                "Channel": "AGENT",
                "ActiveSpeaker": "Ayu",
            }
        )
    )
    assert calls == [("AGENT", "Ayu")]


async def test_control_frames_without_session_are_ignored():
    connection, _, session = make_session([])
    for event_type in ["SPEAKER_CHANGE", "PAUSE", "RESUME", "END"]:
        payload = {"EventType": event_type, "CallId": CALL_ID}
        if event_type == "SPEAKER_CHANGE":
            payload.update({"Channel": "CALLER", "ActiveSpeaker": "Bo"})
        await session.on_text(json.dumps(payload))
    assert connection.sent == []
    assert connection.closes == []
    assert connection.open is True


async def test_pause_discards_audio_and_resume_restores_feed():
    _connection, engines, session = make_session([])
    await open_session(session)
    await session.on_text(json.dumps({"EventType": "PAUSE", "CallId": CALL_ID}))
    await session.on_binary(bytes(19200))
    assert engines[0].stream.fed == []
    await session.on_text(json.dumps({"EventType": "RESUME", "CallId": CALL_ID}))
    await session.on_binary(bytes(19200))
    assert engines[0].stream.fed == [bytes(19200)]


async def test_end_drains_final_results_and_keeps_socket_open():
    partial = {
        "result_id": "r1",
        "is_partial": True,
        "items": [
            WordItem(
                content="hello",
                type="pronunciation",
                start_time=0.0,
                end_time=0.8,
                speaker="spk_0",
                channel="CALLER",
                result_id="r1",
            )
        ],
    }
    final = {
        "result_id": "r1",
        "is_partial": False,
        "items": [
            WordItem(
                content="hello there",
                type="pronunciation",
                start_time=0.0,
                end_time=1.6,
                speaker="spk_0",
                channel="CALLER",
                result_id="r1",
            )
        ],
    }
    connection, engines, session = make_session([partial, final])
    await open_session(session)
    await session.on_binary(bytes(19200))
    await eventually(lambda: len(connection.sent) == 1)
    assert json.loads(connection.sent[0])["IsPartial"] is True
    await session.on_text(json.dumps({"EventType": "END", "CallId": CALL_ID}))
    await eventually(lambda: len(connection.sent) == 2)
    last = json.loads(connection.sent[1])
    assert last["IsPartial"] is False
    assert last["SegmentId"] == json.loads(connection.sent[0])["SegmentId"]
    assert engines[0].stream.closed is True
    assert session.stream is None
    assert connection.open is True


async def test_restart_on_same_socket_finalizes_previous_session():
    _connection, engines, session = make_session([])
    await open_session(session)
    first_stream = engines[0].stream
    await session.on_text(
        json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000})
    )
    assert first_stream.closed is True
    assert len(engines) == 2
    assert engines[1].started_with[0]["call_id"] == CALL_ID
    await session.on_binary(bytes(19200))
    assert engines[1].stream.fed == [bytes(19200)]
    assert first_stream.fed == []
