import asyncio
import json

from lma_pipeline import SegmentAssembler
from lma_stt.types import ProviderResetError, WordItem

from sidecar.frames import INVALID_FRAME_CLOSE_CODE, INVALID_FRAME_CODE, INVALID_FRAME_CONTEXT
from sidecar.session import Session

from tests.helpers import MemoryConnection, ScriptedEngine, eventually

CALL_ID = "11111111-1111-1111-1111-111111111111"


def word(content: str, start: float, end: float, channel: str, result_id: str) -> WordItem:
    return WordItem(
        content=content,
        type="pronunciation",
        start_time=start,
        end_time=end,
        speaker="spk_0",
        channel=channel,
        result_id=result_id,
    )


PARTIAL = {
    "result_id": "r1",
    "is_partial": True,
    "items": [word("hello", 0.0, 0.8, "CALLER", "r1")],
}


def start_message(sampling_rate: int = 48000) -> str:
    return json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": sampling_rate})


async def make_session(results: list[dict], sampling_rate: int = 48000):
    connection = MemoryConnection()
    engine = ScriptedEngine(results)
    session = Session(connection, lambda ctx: engine)
    await session.on_text(start_message(sampling_rate))
    return connection, engine, session


async def test_start_builds_context_and_engine():
    _connection, engine, _ = await make_session([PARTIAL])
    assert engine.started_with == [{
        "call_id": CALL_ID,
        "sample_rate": 48000,
        "diarize": {"system": False, "mic": False},
        "language_hints": [],
    }]


async def test_start_maps_diarization_flags():
    connection = MemoryConnection()
    engine = ScriptedEngine([])
    session = Session(connection, lambda ctx: engine)
    await session.on_text(json.dumps({
        "EventType": "START",
        "CallId": CALL_ID,
        "SamplingRate": 16000,
        "DiarizeSystemChannel": True,
        "DiarizeMicChannel": True,
    }))
    assert engine.started_with[0]["diarize"] == {"system": True, "mic": True}


async def test_audio_flows_to_engine_and_results_emit_as_segments():
    connection, engine, session = await make_session([PARTIAL])
    await session.on_binary(bytes(19200))
    await eventually(lambda: len(connection.sent) == 1)
    expected = SegmentAssembler(CALL_ID).on_result(PARTIAL)
    assert [json.loads(message) for message in connection.sent] == expected
    assert engine.stream.fed == [bytes(19200)]


async def test_chunk_size_tracks_sampling_rate():
    _connection, engine, session = await make_session([], sampling_rate=16000)
    await session.on_binary(bytes(6400))
    assert engine.stream.fed == [bytes(6400)]


async def test_wrong_size_audio_triggers_invalid_frame_policy():
    connection, _, session = await make_session([])
    await session.on_binary(bytes(100))
    await eventually(lambda: bool(connection.closes))
    assert [json.loads(message) for message in connection.sent] == [{
        "EventType": "ERROR",
        "CallId": CALL_ID,
        "Code": INVALID_FRAME_CODE,
        "Context": INVALID_FRAME_CONTEXT,
    }]
    assert connection.closes == [(INVALID_FRAME_CLOSE_CODE, "invalid-frame")]
    assert connection.open is False


async def test_binary_before_start_closes_1008_without_error_frame():
    connection = MemoryConnection()
    session = Session(connection, lambda ctx: ScriptedEngine([]))
    await session.on_binary(bytes(19200))
    await eventually(lambda: bool(connection.closes))
    assert connection.sent == []
    assert connection.closes == [(1008, "audio-before-start")]


async def test_unknown_event_type_triggers_invalid_frame_policy():
    connection, _, session = await make_session([])
    await session.on_text('{"EventType": "NOPE"}')
    await eventually(lambda: bool(connection.closes))
    assert connection.closes == [(1008, "invalid-frame")]


async def test_invalid_text_before_start_reports_empty_call_id():
    connection = MemoryConnection()
    session = Session(connection, lambda ctx: ScriptedEngine([]))
    await session.on_text("{oops")
    await eventually(lambda: len(connection.sent) == 1)
    assert json.loads(connection.sent[0])["CallId"] == ""


async def test_schema_violation_triggers_invalid_frame_policy():
    connection, _, session = await make_session([])
    await session.on_text(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 4000}))
    await eventually(lambda: bool(connection.closes))
    assert connection.closes == [(1008, "invalid-frame")]


class HangingStream:
    def __init__(self):
        self.closed = False

    def __aiter__(self):
        return self

    async def __anext__(self):
        await asyncio.sleep(3600)
        raise StopAsyncIteration

    async def close(self):
        self.closed = True


class HangingEngine:
    def __init__(self):
        self.started_with = []
        self.stream = None

    async def start(self, ctx):
        self.started_with.append(dict(ctx))
        self.stream = HangingStream()
        return self.stream


class RaiseOnCloseStream:
    def __init__(self, exc: Exception):
        self.exc = exc

    def __aiter__(self):
        return self

    async def __anext__(self):
        raise StopAsyncIteration

    async def close(self):
        raise self.exc


class RaiseOnCloseEngine:
    def __init__(self, exc: Exception):
        self.exc = exc
        self.started_with = []

    async def start(self, ctx):
        self.started_with.append(dict(ctx))
        return RaiseOnCloseStream(self.exc)


async def test_drain_timeout_does_not_propagate_out_of_close_session(monkeypatch):
    import sidecar.session as session_module

    monkeypatch.setattr(session_module, "DRAIN_TIMEOUT_SECONDS", 0.01)
    connection = MemoryConnection()
    session = Session(connection, lambda ctx: HangingEngine())
    await session.on_text(start_message())
    await session.on_text(json.dumps({"EventType": "END", "CallId": CALL_ID}))
    assert connection.open is True


async def test_drain_close_provider_error_does_not_propagate_out_of_close_session():
    connection = MemoryConnection()
    session = Session(connection, lambda ctx: RaiseOnCloseEngine(ProviderResetError("boom")))
    await session.on_text(start_message())
    await eventually(lambda: session.pump_task is not None and session.pump_task.done())
    await session.on_text(json.dumps({"EventType": "END", "CallId": CALL_ID}))
    assert connection.open is True


class SizeMismatchStream:
    def __init__(self):
        self.fed = []

    def __aiter__(self):
        return self

    async def __anext__(self):
        raise StopAsyncIteration

    async def feed(self, pcm):
        raise ValueError(f"provider expected a different chunk size, got {len(pcm)}")

    async def close(self):
        pass


class SizeMismatchEngine:
    def __init__(self):
        self.started_with = []

    async def start(self, ctx):
        self.started_with.append(dict(ctx))
        return SizeMismatchStream()


async def test_provider_side_chunk_size_mismatch_is_handled_gracefully():
    connection = MemoryConnection()
    session = Session(connection, lambda ctx: SizeMismatchEngine())
    await session.on_text(start_message())
    await session.on_binary(bytes(19200))
    await eventually(lambda: bool(connection.closes))
    assert [json.loads(message) for message in connection.sent] == [{
        "EventType": "ERROR",
        "CallId": CALL_ID,
        "Code": INVALID_FRAME_CODE,
        "Context": INVALID_FRAME_CONTEXT,
    }]
    assert connection.closes == [(INVALID_FRAME_CLOSE_CODE, "invalid-frame")]


class ResetOnceStream:
    def __init__(self, exc: Exception | None):
        self.exc = exc

    def __aiter__(self):
        return self

    async def __anext__(self):
        if self.exc is not None:
            raise self.exc
        await asyncio.sleep(3600)

    async def close(self):
        pass


class ResetOnceEngine:
    def __init__(self, exc: Exception):
        self.exc = exc
        self.started_with = []
        self.started_count = 0

    async def start(self, ctx):
        self.started_with.append(dict(ctx))
        self.started_count += 1
        return ResetOnceStream(self.exc if self.started_count == 1 else None)


async def test_provider_reset_during_pump_sends_error_frame_and_reconnects():
    connection = MemoryConnection()
    engine = ResetOnceEngine(ProviderResetError("upstream dropped"))
    session = Session(connection, lambda ctx: engine, sleep=lambda _s: asyncio.sleep(0))
    await session.on_text(start_message())
    await eventually(lambda: bool(connection.sent))
    assert [json.loads(message) for message in connection.sent] == [{
        "EventType": "ERROR",
        "CallId": CALL_ID,
        "Code": "STT_STREAM_RESET",
        "Context": {"attempt": 1},
    }]
    assert not session.pump_task.done()
