import asyncio
import io
import json
import re
from pathlib import Path

from lma_stt.types import WordItem
from websockets.asyncio.client import connect

from sidecar.server import run_server
from sidecar.storage.connection import open_db
from sidecar.storage.migrations import apply_migrations
from sidecar.storage.persistence import SqliteWriter

from tests.helpers import ScriptedEngine, eventually, sine_chunk

CALL_ID = "33333333-3333-3333-3333-333333333333"


async def test_e2e_segment_emissions_persist_to_db(tmp_path):
    db_path = tmp_path / "lma.db"
    conn = open_db(db_path)
    apply_migrations(conn, Path(__file__).resolve().parents[1] / "storage" / "migrations")
    writer = SqliteWriter(conn)
    sink = io.StringIO()
    stop = asyncio.Event()
    results = [
        {
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
    ]
    task = asyncio.create_task(
        run_server(
            lambda ctx: ScriptedEngine(results),
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
            await eventually(lambda: row_count(conn, "segments") == 1)
            assert row_count(conn, "segments") == 1
            row = conn.execute(
                "SELECT text FROM segments WHERE meeting_id = ?", (CALL_ID,)
            ).fetchone()
            assert row["text"] == "hello"
            await ws.send(json.dumps({"EventType": "END", "CallId": CALL_ID}))
    finally:
        stop.set()
        conn.close()
        await task


def row_count(conn, table):
    return conn.execute(f"SELECT COUNT(*) AS n FROM {table}").fetchone()["n"]
