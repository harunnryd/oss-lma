import json
from pathlib import Path

from lma_stt.types import WordItem

from sidecar.session import Session
from sidecar.storage.connection import open_db
from sidecar.storage.migrations import apply_migrations
from sidecar.storage.persistence import SqliteWriter
from sidecar.storage.recording import WavRecordingSink

from tests.helpers import MemoryConnection, ScriptedEngine, eventually

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