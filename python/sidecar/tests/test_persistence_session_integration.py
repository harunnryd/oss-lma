import asyncio
import json
from pathlib import Path

from lma_stt.types import ProviderResetError, WordItem
from websockets.exceptions import ConnectionClosed
from websockets.frames import Close

from sidecar.session import Session
from sidecar.storage.connection import open_db
from sidecar.storage.migrations import apply_migrations
from sidecar.storage.persistence import SqliteWriter
from sidecar.storage.recording import WavRecordingSink

from tests.helpers import MemoryConnection, ScriptedEngine, ScriptedStream, eventually

CALL_ID = "11111111-1111-1111-1111-111111111111"


def _bootstrap(tmp_path: Path):
    conn = open_db(tmp_path / "lma.db")
    apply_migrations(conn, Path(__file__).resolve().parents[1] / "storage" / "migrations")
    return conn


async def make_session_with_db(tmp_path, results):
    db_conn = _bootstrap(tmp_path)
    connection = MemoryConnection()
    engine = ScriptedEngine(results)
    session = Session(connection, lambda ctx: engine, db=SqliteWriter(db_conn))
    await session.on_text(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
    return connection, db_conn, engine, session


async def test_session_writes_segments_to_sqlite(tmp_path):
    result = {
        "result_id": "r1",
        "is_partial": False,
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
    connection, db_conn, _, session = await make_session_with_db(tmp_path, [result])
    await session.on_binary(bytes(19200))
    await eventually(lambda: len(connection.sent) == 1)
    row = db_conn.execute(
        "SELECT text, channel, is_partial FROM segments WHERE meeting_id = ?",
        (CALL_ID,),
    ).fetchone()
    assert row["text"] == "hello"
    assert row["channel"] == "CALLER"
    assert row["is_partial"] == 0


async def test_session_meeting_row_exists_after_start(tmp_path):
    _connection, db_conn, _, _ = await make_session_with_db(tmp_path, [])
    row = db_conn.execute(
        "SELECT id, source, status FROM meetings WHERE id = ?", (CALL_ID,)
    ).fetchone()
    assert row["id"] == CALL_ID
    assert row["source"] == "LOCAL"
    assert row["status"] == "RECORDING"


async def test_session_meeting_status_updates_on_end(tmp_path):
    _connection, db_conn, _, session = await make_session_with_db(tmp_path, [])
    await session.on_text(json.dumps({"EventType": "END", "CallId": CALL_ID}))
    await eventually(
        lambda: db_conn.execute(
            "SELECT status FROM meetings WHERE id = ?", (CALL_ID,)
        ).fetchone()["status"]
        == "COMPLETED"
    )


async def test_session_records_audio_when_recorder_provided(tmp_path):
    connection = MemoryConnection()
    engine = ScriptedEngine([])
    recorder_path = tmp_path / "rec.wav"
    recorder = WavRecordingSink(recorder_path)
    session = Session(connection, lambda ctx: engine, recorder=recorder)
    await session.on_text(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
    await session.on_binary(bytes(19200))
    await session.on_text(json.dumps({"EventType": "END", "CallId": CALL_ID}))
    import wave
    with wave.open(str(recorder_path), "rb") as reader:
        assert reader.getnframes() == 4800


async def test_session_creates_recorder_when_record_meeting_true(tmp_path, monkeypatch):
    monkeypatch.setenv("LMA_RECORDING_DIR", str(tmp_path / "recs"))
    connection = MemoryConnection()
    engine = ScriptedEngine([])
    session = Session(
        connection,
        lambda ctx: engine,
        record_meeting=True,
    )
    await session.on_text(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
    assert session.recorder is not None
    await session.on_binary(bytes(19200))
    await session.on_text(json.dumps({"EventType": "END", "CallId": CALL_ID}))
    import wave
    wav_path = tmp_path / "recs" / CALL_ID / "audio.wav"
    assert wav_path.exists()
    with wave.open(str(wav_path), "rb") as reader:
        assert reader.getframerate() == 48000


async def test_session_default_db_is_noop():
    connection = MemoryConnection()
    engine = ScriptedEngine([])
    session = Session(connection, lambda ctx: engine)
    await session.on_text(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
    assert session.db is None


async def test_db_write_error_sends_db_write_conflict_frame(tmp_path):
    db_conn = _bootstrap(tmp_path)
    db_conn.execute(
        "INSERT INTO meetings (id, source, started_at) VALUES (?, ?, ?)",
        (CALL_ID, "LOCAL", 1700000000000),
    )
    db_conn.commit()

    result = {
        "result_id": "r1",
        "is_partial": False,
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
    connection = MemoryConnection()
    engine = ScriptedEngine([result])
    session = Session(
        connection,
        lambda ctx: engine,
        db=SqliteWriter(db_conn),
    )
    await session.on_text(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
    db_conn.execute("DELETE FROM meetings WHERE id = ?", (CALL_ID,))
    db_conn.commit()
    await session.on_binary(bytes(19200))
    await eventually(lambda: any(
        json.loads(m).get("Code") == "DB_WRITE_CONFLICT" for m in connection.sent
    ))
    frame = next(json.loads(m) for m in connection.sent if json.loads(m).get("Code") == "DB_WRITE_CONFLICT")
    assert frame["EventType"] == "ERROR"
    assert frame["CallId"] == CALL_ID


def test_session_default_time_offset_is_zero():
    from sidecar.session import Session
    conn = MemoryConnection()
    session = Session(conn, lambda ctx: ScriptedEngine([]))
    assert session.time_offset_ms == 0


def test_apply_offset_identity_when_zero():
    from sidecar.session import Session
    conn = MemoryConnection()
    session = Session(conn, lambda ctx: ScriptedEngine([]))
    event = {
        "EventType": "ADD_TRANSCRIPT_SEGMENT",
        "SegmentId": "r1",
        "Channel": "CALLER",
        "StartTime": 5.0,
        "EndTime": 6.5,
        "Transcript": "hi",
        "IsPartial": False,
    }
    assert session._apply_offset(event, 0) is event


def test_apply_offset_adds_to_segment_timestamps():
    from sidecar.session import Session
    conn = MemoryConnection()
    session = Session(conn, lambda ctx: ScriptedEngine([]))
    event = {
        "EventType": "ADD_TRANSCRIPT_SEGMENT",
        "SegmentId": "r1",
        "Channel": "CALLER",
        "StartTime": 5.0,
        "EndTime": 6.5,
        "Transcript": "hi",
        "IsPartial": False,
    }
    adjusted = session._apply_offset(event, 12_500)
    assert adjusted["StartTime"] == 17.5
    assert adjusted["EndTime"] == 19.0


async def test_pump_persists_offset_adjusted_segments(tmp_path):
    db_conn = _bootstrap(tmp_path)
    result = {
        "result_id": "r1",
        "is_partial": False,
        "items": [
            WordItem(
                content="hello",
                type="pronunciation",
                start_time=5.0,
                end_time=6.5,
                speaker="spk_0",
                channel="CALLER",
                result_id="r1",
            )
        ],
    }
    connection = MemoryConnection()
    engine = ScriptedEngine([result])
    session = Session(connection, lambda ctx: engine, db=SqliteWriter(db_conn))
    await session.on_text(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
    session.time_offset_ms = 12_500
    await session.on_binary(bytes(19200))
    await eventually(lambda: len(connection.sent) == 1)
    row = db_conn.execute(
        "SELECT start_ms, end_ms FROM segments WHERE meeting_id = ?", (CALL_ID,)
    ).fetchone()
    assert row["start_ms"] == 17_500
    assert row["end_ms"] == 19_000
    wire = json.loads(connection.sent[0])
    assert wire["StartTime"] == 17.5
    assert wire["EndTime"] == 19.0


async def test_start_session_loads_existing_offset_from_db(tmp_path):
    db_conn = _bootstrap(tmp_path)
    db_conn.execute(
        "INSERT INTO meetings (id, source, started_at, time_offset_ms) VALUES (?, ?, ?, ?)",
        ("m-1", "LOCAL", 1700000000000, 12345),
    )
    db_conn.commit()
    connection = MemoryConnection()
    session = Session(connection, lambda ctx: ScriptedEngine([]), db=SqliteWriter(db_conn))
    await session.on_text(json.dumps({"EventType": "START", "CallId": "m-1", "SamplingRate": 48000}))
    assert session.time_offset_ms == 12345


class FlakyStream:
    def __init__(self, real_stream: ScriptedStream):
        self._real = real_stream
        self._raised = False

    def __aiter__(self):
        return self

    async def __anext__(self):
        if not self._raised:
            self._raised = True
            raise ProviderResetError("flaky first stream")
        return await self._real.__anext__()

    async def feed(self, pcm: bytes) -> None:
        await self._real.feed(pcm)

    async def close(self) -> None:
        await self._real.close()


class FlakyScriptedEngine(ScriptedEngine):
    def __init__(self, results, *, raise_reset_first=False):
        super().__init__(results)
        self.raise_reset_first = raise_reset_first
        self.engines_started = 0

    async def start(self, ctx):
        self.engines_started += 1
        real_stream = await super().start(ctx)
        if self.raise_reset_first and self.engines_started == 1:
            wrapped = FlakyStream(real_stream)
            self.stream = wrapped
            return wrapped
        return real_stream


async def test_pump_reconnects_after_provider_reset(tmp_path):
    db_conn = _bootstrap(tmp_path)
    result = {
        "result_id": "r1",
        "is_partial": False,
        "items": [
            WordItem(
                content="hello",
                type="pronunciation",
                start_time=0.0,
                end_time=1.0,
                speaker="spk_0",
                channel="CALLER",
                result_id="r1",
            )
        ],
    }
    connection = MemoryConnection()
    engine = FlakyScriptedEngine([result], raise_reset_first=True)
    session = Session(
        connection,
        lambda ctx: engine,
        db=SqliteWriter(db_conn),
        sleep=lambda _s: asyncio.sleep(0),
    )
    await session.on_text(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
    await eventually(
        lambda: any(json.loads(m).get("Code") == "STT_STREAM_RESET" for m in connection.sent)
    )
    await session.on_binary(bytes(19200))
    await eventually(
        lambda: any(json.loads(m).get("EventType") == "ADD_TRANSCRIPT_SEGMENT" for m in connection.sent)
    )
    sent = [json.loads(m) for m in connection.sent]
    assert any(s.get("Code") == "STT_STREAM_RESET" for s in sent if s.get("EventType") == "ERROR")
    assert any(s.get("EventType") == "ADD_TRANSCRIPT_SEGMENT" for s in sent)
    assert engine.engines_started == 2


class AlwaysResetStream:
    def __init__(self, exc: Exception):
        self.exc = exc

    def __aiter__(self):
        return self

    async def __anext__(self):
        raise self.exc

    async def feed(self, pcm: bytes) -> None:
        return None

    async def close(self) -> None:
        return None


class RestartFlakyEngine(ScriptedEngine):
    def __init__(self, results, *, fail_restart_attempts=()):
        super().__init__(results)
        self.fail_restart_attempts = set(fail_restart_attempts)
        self.engines_started = 0

    async def start(self, ctx):
        self.engines_started += 1
        if self.engines_started == 1:
            return AlwaysResetStream(ProviderResetError("dead stream"))
        if self.engines_started in self.fail_restart_attempts:
            raise ProviderResetError("restart attempt failed")
        return await super().start(ctx)


async def test_pump_retries_when_restart_attempt_itself_fails(tmp_path):
    db_conn = _bootstrap(tmp_path)
    result = {
        "result_id": "r1",
        "is_partial": False,
        "items": [
            WordItem(
                content="hello",
                type="pronunciation",
                start_time=0.0,
                end_time=1.0,
                speaker="spk_0",
                channel="CALLER",
                result_id="r1",
            )
        ],
    }
    connection = MemoryConnection()
    engine = RestartFlakyEngine([result], fail_restart_attempts={2})
    session = Session(
        connection,
        lambda ctx: engine,
        db=SqliteWriter(db_conn),
        sleep=lambda _s: asyncio.sleep(0),
    )
    await session.on_text(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
    await eventually(
        lambda: sum(
            1 for m in connection.sent if json.loads(m).get("Code") == "STT_STREAM_RESET"
        )
        >= 2
    )
    await session.on_binary(bytes(19200))
    await eventually(
        lambda: any(json.loads(m).get("EventType") == "ADD_TRANSCRIPT_SEGMENT" for m in connection.sent)
    )
    sent = [json.loads(m) for m in connection.sent]
    reset_frames = [s for s in sent if s.get("Code") == "STT_STREAM_RESET"]
    assert [f["Context"]["attempt"] for f in reset_frames] == [1, 2]
    assert any(s.get("EventType") == "ADD_TRANSCRIPT_SEGMENT" for s in sent)
    assert engine.engines_started == 3
    assert session.reconnect_state.consecutive_failures == 0


class AlwaysFailEngine(ScriptedEngine):
    def __init__(self, results):
        super().__init__(results)
        self.engines_started = 0

    async def start(self, ctx):
        self.engines_started += 1
        if self.engines_started == 1:
            return AlwaysResetStream(ProviderResetError("dead stream"))
        raise ProviderResetError("restart attempt failed")


async def test_pump_closes_connection_after_budget_exhausted(tmp_path):
    db_conn = _bootstrap(tmp_path)
    connection = MemoryConnection()
    engine = AlwaysFailEngine([])
    session = Session(
        connection,
        lambda ctx: engine,
        db=SqliteWriter(db_conn),
        sleep=lambda _s: asyncio.sleep(0),
    )
    await session.on_text(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
    await eventually(lambda: bool(connection.closes))
    assert connection.closes == [(1013, "stt-reconnect-exhausted")]
    reset_frames = [
        json.loads(m) for m in connection.sent if json.loads(m).get("Code") == "STT_STREAM_RESET"
    ]
    assert [f["Context"].get("attempt") for f in reset_frames[:-1]] == [1, 2, 3, 4, 5]
    assert reset_frames[-1]["Context"] == {"attempts": 6}
    assert reset_frames[-1]["EventType"] == "ERROR"
    assert reset_frames[-1]["CallId"] == CALL_ID
    row = db_conn.execute(
        "SELECT status, ended_at, duration_ms FROM meetings WHERE id = ?", (CALL_ID,)
    ).fetchone()
    assert row["status"] == "FAILED"
    assert row["ended_at"] is not None
    assert row["duration_ms"] is not None


def _provider_closed() -> ConnectionClosed:
    return ConnectionClosed(Close(1006, "provider gone"), None)


class DeadFeedStream:
    def __init__(self, results, feed_exc: Exception):
        self._real = ScriptedStream(list(results))
        self._feed_exc = feed_exc
        self._raised = False
        self.feed_calls = 0
        self.closed = False

    def __aiter__(self):
        return self

    async def __anext__(self):
        if not self._raised:
            self._raised = True
            raise ProviderResetError("provider dropped mid-stream")
        return await self._real.__anext__()

    async def feed(self, pcm: bytes) -> None:
        self.feed_calls += 1
        raise self._feed_exc

    async def close(self) -> None:
        self.closed = True


class DeadFeedEngine(ScriptedEngine):
    def __init__(self, results, feed_exc: Exception):
        super().__init__(results)
        self.feed_exc = feed_exc
        self.engines_started = 0
        self.first_stream: DeadFeedStream | None = None

    async def start(self, ctx):
        self.engines_started += 1
        if self.engines_started == 1:
            self.first_stream = DeadFeedStream(self.results, self.feed_exc)
            self.stream = self.first_stream
            return self.first_stream
        return await super().start(ctx)


async def test_on_binary_tolerates_dead_provider_stream(tmp_path):
    connection = MemoryConnection()
    session = Session(connection, lambda ctx: ScriptedEngine([]))
    await session.on_text(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
    dead = DeadFeedStream([], _provider_closed())
    session.stream = dead
    await session.on_binary(bytes(19200))
    assert dead.feed_calls == 1
    assert connection.closes == []
    assert connection.sent == []


async def test_session_survives_audio_during_reconnect_backoff(tmp_path):
    db_conn = _bootstrap(tmp_path)
    result = {
        "result_id": "r1",
        "is_partial": False,
        "items": [
            WordItem(
                content="hello",
                type="pronunciation",
                start_time=0.0,
                end_time=1.0,
                speaker="spk_0",
                channel="CALLER",
                result_id="r1",
            )
        ],
    }
    connection = MemoryConnection()
    engine = DeadFeedEngine([result], _provider_closed())
    entered_backoff = asyncio.Event()
    release_backoff = asyncio.Event()

    async def gated_sleep(_seconds):
        entered_backoff.set()
        await release_backoff.wait()

    session = Session(
        connection,
        lambda ctx: engine,
        db=SqliteWriter(db_conn),
        sleep=gated_sleep,
    )
    await session.on_text(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
    await asyncio.wait_for(entered_backoff.wait(), 2.0)
    await session.on_binary(bytes(19200))
    await session.on_binary(bytes(19200))
    assert engine.first_stream is not None
    assert engine.first_stream.feed_calls == 0
    assert connection.closes == []
    release_backoff.set()
    await eventually(lambda: engine.engines_started == 2)
    assert engine.first_stream.closed is True
    await session.on_binary(bytes(19200))
    await eventually(
        lambda: any(
            json.loads(m).get("EventType") == "ADD_TRANSCRIPT_SEGMENT" for m in connection.sent
        )
    )
    assert connection.closes == []
    assert session.pump_task is not None and not session.pump_task.done()


def _one_word_result(result_id: str, start: float, end: float) -> dict:
    return {
        "result_id": result_id,
        "is_partial": False,
        "items": [
            WordItem(
                content="hello",
                type="pronunciation",
                start_time=start,
                end_time=end,
                speaker="spk_0",
                channel="CALLER",
                result_id=result_id,
            )
        ],
    }


async def test_start_resumes_timeline_after_crash_without_clean_end(tmp_path):
    db_conn = _bootstrap(tmp_path)
    start_frame = json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000})

    first_connection = MemoryConnection()
    first_engine = ScriptedEngine([_one_word_result("r1", 0.0, 3.0)])
    first_session = Session(first_connection, lambda ctx: first_engine, db=SqliteWriter(db_conn))
    await first_session.on_text(start_frame)
    await first_session.on_binary(bytes(19200))
    await eventually(lambda: len(first_connection.sent) == 1)
    assert first_session.time_offset_ms == 0
    first_session.pump_task.cancel()
    await asyncio.gather(first_session.pump_task, return_exceptions=True)

    crashed = db_conn.execute(
        "SELECT status, time_offset_ms, ended_at FROM meetings WHERE id = ?", (CALL_ID,)
    ).fetchone()
    assert crashed["status"] == "RECORDING"
    assert crashed["time_offset_ms"] == 0
    assert crashed["ended_at"] is None

    second_connection = MemoryConnection()
    second_engine = ScriptedEngine([_one_word_result("r2", 0.0, 2.0)])
    second_session = Session(second_connection, lambda ctx: second_engine, db=SqliteWriter(db_conn))
    await second_session.on_text(start_frame)
    assert second_session.time_offset_ms == 3000
    await second_session.on_binary(bytes(19200))
    await eventually(lambda: len(second_connection.sent) == 1)

    rows = db_conn.execute(
        "SELECT segment_id, start_ms, end_ms FROM segments WHERE meeting_id = ? "
        "ORDER BY start_ms",
        (CALL_ID,),
    ).fetchall()
    assert [(r["start_ms"], r["end_ms"]) for r in rows] == [(0, 3000), (3000, 5000)]
    wire = json.loads(second_connection.sent[0])
    assert wire["StartTime"] == 3.0
    assert wire["EndTime"] == 5.0
    await second_session.on_text(json.dumps({"EventType": "END", "CallId": CALL_ID}))
