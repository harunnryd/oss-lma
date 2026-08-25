import asyncio
import io
import json

import pytest
import pytest_asyncio
import websockets
from websockets.asyncio.client import connect

from lma_pipeline import SegmentAssembler
from lma_stt.types import WordItem

from sidecar.server import run_server

from tests.helpers import ScriptedEngine, eventually, sine_chunk

CALL_ID = "22222222-2222-2222-2222-222222222222"
QUERY_ID = "33333333-3333-3333-3333-333333333333"
CHUNK_BYTES = 19200


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


RESULTS = [
    {
        "result_id": "r1",
        "is_partial": True,
        "items": [word("hello", 0.0, 0.8, "CALLER", "r1")],
    },
    {
        "result_id": "r1",
        "is_partial": False,
        "items": [
            word("hello there", 0.0, 1.6, "CALLER", "r1"),
            word("hi", 1.6, 2.4, "AGENT", "r1"),
        ],
    },
]


def url(port: int, token: str) -> str:
    return f"ws://127.0.0.1:{port}/ws?token={token}"


@pytest_asyncio.fixture
async def live_sidecar():
    sink = io.StringIO()
    stop = asyncio.Event()
    task = asyncio.create_task(run_server(lambda ctx: ScriptedEngine(list(RESULTS)), stop=stop, ready_sink=sink))
    await eventually(lambda: "SIDECAR_READY" in sink.getvalue())
    port = int(sink.getvalue().split("port=")[1].split()[0])
    token = sink.getvalue().split("token=")[1].strip()
    yield port, token
    stop.set()
    await task


async def test_upgrade_without_token_gets_401(live_sidecar):
    port, _ = live_sidecar
    with pytest.raises(websockets.InvalidStatus) as exc_info:
        async with connect(f"ws://127.0.0.1:{port}/ws"):
            pass
    assert exc_info.value.response.status_code == 401


async def test_happy_path_streams_assembler_output_then_closes_cleanly(live_sidecar):
    port, token = live_sidecar
    reference = SegmentAssembler(CALL_ID)
    expected = [event for result in RESULTS for event in reference.on_result(result)]
    async with connect(url(port, token)) as ws:
        received = []

        async def reader():
            try:
                async for message in ws:
                    received.append(json.loads(message))
            except websockets.ConnectionClosed:
                pass

        pump = asyncio.create_task(reader())
        await ws.send(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
        chunk = sine_chunk(48000, 440, 880)
        assert len(chunk) == CHUNK_BYTES
        for _ in range(10):
            await ws.send(chunk)
        await eventually(lambda: len(received) == len(expected))
        assert received == expected
        await ws.send(json.dumps({"EventType": "END", "CallId": CALL_ID}))
        await asyncio.sleep(0.2)
        assert received == expected
        await ws.close(code=1000)
        await pump
    assert received == expected


async def test_invalid_text_frame_yields_error_then_close_1008(live_sidecar):
    port, token = live_sidecar
    async with connect(url(port, token)) as ws:
        await ws.send("{oops")
        error = json.loads(await ws.recv())
        assert error == {
            "EventType": "ERROR",
            "CallId": "",
            "Code": "LINK_DISCONNECTED",
            "Context": {"reason": "invalid-frame"},
        }
        with pytest.raises(websockets.ConnectionClosed) as exc_info:
            await ws.recv()
        assert exc_info.value.rcvd.code == 1008


async def test_wrong_size_binary_yields_error_then_close_1008(live_sidecar):
    port, token = live_sidecar
    async with connect(url(port, token)) as ws:
        await ws.send(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
        await ws.send(b"\x00" * 100)
        error = json.loads(await ws.recv())
        assert error["CallId"] == CALL_ID
        assert error["Code"] == "LINK_DISCONNECTED"
        with pytest.raises(websockets.ConnectionClosed) as exc_info:
            await ws.recv()
        assert exc_info.value.rcvd.code == 1008


async def test_agent_query_receives_status_thinking_step(live_sidecar):
    port, token = live_sidecar
    async with connect(url(port, token)) as ws:
        await ws.send(json.dumps({
            "EventType": "AGENT_QUERY",
            "CallId": CALL_ID,
            "QueryId": QUERY_ID,
            "Message": "What did we just discuss?",
            "History": [],
        }))
        event = json.loads(await ws.recv())
        assert event == {
            "EventType": "THINKING_STEP",
            "CallId": CALL_ID,
            "QueryId": QUERY_ID,
            "Seq": 0,
            "StepType": "status",
            "Content": "agent unavailable in P1",
        }
        await ws.close(code=1000)
