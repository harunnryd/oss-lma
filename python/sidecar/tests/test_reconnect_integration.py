import asyncio
import io
import json
import re
from pathlib import Path

from lma_stt.types import ProviderResetError, WordItem
from websockets.asyncio.client import connect

from sidecar.server import run_server
from sidecar.storage.connection import open_db
from sidecar.storage.migrations import apply_migrations
from sidecar.storage.persistence import SqliteWriter

from tests.helpers import ScriptedStream, eventually, sine_chunk

CALL_ID = "44444444-4444-4444-4444-444444444444"


class FlakyStream:
    def __init__(self, real_stream: ScriptedStream):
        self._real = real_stream
        self._calls = 0

    def __aiter__(self):
        return self

    async def __anext__(self):
        self._calls += 1
        if self._calls == 2:
            raise ProviderResetError("flaky live stream reset")
        return await self._real.__anext__()

    async def feed(self, pcm: bytes) -> None:
        await self._real.feed(pcm)

    async def close(self) -> None:
        await self._real.close()


class FlakyLiveEngine:
    def __init__(self, first_results: list[dict], second_results: list[dict]):
        self._first_results = list(first_results)
        self._second_results = list(second_results)
        self.engines_started = 0
        self.streams: list = []

    async def start(self, ctx: dict):
        self.engines_started += 1
        if self.engines_started == 1:
            stream = FlakyStream(ScriptedStream(list(self._first_results)))
        else:
            stream = ScriptedStream(list(self._second_results))
        self.streams.append(stream)
        return stream


async def test_e2e_reconnect_with_offset_continuity(tmp_path):
    db_path = tmp_path / "lma.db"
    db_conn = open_db(db_path)
    apply_migrations(db_conn, Path(__file__).resolve().parents[1] / "storage" / "migrations")
    writer = SqliteWriter(db_conn)
    result1 = {
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
    result2 = {
        "result_id": "r2",
        "is_partial": False,
        "items": [
            WordItem(
                content="world",
                type="pronunciation",
                start_time=0.0,
                end_time=1.0,
                speaker="spk_1",
                channel="CALLER",
                result_id="r2",
            )
        ],
    }
    engine = FlakyLiveEngine([result1], [result2])
    sink = io.StringIO()
    stop = asyncio.Event()
    task = asyncio.create_task(
        run_server(
            lambda ctx: engine,
            stop=stop,
            ready_sink=sink,
            db_writer=writer,
        )
    )
    try:
        await eventually(lambda: "SIDECAR_READY" in sink.getvalue())
        match = re.search(r"port=(\d+) token=(\w+)", sink.getvalue())
        port = int(match.group(1))
        token = match.group(2)
        async with connect(f"ws://127.0.0.1:{port}/ws?token={token}") as ws:
            await ws.send(
                json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000})
            )
            await ws.send(sine_chunk(48000, 440, 880))

            segments: list[dict] = []
            errors: list[dict] = []
            while not segments or not errors:
                payload = json.loads(await ws.recv())
                if payload.get("EventType") == "ADD_TRANSCRIPT_SEGMENT":
                    segments.append(payload)
                elif payload.get("EventType") == "ERROR":
                    errors.append(payload)

            await eventually(lambda: engine.engines_started == 2)
            await ws.send(sine_chunk(48000, 220, 440))

            while len(segments) < 2:
                payload = json.loads(await ws.recv())
                if payload.get("EventType") == "ADD_TRANSCRIPT_SEGMENT":
                    segments.append(payload)
                elif payload.get("EventType") == "ERROR":
                    errors.append(payload)

            assert ws.close_code is None
            await ws.ping()

            await ws.send(json.dumps({"EventType": "END", "CallId": CALL_ID}))
            await eventually(
                lambda: (
                    db_conn.execute(
                        "SELECT status FROM meetings WHERE id = ?", (CALL_ID,)
                    ).fetchone()["status"]
                    == "COMPLETED"
                )
            )

            assert len(segments) == 2
            assert len(errors) == 1
            reset_error = errors[0]
            assert reset_error["Code"] == "STT_STREAM_RESET"
            assert reset_error.get("Context", {}).get("attempt") == 1

            assert segments[0]["StartTime"] == 0.0
            assert segments[1]["StartTime"] > segments[0]["StartTime"]

            row = db_conn.execute(
                "SELECT reconnect_attempts, time_offset_ms FROM meetings WHERE id = ?",
                (CALL_ID,),
            ).fetchone()
            assert row["reconnect_attempts"] == 1
            assert row["time_offset_ms"] > 0

            seg_row = db_conn.execute(
                "SELECT start_ms, end_ms FROM segments WHERE meeting_id = ? AND text = ?",
                (CALL_ID, "world"),
            ).fetchone()
            assert seg_row["start_ms"] > 0
            assert engine.engines_started == 2
    finally:
        stop.set()
        db_conn.close()
        await task
