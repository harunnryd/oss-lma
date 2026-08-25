import asyncio
import json

import pytest

from lma_stt.deepgram import DeepgramResultStream, map_message
from lma_stt.fixtures import load_fixture
from lma_stt.tests.transports import FakeTransport
from lma_stt.types import ProviderResetError


def collected(conn: FakeTransport, count: int) -> list[dict]:
    async def run():
        stream = DeepgramResultStream(conn)
        results = [await anext(stream) for _ in range(count)]
        await stream.close()
        return results

    return asyncio.run(run())


def test_maps_recorded_miniature_session_to_expected_results():
    messages, expected = load_fixture("deepgram", "two_channel_miniature")
    conn = FakeTransport(messages=[json.dumps(m) for m in messages])
    assert collected(conn, len(expected)) == expected


def test_close_sends_closestream_then_closes_transport():
    conn = FakeTransport()
    collected(conn, 0)
    assert conn.closed is True
    assert json.loads(conn.sent[-1]) == {"type": "CloseStream"}


def test_result_id_stable_across_partial_and_final():
    messages, _ = load_fixture("deepgram", "two_channel_miniature")
    partial = map_message(messages[0])
    final = map_message(messages[1])
    agent_final = map_message(messages[2])
    assert partial["result_id"] == "req-mini-1-1"
    assert final["result_id"] == partial["result_id"]
    assert agent_final["result_id"] == "req-mini-1-2"


def test_transport_exhaustion_without_close_is_a_reset():
    conn = FakeTransport()

    async def run():
        stream = DeepgramResultStream(conn)
        await anext(stream)

    with pytest.raises(ProviderResetError):
        asyncio.run(run())
