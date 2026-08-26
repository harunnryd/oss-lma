import asyncio
import io
import json
import re

import pytest
import websockets
from websockets.asyncio.client import connect

from sidecar.server import MAX_BIND_ATTEMPTS, BindFailed, authorize, run_server

from tests.helpers import READY_LINE, ScriptedEngine, eventually, spawn_sidecar

CALL_ID = "11111111-1111-1111-1111-111111111111"
TOKEN = "a" * 32


def test_authorize_accepts_matching_token_on_ws_path():
    assert authorize("/ws?token=" + TOKEN, TOKEN) is True


def test_authorize_rejects_stale_token_after_respawn():
    assert authorize("/ws?token=" + "b" * 32, TOKEN) is False


def test_authorize_rejects_missing_token():
    assert authorize("/ws", TOKEN) is False


def test_authorize_rejects_non_ws_paths():
    assert authorize("/health?token=" + TOKEN, TOKEN) is False


def test_max_bind_attempts_matches_catalog():
    assert MAX_BIND_ATTEMPTS == 10


async def test_ready_line_is_the_only_stdout_output():
    sink = io.StringIO()
    stop = asyncio.Event()
    task = asyncio.create_task(run_server(lambda ctx: ScriptedEngine([]), stop=stop, ready_sink=sink))
    try:
        await eventually(lambda: sink.getvalue() != "")
        assert READY_LINE.fullmatch(sink.getvalue()) is not None
    finally:
        stop.set()
        port, token = await task
    assert isinstance(port, int)
    assert re.fullmatch(r"[0-9a-f]{32}", token) is not None


async def test_upgrade_without_token_gets_401():
    stop, task, port, _ = await spawn_sidecar(lambda ctx: ScriptedEngine([]))
    try:
        with pytest.raises(websockets.InvalidStatus) as exc_info:
            async with connect(f"ws://127.0.0.1:{port}/ws"):
                pass
        assert exc_info.value.response.status_code == 401
    finally:
        stop.set()
        await task


async def test_upgrade_with_stale_token_gets_401():
    stop, task, port, _ = await spawn_sidecar(lambda ctx: ScriptedEngine([]))
    try:
        with pytest.raises(websockets.InvalidStatus) as exc_info:
            async with connect(f"ws://127.0.0.1:{port}/ws?token={'c' * 32}"):
                pass
        assert exc_info.value.response.status_code == 401
    finally:
        stop.set()
        await task


async def test_upgrade_on_non_ws_path_gets_404():
    stop, task, port, token = await spawn_sidecar(lambda ctx: ScriptedEngine([]))
    try:
        with pytest.raises(websockets.InvalidStatus) as exc_info:
            async with connect(f"ws://127.0.0.1:{port}/other?token={token}"):
                pass
        assert exc_info.value.response.status_code == 404
    finally:
        stop.set()
        await task


async def test_bind_exhaustion_raises_bind_failed(monkeypatch):
    async def refuse(*args, **kwargs):
        raise OSError("address in use")

    monkeypatch.setattr("sidecar.server.serve", refuse)
    with pytest.raises(BindFailed):
        await run_server(
            lambda ctx: ScriptedEngine([]),
            stop=asyncio.Event(),
            ready_sink=io.StringIO(),
        )


async def test_graceful_stop_closes_live_sessions_with_1000():
    engines = []

    def factory(ctx):
        engine = ScriptedEngine([])
        engines.append(engine)
        return engine

    stop, task, port, token = await spawn_sidecar(factory)
    try:
        async with connect(f"ws://127.0.0.1:{port}/ws?token={token}") as ws:
            await ws.send(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
            await eventually(lambda: len(engines) == 1)
            stop.set()
            with pytest.raises(websockets.ConnectionClosed) as exc_info:
                await ws.recv()
            assert exc_info.value.rcvd.code == 1000
    finally:
        port, token = await task
    assert engines[0].stream.closed is True
    assert task.result() == (port, token)


async def test_session_runs_over_real_socket():
    stop, task, port, token = await spawn_sidecar(lambda ctx: ScriptedEngine([]))
    try:
        async with connect(f"ws://127.0.0.1:{port}/ws?token={token}") as ws:
            await ws.send("{oops")
            error = json.loads(await ws.recv())
            assert error["Code"] == "LINK_DISCONNECTED"
            assert error["Context"] == {"reason": "invalid-frame"}
            with pytest.raises(websockets.ConnectionClosed) as exc_info:
                await ws.recv()
            assert exc_info.value.rcvd.code == 1008
    finally:
        stop.set()
        await task


async def test_run_server_accepts_db_writer_and_record_meeting_kwargs():
    stop = asyncio.Event()
    sink = io.StringIO()
    task = asyncio.create_task(
        run_server(
            lambda ctx: ScriptedEngine([]),
            stop=stop,
            ready_sink=sink,
            db_writer=None,
            record_meeting=False,
        )
    )
    try:
        await eventually(lambda: READY_LINE.fullmatch(sink.getvalue()) is not None)
        assert READY_LINE.fullmatch(sink.getvalue()) is not None
    finally:
        stop.set()
        port, token = await task
    assert isinstance(port, int)
    assert re.fullmatch(r"[0-9a-f]{32}", token) is not None


async def test_record_meeting_flag_threads_through_to_session():
    stop, task, port, token = await spawn_sidecar(
        lambda ctx: ScriptedEngine([]),
        record_meeting=True,
    )
    try:
        async with connect(f"ws://127.0.0.1:{port}/ws?token={token}") as ws:
            await ws.send(json.dumps({"EventType": "START", "CallId": CALL_ID, "SamplingRate": 48000}))
            await eventually(lambda: True)
    finally:
        stop.set()
        await task
